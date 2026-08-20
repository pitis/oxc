use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_end_tag_span, svelte_start_tag_span, walk_svelte_elements},
};

fn expected_before_closing_bracket_diagnostic(
    expected: usize,
    actual: usize,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Expected {} before closing bracket, but {} found.",
        phrase(expected),
        phrase(actual)
    ))
    .with_help("Match the line breaks before the closing bracket to the configured style.")
    .with_label(span)
}

/// How the message spells a line-break count.
fn phrase(line_breaks: usize) -> String {
    match line_breaks {
        0 => "no line breaks".to_string(),
        1 => "1 line break".to_string(),
        count => format!("{count} line breaks"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Style {
    /// Put the closing bracket on a line of its own.
    Always,
    /// Keep it on the same line as the last attribute.
    Never,
}

impl Style {
    fn line_breaks(self) -> usize {
        match self {
            Self::Always => 1,
            Self::Never => 0,
        }
    }
}

/// The self-closing override, where either half may be left unset.
#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct SelfClosingTag {
    singleline: Option<Style>,
    multiline: Option<Style>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct HtmlClosingBracketNewLine {
    /// For a tag written on one line.
    singleline: Style,
    /// For a tag whose attributes span several lines.
    multiline: Style,
    /// Overrides for a self-closing tag.
    self_closing_tag: SelfClosingTag,
}

impl Default for HtmlClosingBracketNewLine {
    fn default() -> Self {
        Self {
            singleline: Style::Never,
            multiline: Style::Always,
            self_closing_tag: SelfClosingTag::default(),
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires — or forbids — a line break before a tag's closing bracket.
    ///
    /// ### Why is this bad?
    ///
    /// When a tag's attributes already span several lines, leaving the `>`
    /// trailing off the last one makes it easy to miss where the tag ends.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div
    ///   id="a"
    ///   class="b"></div>
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
    /// `singleline` defaults to `"never"` and `multiline` to `"always"`.
    /// `selfClosingTag` overrides either for a `<x />` tag.
    ///
    /// ```json
    /// {
    ///   "svelte/html-closing-bracket-new-line": [
    ///     "error",
    ///     { "singleline": "never", "multiline": "always" }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream rewrites the whitespace; the Svelte markup pass reports only.
    HtmlClosingBracketNewLine,
    svelte,
    style,
    config = HtmlClosingBracketNewLine,
    version = "1.80.0",
    short_description = "Require or forbid a line break before a tag's closing bracket.",
);

impl Rule for HtmlClosingBracketNewLine {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for HtmlClosingBracketNewLine {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            let start_tag = svelte_start_tag_span(element);
            // For `<x />` the bracket in question is the `/`, not the `>`.
            let bracket_len = if element.self_closing { 2 } else { 1 };
            if let Some(diagnostic) =
                self.check(start_tag, bracket_len, element.self_closing, false, source)
            {
                diagnostics.push(diagnostic);
            }
            if let Some(end_tag) = svelte_end_tag_span(element)
                && let Some(diagnostic) = self.check(end_tag, 1, false, true, source)
            {
                diagnostics.push(diagnostic);
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

impl HtmlClosingBracketNewLine {
    /// `bracket_len` is how many characters the closing bracket takes: 2 for
    /// `/>`, 1 for `>`.
    fn check(
        self,
        tag: Span,
        bracket_len: u32,
        self_closing: bool,
        is_end_tag: bool,
        source: &str,
    ) -> Option<OxcDiagnostic> {
        let bracket_start = tag.end.checked_sub(bracket_len)?;
        let body = source.get(tag.start as usize..bracket_start as usize)?;
        // Where the last token before the bracket ends.
        let previous_end = tag.start + u32::try_from(body.trim_end().len()).ok()?;
        let between = source.get(previous_end as usize..bracket_start as usize)?;

        let multiline = source
            .get(tag.start as usize..previous_end as usize)
            .is_some_and(|text| text.contains('\n'));
        let expected = self.style(self_closing, multiline).line_breaks();
        let actual = between.matches('\n').count();
        if actual == expected {
            return None;
        }
        // A closing tag cannot sensibly gain a line break, so upstream only
        // ever reports one that should lose them.
        if is_end_tag && expected != 0 {
            return None;
        }
        Some(expected_before_closing_bracket_diagnostic(
            expected,
            actual,
            Span::new(previous_end, bracket_start),
        ))
    }

    fn style(self, self_closing: bool, multiline: bool) -> Style {
        let override_style = if multiline {
            self.self_closing_tag.multiline
        } else {
            self.self_closing_tag.singleline
        };
        match (self_closing, override_style) {
            (true, Some(style)) => style,
            _ if multiline => self.multiline,
            _ => self.singleline,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HtmlClosingBracketNewLine;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let inverted =
            || Some(serde_json::json!([{ "singleline": "always", "multiline": "never" }]));
        let self_closing =
            || Some(serde_json::json!([{ "selfClosingTag": { "multiline": "never" } }]));
        let pass = vec![
            ("<div id=\"a\"></div>", None, None, path()),
            ("<div\n\tid=\"a\"\n\tclass=\"b\"\n></div>", None, None, path()),
            ("<input\n\tid=\"a\"\n/>", None, None, path()),
            ("<input id=\"a\" />", None, None, path()),
            ("<div\n\tid=\"a\" />", self_closing(), None, path()),
            ("<div\n>a</div\n>", inverted(), None, path()),
            ("<div></div>", None, None, path()),
        ];
        let fail = vec![
            ("<div\n\tid=\"a\"\n\tclass=\"b\"></div>", None, None, path()),
            ("<input\n\tid=\"a\" />", None, None, path()),
            ("<div\n>a</div>", None, None, path()),
            ("<div id=\"a\"\n></div>", None, None, path()),
            ("<div id=\"a\"></div>", inverted(), None, path()),
            ("<input\n\tid=\"a\"\n/>", self_closing(), None, path()),
        ];

        Tester::new(HtmlClosingBracketNewLine::NAME, HtmlClosingBracketNewLine::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
