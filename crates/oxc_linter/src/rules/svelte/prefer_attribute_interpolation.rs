use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, walk_svelte_elements},
};

fn prefer_attribute_interpolation_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Use attribute interpolation instead of a template literal.")
        .with_help("Write `attr=\"prefix{expr}suffix\"` instead of `attr={`prefix${expr}suffix`}`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferAttributeInterpolation;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers Svelte's own attribute interpolation over a JavaScript
    /// template literal.
    ///
    /// ### Why is this bad?
    ///
    /// `class="a {b} c"` is Svelte's native syntax for the same thing and
    /// reads as markup rather than as an expression escape hatch.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class={`a ${b} c`}></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class="a {b} c"></div>
    /// <div class={cls}></div>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream additionally leaves alone a template literal whose static
    /// parts carry a string escape that would change meaning once moved into
    /// the attribute (`\n`, `A`, …). oxlint only skips the cases
    /// upstream also skips for structural reasons — a newline or a `{` in a
    /// static part — so a literal relying on such an escape is reported.
    PreferAttributeInterpolation,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Prefer attribute interpolation over a template literal.",
);

impl Rule for PreferAttributeInterpolation {}

impl SvelteTemplateRule for PreferAttributeInterpolation {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let allocator = Allocator::new();
        let mut spans = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let AttributeKind::Plain { value: Some(value), .. } = &attribute.kind else {
                    continue;
                };
                if let Some(span) = interpolatable_template(value, &allocator) {
                    spans.push(span);
                }
            }
        });
        for span in spans {
            ctx.diagnostic(prefer_attribute_interpolation_diagnostic(span));
        }
    }
}

/// The span of an `attr={`…`}` value whose template literal could be written
/// as Svelte attribute interpolation instead.
fn interpolatable_template(value: &AttributeValue<'_>, allocator: &Allocator) -> Option<Span> {
    // Only a value that is exactly one `{…}` part, like upstream.
    let expression = value.as_single_expression()?;
    let parsed = parse_svelte_expression(allocator, expression.expression)?;
    let Expression::TemplateLiteral(template) = parsed.get_inner_expression() else {
        return None;
    };
    // A template with no substitutions is just a string; upstream leaves it.
    if template.expressions.is_empty() {
        return None;
    }
    // A newline would not survive the move, and a `{` in a static part would
    // start a new Svelte interpolation.
    let movable = template.quasis.iter().all(|quasi| !quasi.value.raw.contains(['\n', '\r', '{']));
    movable.then_some(expression.span)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::PreferAttributeInterpolation;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<div class=\"a {b} c\"></div>", None, None, path()),
            ("<div class={cls}></div>", None, None, path()),
            // No substitutions: not an interpolation.
            ("<div class={`static`}></div>", None, None, path()),
            // A newline in a static part cannot move into the attribute.
            ("<div class={`a\n${b}`}></div>", None, None, path()),
            // A `{` in a static part would open a Svelte interpolation.
            ("<div class={`a{ ${b}`}></div>", None, None, path()),
            // Not a lone expression value.
            ("<div class=\"x {`a ${b}`}\"></div>", None, None, path()),
        ];
        let fail = vec![
            ("<div class={`a ${b} c`}></div>", None, None, path()),
            ("<div title={`${a}`}></div>", None, None, path()),
            ("<div data-x={`${a}-${b}`}></div>", None, None, path()),
        ];

        Tester::new(
            PreferAttributeInterpolation::NAME,
            PreferAttributeInterpolation::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
