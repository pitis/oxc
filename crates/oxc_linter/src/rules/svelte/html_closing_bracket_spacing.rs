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

fn expected_space_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected space before '>', but not found.")
        .with_help("Put a space before the closing bracket.")
        .with_label(span)
}

fn unexpected_space_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected no space before '>', but found.")
        .with_help("Remove the space before the closing bracket.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Spacing {
    /// Require a space before `>`.
    Always,
    /// Require no space before `>`.
    #[default]
    Never,
    /// Leave the tag alone.
    Ignore,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
// The shared `tag` suffix is upstream's option naming, which the config has
// to match.
#[expect(clippy::struct_field_names)]
pub struct HtmlClosingBracketSpacing {
    /// `<div >` — an opening tag that is not self-closing.
    start_tag: Spacing,
    /// `</div >` — a closing tag.
    end_tag: Spacing,
    /// `<div />` — a self-closing tag.
    self_closing_tag: Spacing,
}

impl Default for HtmlClosingBracketSpacing {
    fn default() -> Self {
        Self {
            start_tag: Spacing::Never,
            end_tag: Spacing::Never,
            self_closing_tag: Spacing::Always,
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires — or forbids — a space before a tag's closing bracket.
    ///
    /// ### Why is this bad?
    ///
    /// `<div >` and `<div/>` read as typos next to their conventional
    /// spellings, and mixing both in one file is noise.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div ></div >
    /// <input/>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div></div>
    /// <input />
    /// ```
    ///
    /// ### Options
    ///
    /// Each tag position takes `"always"`, `"never"` or `"ignore"`. The
    /// defaults match the conventional spelling: `startTag` and `endTag` are
    /// `"never"`, `selfClosingTag` is `"always"`.
    ///
    /// ```json
    /// {
    ///   "svelte/html-closing-bracket-spacing": [
    ///     "error",
    ///     { "startTag": "never", "endTag": "never", "selfClosingTag": "always" }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream adds or removes the space; the Svelte markup pass reports
    /// only.
    HtmlClosingBracketSpacing,
    svelte,
    style,
    config = HtmlClosingBracketSpacing,
    version = "1.80.0",
    short_description = "Require or forbid a space before a tag's closing bracket.",
);

impl Rule for HtmlClosingBracketSpacing {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for HtmlClosingBracketSpacing {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            let start_tag = svelte_start_tag_span(element);
            // A void element written without a slash is an ordinary start
            // tag; only `/>` makes it self-closing.
            let expected =
                if element.self_closing { self.self_closing_tag } else { self.start_tag };
            if let Some(diagnostic) = check(expected, start_tag, source) {
                diagnostics.push(diagnostic);
            }
            if let Some(end_tag) = svelte_end_tag_span(element, source)
                && let Some(diagnostic) = check(self.end_tag, end_tag, source)
            {
                diagnostics.push(diagnostic);
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Check one tag's trailing `…>` or `… />`, reporting the run of whitespace
/// that is there (or the empty span where one should be).
fn check(expected: Spacing, tag: Span, source: &str) -> Option<OxcDiagnostic> {
    if expected == Spacing::Ignore {
        return None;
    }
    let text = source.get(tag.start as usize..tag.end as usize)?;
    // Split off the trailing `>` or `/>`, then the whitespace before it.
    let before_bracket = text.strip_suffix('>')?;
    let before_slash = before_bracket.strip_suffix('/').unwrap_or(before_bracket);
    let spaces = &before_slash[before_slash.trim_end().len()..];
    // A tag broken across lines is `svelte/html-closing-bracket-new-line`'s
    // business, not this rule's.
    if spaces.contains('\n') {
        return None;
    }
    // Report the whitespace plus the bracket, as upstream does — so an
    // "expected a space" report still has the bracket to point at.
    let reported = spaces.len() + (text.len() - before_slash.len());
    let span = Span::new(tag.end - u32::try_from(reported).ok()?, tag.end);
    match (expected, spaces.is_empty()) {
        (Spacing::Always, true) => Some(expected_space_diagnostic(span)),
        (Spacing::Never, false) => Some(unexpected_space_diagnostic(span)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HtmlClosingBracketSpacing;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let always = || Some(serde_json::json!([{ "startTag": "always", "endTag": "always" }]));
        let ignore = || {
            Some(serde_json::json!([
                { "startTag": "ignore", "endTag": "ignore", "selfClosingTag": "ignore" }
            ]))
        };
        let pass = vec![
            ("<div></div>", None, None, path()),
            ("<input />", None, None, path()),
            ("<div class=\"a\"></div>", None, None, path()),
            ("<div ></div >", always(), None, path()),
            ("<div ></div >", ignore(), None, path()),
            ("<input/>", ignore(), None, path()),
            // A tag broken across lines is another rule's business.
            ("<div\n></div>", None, None, path()),
            // A void element with no slash is an ordinary start tag.
            ("<br>", None, None, path()),
        ];
        let fail = vec![
            ("<div ></div>", None, None, path()),
            ("<div></div >", None, None, path()),
            ("<input/>", None, None, path()),
            ("<div></div>", always(), None, path()),
            ("<div class=\"a\" ></div>", None, None, path()),
            ("<br >", None, None, path()),
        ];

        Tester::new(HtmlClosingBracketSpacing::NAME, HtmlClosingBracketSpacing::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
