use std::num::NonZeroU32;

use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{Attribute, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn require_newline_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{name}' should be on a new line."))
        .with_help("Put the attribute on a line of its own.")
        .with_label(span)
}

// `u32` rather than `usize`: two `usize`s would take the whole
// 16-byte `RuleEnum` budget on their own.
fn one() -> NonZeroU32 {
    NonZeroU32::MIN
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MaxAttributesPerLine {
    /// How many attributes a line may carry when the tag spans several lines.
    #[schemars(with = "u32")]
    multiline: NonZeroU32,
    /// How many the tag may carry when it is written on one line.
    #[schemars(with = "u32")]
    singleline: NonZeroU32,
}

impl Default for MaxAttributesPerLine {
    fn default() -> Self {
        Self { multiline: one(), singleline: one() }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Limits how many attributes may share a line.
    ///
    /// ### Why is this bad?
    ///
    /// A tag with many attributes on one long line is hard to scan and
    /// produces diffs that touch every attribute when only one changes.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div id="a" class="b"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div
    ///   id="a"
    ///   class="b"
    /// ></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `singleline` and `multiline` both default to `1`.
    ///
    /// ```json
    /// {
    ///   "svelte/max-attributes-per-line": [
    ///     "error",
    ///     { "singleline": 1, "multiline": 1 }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream inserts the line break; the Svelte markup pass reports only.
    MaxAttributesPerLine,
    svelte,
    style,
    config = MaxAttributesPerLine,
    version = "1.80.0",
    short_description = "Limit how many attributes share a line.",
);

impl Rule for MaxAttributesPerLine {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for MaxAttributesPerLine {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            if element.attributes.is_empty() {
                return;
            }
            let mut report = |attribute: Option<&Attribute<'_>>| {
                if let Some(attribute) = attribute {
                    let name = attribute_name(attribute, source);
                    diagnostics.push(require_newline_diagnostic(name, attribute.span));
                }
            };

            // The whole opening tag on one line, or spread over several?
            let start_tag = &source[element.span.start as usize..element.open_tag_end as usize];
            if start_tag.contains('\n') {
                for line in group_by_line(&element.attributes, source) {
                    report(line.get(self.multiline.get() as usize).copied());
                }
            } else if element.attributes.len() > self.singleline.get() as usize {
                report(element.attributes.get(self.singleline.get() as usize));
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// The attributes grouped into the lines they sit on. An attribute joins the
/// current line when it starts on the line the group's *first* attribute
/// ends on, which is how upstream groups them.
fn group_by_line<'e, 'a>(
    attributes: &'e [Attribute<'a>],
    source: &str,
) -> Vec<Vec<&'e Attribute<'a>>> {
    let mut groups: Vec<Vec<&'e Attribute<'a>>> = Vec::new();
    for attribute in attributes {
        let same_line = groups.last().and_then(|group| group.first()).is_some_and(|first| {
            !source[first.span.end as usize..attribute.span.start as usize].contains('\n')
        });
        if same_line {
            groups.last_mut().expect("checked above").push(attribute);
        } else {
            groups.push(vec![attribute]);
        }
    }
    groups
}

/// How the message names an attribute: its written key, or the whole thing
/// for a spread, which has no key.
fn attribute_name<'a>(attribute: &Attribute<'a>, source: &'a str) -> &'a str {
    attribute.name().unwrap_or(&source[attribute.span.start as usize..attribute.span.end as usize])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::MaxAttributesPerLine;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let two = || Some(serde_json::json!([{ "singleline": 2, "multiline": 2 }]));
        let pass = vec![
            ("<div id=\"a\"></div>", None, None, path()),
            ("<div></div>", None, None, path()),
            ("<div\n\tid=\"a\"\n\tclass=\"b\"\n></div>", None, None, path()),
            ("<div id=\"a\" class=\"b\"></div>", two(), None, path()),
            ("<div\n\tid=\"a\" class=\"b\"\n></div>", two(), None, path()),
        ];
        let fail = vec![
            ("<div id=\"a\" class=\"b\"></div>", None, None, path()),
            ("<div\n\tid=\"a\" class=\"b\"\n></div>", None, None, path()),
            ("<div id=\"a\" class=\"b\" role=\"c\"></div>", two(), None, path()),
            // A spread is named by its whole text.
            ("<div {...props} id=\"a\"></div>", None, None, path()),
            // Directives are named by their written key.
            ("<input bind:value type=\"text\" />", None, None, path()),
        ];

        Tester::new(MaxAttributesPerLine::NAME, MaxAttributesPerLine::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
