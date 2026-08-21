use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn expected_linebreak_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected a linebreak before this attribute.")
        .with_help("Put the first attribute on its own line.")
        .with_label(span)
}

fn unexpected_linebreak_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected no linebreak before this attribute.")
        .with_help("Put the first attribute on the same line as the tag name.")
        .with_label(span)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LinebreakLocation {
    /// The first attribute goes on the line after the tag name.
    Below,
    /// The first attribute goes on the same line as the tag name.
    Beside,
    /// Not checked.
    Ignore,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct FirstAttributeLinebreak {
    /// Applied when the attributes span more than one line.
    pub multiline: LinebreakLocation,
    /// Applied when they all fit on one line.
    pub singleline: LinebreakLocation,
}

impl Default for FirstAttributeLinebreak {
    fn default() -> Self {
        Self { multiline: LinebreakLocation::Below, singleline: LinebreakLocation::Ignore }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces where the first attribute of a multi-line tag goes: on the
    /// line after the tag name, or beside it.
    ///
    /// ### Why is this bad?
    ///
    /// Purely a consistency rule, and only for tags whose attributes already
    /// wrap. Fixing the first attribute's position makes the attribute block
    /// line up, so diffs touch one attribute rather than re-indenting the
    /// whole tag.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (with the defaults):
    /// ```vue
    /// <template>
    ///   <MyComponent id="a"
    ///     name="b" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent
    ///     id="a"
    ///     name="b" />
    ///   <MyComponent id="a" name="b" />
    /// </template>
    /// ```
    ///
    /// ### Options
    ///
    /// `{ multiline, singleline }`, each `"below"`, `"beside"` or `"ignore"`.
    /// Defaults are `multiline: "below"` and `singleline: "ignore"`.
    ///
    /// ```json
    /// { "vue/first-attribute-linebreak": ["error", { "singleline": "beside" }] }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream is auto-fixable; this is not, because the `<template>` pass
    /// cannot emit fixes yet.
    FirstAttributeLinebreak,
    vue,
    style,
    config = FirstAttributeLinebreak,
    version = "1.80.0",
    short_description = "Enforce the location of the first attribute.",
);

impl Rule for FirstAttributeLinebreak {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for FirstAttributeLinebreak {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let lines = LineIndex::new(ctx.source_text());
        let mut reports = Vec::new();
        walk_elements(nodes, &mut |element| {
            let Some(first) = element.attributes.first() else { return };
            let last = element.attributes.last().unwrap_or(first);

            let first_line = lines.line_of(first.span.start);
            let location = if first_line == lines.line_of(last.span.end) {
                self.singleline
            } else {
                self.multiline
            };

            match location {
                LinebreakLocation::Ignore => {}
                LinebreakLocation::Beside => {
                    if lines.line_of(element.span.start) != first_line {
                        reports.push(unexpected_linebreak_diagnostic(first.span));
                    }
                }
                LinebreakLocation::Below => {
                    if lines.line_of(element.span.start) >= first_line {
                        reports.push(expected_linebreak_diagnostic(first.span));
                    }
                }
            }
        });
        for diagnostic in reports {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Line numbers for a handful of offsets, without re-scanning the file for
/// each one.
struct LineIndex {
    line_starts: Vec<u32>,
}

impl LineIndex {
    fn new(source_text: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source_text
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| u32::try_from(index + 1).unwrap_or(u32::MAX)),
        );
        Self { line_starts }
    }

    fn line_of(&self, offset: u32) -> usize {
        self.line_starts.partition_point(|start| *start <= offset)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::FirstAttributeLinebreak;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            // Single-line tags are ignored by default.
            ("<template><MyComponent id=\"a\" name=\"b\" /></template>", None, None, vue()),
            // Multi-line with the first attribute below the tag name.
            (
                "<template>\n  <MyComponent\n    id=\"a\"\n    name=\"b\" />\n</template>",
                None,
                None,
                vue(),
            ),
            // No attributes at all.
            ("<template><div /></template>", None, None, vue()),
            // `beside` satisfied.
            (
                "<template>\n  <MyComponent id=\"a\"\n    name=\"b\" />\n</template>",
                Some(json!([{ "multiline": "beside" }])),
                None,
                vue(),
            ),
        ];

        let fail = vec![
            // Multi-line, first attribute beside the tag name.
            (
                "<template>\n  <MyComponent id=\"a\"\n    name=\"b\" />\n</template>",
                None,
                None,
                vue(),
            ),
            // `beside` violated.
            (
                "<template>\n  <MyComponent\n    id=\"a\"\n    name=\"b\" />\n</template>",
                Some(json!([{ "multiline": "beside" }])),
                None,
                vue(),
            ),
            // Single-line opted into `below`.
            (
                "<template>\n  <MyComponent id=\"a\" name=\"b\" />\n</template>",
                Some(json!([{ "singleline": "below" }])),
                None,
                vue(),
            ),
        ];

        Tester::new(FirstAttributeLinebreak::NAME, FirstAttributeLinebreak::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
