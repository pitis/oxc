use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, Node, ValuePart};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, walk_svelte_elements},
};

fn expected_shorthand_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected shorthand attribute.")
        .with_help(format!("Write it as `{{{name}}}`."))
        .with_label(span)
}

fn expected_regular_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected regular attribute syntax.")
        .with_help(format!("Write it as `{name}={{{name}}}`."))
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Prefer {
    /// Require `{name}` wherever it is available.
    #[default]
    Always,
    /// Require the written-out `name={name}`.
    Never,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ShorthandAttribute {
    /// Which form to require.
    prefer: Prefer,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the shorthand form of an attribute whose value is just the
    /// identically named variable — or, with `prefer: "never"`, requires the
    /// written-out form instead.
    ///
    /// ### Why is this bad?
    ///
    /// `name={name}` repeats itself, and mixing the two forms in one
    /// component makes the markup read inconsistently.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <Widget foo={foo} />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <Widget {foo} />
    /// ```
    ///
    /// ### Options
    ///
    /// `prefer` is `"always"` by default. `"never"` reports the shorthand
    /// form instead.
    ///
    /// ```json
    /// { "svelte/shorthand-attribute": ["error", { "prefer": "never" }] }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream rewrites the attribute; the Svelte markup pass reports only.
    ShorthandAttribute,
    svelte,
    style,
    config = ShorthandAttribute,
    version = "1.80.0",
    short_description = "Enforce the shorthand form of an attribute.",
);

impl Rule for ShorthandAttribute {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for ShorthandAttribute {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let allocator = Allocator::new();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                match &attribute.kind {
                    AttributeKind::Plain { name, value: Some(value), .. }
                        if self.prefer == Prefer::Always =>
                    {
                        let [ValuePart::Expression(tag)] = value.parts.as_slice() else { continue };
                        if is_identifier_named(&allocator, tag.expression, name) {
                            diagnostics.push(expected_shorthand_diagnostic(name, attribute.span));
                        }
                    }
                    AttributeKind::Shorthand { name, .. } if self.prefer == Prefer::Never => {
                        diagnostics.push(expected_regular_diagnostic(name, attribute.span));
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

    use super::ShorthandAttribute;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let never = || Some(serde_json::json!([{ "prefer": "never" }]));
        let pass = vec![
            ("<Widget {foo} />", None, None, path()),
            // The names differ, so there is no shorthand for it.
            ("<Widget foo={bar} />", None, None, path()),
            // Not a bare identifier.
            ("<Widget foo={foo.bar} />", None, None, path()),
            ("<Widget foo=\"foo\" />", None, None, path()),
            // More than one value part.
            ("<Widget foo=\"a{foo}\" />", None, None, path()),
            // A bare boolean attribute.
            ("<Widget foo />", None, None, path()),
            ("<Widget foo={foo} />", never(), None, path()),
            // Directives are `svelte/shorthand-directive`'s business.
            ("<input bind:value={value} />", None, None, path()),
        ];
        let fail = vec![
            ("<Widget foo={foo} />", None, None, path()),
            // Quoted around the mustache counts too.
            ("<Widget foo=\"{foo}\" />", None, None, path()),
            ("<Widget foo={ foo } />", None, None, path()),
            ("<Widget {foo} />", never(), None, path()),
        ];

        Tester::new(ShorthandAttribute::NAME, ShorthandAttribute::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
