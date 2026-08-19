use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node, ValuePart};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, walk_svelte_elements},
};

fn expected_shorthand_diagnostic(raw_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected shorthand directive.")
        .with_help(format!("Write it as `{raw_name}`."))
        .with_label(span)
}

fn expected_regular_diagnostic(raw_name: &str, name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected regular directive syntax.")
        .with_help(format!("Write it as `{raw_name}={{{name}}}`."))
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Prefer {
    /// Require `bind:value` wherever it is available.
    #[default]
    Always,
    /// Require the written-out `bind:value={value}`.
    Never,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ShorthandDirective {
    /// Which form to require.
    prefer: Prefer,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the shorthand form of a `bind:`, `class:` or `style:`
    /// directive whose value is just the identically named variable — or,
    /// with `prefer: "never"`, requires the written-out form instead.
    ///
    /// ### Why is this bad?
    ///
    /// `bind:value={value}` repeats itself, and mixing the two forms in one
    /// component makes the markup read inconsistently.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <input bind:value={value} />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <input bind:value />
    /// ```
    ///
    /// ### Options
    ///
    /// `prefer` is `"always"` by default. `"never"` reports the shorthand
    /// form instead.
    ///
    /// ```json
    /// { "svelte/shorthand-directive": ["error", { "prefer": "never" }] }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream rewrites the directive; the Svelte markup pass reports only.
    ShorthandDirective,
    svelte,
    style,
    config = ShorthandDirective,
    version = "1.80.0",
    short_description = "Enforce the shorthand form of a directive.",
);

impl Rule for ShorthandDirective {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for ShorthandDirective {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let allocator = Allocator::new();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let AttributeKind::Directive(directive) = &attribute.kind else { continue };
                // Only these three have a shorthand form.
                if !matches!(
                    directive.kind,
                    DirectiveKind::Bind | DirectiveKind::Class | DirectiveKind::Style
                ) {
                    continue;
                }
                match (&directive.value, self.prefer) {
                    (None, Prefer::Never) => diagnostics.push(expected_regular_diagnostic(
                        directive.raw_name,
                        directive.name,
                        attribute.span,
                    )),
                    (Some(value), Prefer::Always) => {
                        let [ValuePart::Expression(tag)] = value.parts.as_slice() else { continue };
                        if is_identifier_named(&allocator, tag.expression, directive.name) {
                            diagnostics.push(expected_shorthand_diagnostic(
                                directive.raw_name,
                                attribute.span,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Whether an expression is exactly the bare identifier `name`.
fn is_identifier_named(allocator: &Allocator, text: &str, name: &str) -> bool {
    matches!(
        parse_svelte_expression(allocator, text),
        Some(Expression::Identifier(identifier)) if identifier.name == name
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ShorthandDirective;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let never = || Some(serde_json::json!([{ "prefer": "never" }]));
        let pass = vec![
            ("<input bind:value />", None, None, path()),
            ("<div class:active />", None, None, path()),
            ("<div style:color />", None, None, path()),
            // The names differ, so there is no shorthand for it.
            ("<input bind:value={other} />", None, None, path()),
            ("<div class:active={isActive} />", None, None, path()),
            // `bind:this` can never be shortened.
            ("<div bind:this={element} />", None, None, path()),
            // Other directive kinds have no shorthand at all.
            ("<button on:click={click} />", None, None, path()),
            ("<div use:action={action} />", None, None, path()),
            ("<input bind:value={value} />", never(), None, path()),
        ];
        let fail = vec![
            ("<input bind:value={value} />", None, None, path()),
            ("<div class:active={active} />", None, None, path()),
            ("<div style:color={color} />", None, None, path()),
            ("<input bind:value />", never(), None, path()),
            ("<div class:active />", never(), None, path()),
        ];

        Tester::new(ShorthandDirective::NAME, ShorthandDirective::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
