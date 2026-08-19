use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn expected_space_after_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected space or tab after '<!--' in comment.")
        .with_help("Write `<!-- comment -->`.")
        .with_label(span)
}

fn expected_space_before_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected space or tab before '-->' in comment.")
        .with_help("Write `<!-- comment -->`.")
        .with_label(span)
}

fn unexpected_space_after_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected space or tab after '<!--' in comment.")
        .with_help("Write `<!--comment-->`.")
        .with_label(span)
}

fn unexpected_space_before_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected space or tab before '-->' in comment.")
        .with_help("Write `<!--comment-->`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Style {
    /// Require a space or tab inside the comment markers.
    #[default]
    Always,
    /// Disallow a space or tab inside the comment markers.
    Never,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SpacedHtmlComment(Style);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces consistent spacing after `<!--` and before `-->` in an HTML
    /// comment.
    ///
    /// ### Why is this bad?
    ///
    /// Nothing breaks either way; this is a consistency rule, like
    /// `eslint/spaced-comment` for JavaScript.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (default `"always"`):
    /// ```svelte
    /// <!--comment-->
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <!-- comment -->
    /// ```
    ///
    /// ### Options
    ///
    /// `"always"` (default) or `"never"`.
    ///
    /// ```json
    /// { "svelte/spaced-html-comment": ["error", "never"] }
    /// ```
    SpacedHtmlComment,
    svelte,
    style,
    config = SpacedHtmlComment,
    version = "1.80.0",
    short_description = "Enforce consistent spacing inside HTML comment markers.",
);

impl Rule for SpacedHtmlComment {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let style = match value.get(0).and_then(serde_json::Value::as_str) {
            Some("never") => Style::Never,
            _ => Style::Always,
        };
        Ok(Self(style))
    }
}

impl SvelteTemplateRule for SpacedHtmlComment {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut diagnostics = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| {
            let Node::Comment(comment) = node else { return };
            // An unterminated comment has no `-->` to space, and a blank
            // comment (`<!---->`) is exempt upstream.
            if comment.unterminated || comment.content.trim().is_empty() {
                return;
            }
            let content = comment.content;
            match self.0 {
                Style::Always => {
                    if content.starts_with(|c: char| !c.is_whitespace()) {
                        diagnostics.push(expected_space_after_diagnostic(comment.span));
                    }
                    if content.ends_with(|c: char| !c.is_whitespace()) {
                        diagnostics.push(expected_space_before_diagnostic(comment.span));
                    }
                }
                Style::Never => {
                    // Upstream only rejects a space or tab, not a newline:
                    // a comment broken across lines is left alone.
                    if content.starts_with([' ', '\t']) {
                        diagnostics.push(unexpected_space_after_diagnostic(comment.span));
                    }
                    if content.ends_with([' ', '\t'])
                        && content
                            .trim_end_matches([' ', '\t'])
                            .ends_with(|c: char| !c.is_whitespace())
                    {
                        diagnostics.push(unexpected_space_before_diagnostic(comment.span));
                    }
                }
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SpacedHtmlComment;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let never = || Some(serde_json::json!(["never"]));
        let pass = vec![
            ("<!-- comment -->", None, None, path()),
            ("<!--\n\tcomment\n-->", None, None, path()),
            // Empty comments are exempt.
            ("<!---->", None, None, path()),
            ("<!-- -->", None, None, path()),
            ("<!--comment-->", never(), None, path()),
            // `never` only rejects spaces and tabs, not newlines.
            ("<!--\n\tcomment\n-->", never(), None, path()),
        ];
        let fail = vec![
            ("<!--comment-->", None, None, path()),
            ("<!-- comment-->", None, None, path()),
            ("<!--comment -->", None, None, path()),
            ("<!-- comment -->", never(), None, path()),
            ("<!-- comment-->", never(), None, path()),
        ];

        Tester::new(SpacedHtmlComment::NAME, SpacedHtmlComment::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
