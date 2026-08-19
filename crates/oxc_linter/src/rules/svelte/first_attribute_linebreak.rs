use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn expected_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected a linebreak before this attribute.")
        .with_help("Move the first attribute onto its own line.")
        .with_label(span)
}

fn unexpected_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected no linebreak before this attribute.")
        .with_help("Keep the first attribute on the same line as the tag name.")
        .with_label(span)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Location {
    /// On the line after the tag name.
    Below,
    /// On the same line as the tag name.
    Beside,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct FirstAttributeLinebreak {
    /// Where the first attribute goes when the attributes span more than one
    /// line.
    multiline: Location,
    /// Where it goes when they all fit on one.
    singleline: Location,
}

impl Default for FirstAttributeLinebreak {
    fn default() -> Self {
        Self { multiline: Location::Below, singleline: Location::Beside }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces where the first attribute of a tag goes: beside the tag name
    /// or on the line below it.
    ///
    /// ### Why is this bad?
    ///
    /// A tag whose attributes are spread over several lines reads better
    /// when the first one starts a line of its own, so every attribute lines
    /// up.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div id="a"
    ///   class="b"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div
    ///   id="a"
    ///   class="b"></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `multiline` is `"below"` by default and `singleline` is `"beside"`;
    /// either may be set to the other.
    ///
    /// ```json
    /// {
    ///   "svelte/first-attribute-linebreak": [
    ///     "error",
    ///     { "multiline": "below", "singleline": "beside" }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream moves the attribute; the Svelte markup pass reports only.
    FirstAttributeLinebreak,
    svelte,
    style,
    config = FirstAttributeLinebreak,
    version = "1.80.0",
    short_description = "Enforce where a tag's first attribute goes.",
);

impl Rule for FirstAttributeLinebreak {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for FirstAttributeLinebreak {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            let (Some(first), Some(last)) = (element.attributes.first(), element.attributes.last())
            else {
                return;
            };
            let all_on_one_line = !spans_a_line_break(source, first.span.start, last.span.end);
            let expected = if all_on_one_line { self.singleline } else { self.multiline };
            let name_and_first_are_split =
                spans_a_line_break(source, element.name_span.end, first.span.start);
            match expected {
                Location::Beside if name_and_first_are_split => {
                    diagnostics.push(unexpected_diagnostic(first.span));
                }
                Location::Below if !name_and_first_are_split => {
                    diagnostics.push(expected_diagnostic(first.span));
                }
                _ => {}
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Whether the source between two offsets crosses a line.
fn spans_a_line_break(source: &str, from: u32, to: u32) -> bool {
    source.get(from as usize..to as usize).is_some_and(|text| text.contains('\n'))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::FirstAttributeLinebreak;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let swapped =
            || Some(serde_json::json!([{ "multiline": "beside", "singleline": "below" }]));
        let pass = vec![
            ("<div id=\"a\" class=\"b\"></div>", None, None, path()),
            ("<div\n\tid=\"a\"\n\tclass=\"b\"></div>", None, None, path()),
            ("<div></div>", None, None, path()),
            ("<div id=\"a\"></div>", None, None, path()),
            ("<div id=\"a\"\n\tclass=\"b\"></div>", swapped(), None, path()),
            ("<div\n\tid=\"a\"></div>", swapped(), None, path()),
        ];
        let fail = vec![
            // Multiline attributes, but the first one is beside the name.
            ("<div id=\"a\"\n\tclass=\"b\"></div>", None, None, path()),
            // Single-line attributes, but the first one is below the name.
            ("<div\n\tid=\"a\" class=\"b\"></div>", None, None, path()),
            ("<div\n\tid=\"a\"></div>", None, None, path()),
            ("<div id=\"a\"></div>", swapped(), None, path()),
        ];

        Tester::new(FirstAttributeLinebreak::NAME, FirstAttributeLinebreak::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
