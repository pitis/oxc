use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, DirectiveKind, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn expected_enclosed_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected to be enclosed by quotes.")
        .with_help("Wrap the attribute value in quotes.")
        .with_label(span)
}

fn expected_enclosed_by_diagnostic(kind: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Expected to be enclosed by {kind}."))
        .with_help(format!("Rewrite the attribute value as {kind}."))
        .with_label(span)
}

fn unexpected_enclosed_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected to be enclosed by any quotes.")
        .with_help("Remove the quotes around the attribute value.")
        .with_label(span)
}

/// How an attribute value is (or should be) delimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    Double,
    Single,
    Unquoted,
}

impl Quote {
    fn character(self) -> Option<char> {
        match self {
            Self::Double => Some('"'),
            Self::Single => Some('\''),
            Self::Unquoted => None,
        }
    }

    /// How the rule names this form in its messages.
    fn noun(self) -> &'static str {
        match self {
            Self::Double => "double quotes",
            Self::Single => "single quotes",
            Self::Unquoted => "unquoted",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Prefer {
    #[default]
    Double,
    Single,
}

impl From<Prefer> for Quote {
    fn from(prefer: Prefer) -> Self {
        match prefer {
            Prefer::Double => Self::Double,
            Prefer::Single => Self::Single,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct Dynamic {
    /// Quote a value that is a single `{…}` expression too.
    quoted: bool,
    /// Quote a `{…}` value whose text could not legally be left unquoted in
    /// HTML, even when `quoted` is `false`.
    // Upstream spells `HTML` in capitals, which `rename_all` would not.
    #[serde(rename = "avoidInvalidUnquotedInHTML")]
    #[schemars(rename = "avoidInvalidUnquotedInHTML")]
    avoid_invalid_unquoted_in_html: bool,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct HtmlQuotes {
    /// Which quote character to require around a static value.
    prefer: Prefer,
    /// How to treat a value that is a single `{…}` expression.
    dynamic: Dynamic,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces one quoting style for HTML attribute values.
    ///
    /// ### Why is this bad?
    ///
    /// Mixing `"`, `'` and unquoted values in the same markup is noise, and
    /// an unquoted value silently ends at the first space or `>`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class='foo'></div>
    /// <div class=foo></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class="foo"></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `prefer` is `"double"` by default and may be `"single"`. A value that
    /// is a single `{…}` expression is expected to be unquoted unless
    /// `dynamic.quoted` is `true`; `dynamic.avoidInvalidUnquotedInHTML`
    /// additionally quotes an expression whose text could not legally be
    /// left unquoted in HTML.
    ///
    /// ```json
    /// {
    ///   "svelte/html-quotes": [
    ///     "error",
    ///     { "prefer": "double", "dynamic": { "quoted": false } }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream rewrites the quotes; the Svelte markup pass reports only.
    HtmlQuotes,
    svelte,
    style,
    config = HtmlQuotes,
    version = "1.80.0",
    short_description = "Enforce one quoting style for attribute values.",
);

impl Rule for HtmlQuotes {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for HtmlQuotes {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let prefer = Quote::from(self.prefer);
        // A `{…}` value is left unquoted unless `dynamic.quoted` asks for it.
        let dynamic = if self.dynamic.quoted { prefer } else { Quote::Unquoted };

        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let value = match &attribute.kind {
                    // `style:` directives take a value like a plain attribute.
                    AttributeKind::Plain { value: Some(value), .. } => value,
                    AttributeKind::Directive(directive) => {
                        let Some(value) = &directive.value else { continue };
                        if directive.kind != DirectiveKind::Style
                            && value.as_single_expression().is_none()
                        {
                            // Upstream only looks at a directive whose value
                            // is a single mustache.
                            continue;
                        }
                        value
                    }
                    _ => continue,
                };
                if value.parts.is_empty() || value.unterminated {
                    continue;
                }

                let expected = match value.as_single_expression() {
                    Some(tag) => {
                        // A `{…}` that could not legally sit unquoted in HTML
                        // is quoted anyway, when asked.
                        let text = &source[tag.span.start as usize..tag.span.end as usize];
                        if self.dynamic.avoid_invalid_unquoted_in_html
                            && !can_be_unquoted_in_html(text)
                        {
                            prefer
                        } else {
                            dynamic
                        }
                    }
                    None => prefer,
                };
                if let Some(diagnostic) = verify_quote(expected, value, source) {
                    diagnostics.push(diagnostic);
                }
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Whether the text could be left unquoted in HTML without changing where the
/// value ends.
fn can_be_unquoted_in_html(text: &str) -> bool {
    !text.contains([' ', '\t', '\n', '\r', '\u{c}', '"', '\'', '<', '=', '>', '`'])
}

/// The value's span including its quotes, when it has any.
fn quoted_span(value: &AttributeValue<'_>) -> Span {
    if value.quote == 0 { value.span } else { Span::new(value.span.start - 1, value.span.end + 1) }
}

fn verify_quote(
    expected: Quote,
    value: &AttributeValue<'_>,
    source: &str,
) -> Option<OxcDiagnostic> {
    let actual = match value.quote {
        b'"' => Quote::Double,
        b'\'' => Quote::Single,
        _ => Quote::Unquoted,
    };
    if actual == expected {
        return None;
    }
    let span = quoted_span(value);
    // The value's own text, quotes excluded either way.
    let content = &source[value.span.start as usize..value.span.end as usize];

    if actual != Quote::Unquoted {
        if expected == Quote::Unquoted {
            return Some(unexpected_enclosed_diagnostic(span));
        }
        // Requoting would mean escaping, so leave it alone.
        if expected.character().is_some_and(|quote| content.contains(quote)) {
            return None;
        }
        return Some(expected_enclosed_by_diagnostic(expected.noun(), span));
    }

    // Currently unquoted: pick the quote that does not need escaping.
    let has_double = content.contains('"');
    let has_single = content.contains('\'');
    if has_double && has_single {
        return None;
    }
    if has_double && expected == Quote::Double || has_single && expected == Quote::Single {
        return Some(expected_enclosed_diagnostic(span));
    }
    Some(expected_enclosed_by_diagnostic(expected.noun(), span))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HtmlQuotes;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let single = || Some(serde_json::json!([{ "prefer": "single" }]));
        let dynamic_quoted = || Some(serde_json::json!([{ "dynamic": { "quoted": true } }]));
        let avoid_invalid =
            || Some(serde_json::json!([{ "dynamic": { "avoidInvalidUnquotedInHTML": true } }]));
        let pass = vec![
            ("<div class=\"foo\"></div>", None, None, path()),
            ("<div class='foo'></div>", single(), None, path()),
            // A single mustache is expected to be unquoted by default.
            ("<div class={foo}></div>", None, None, path()),
            ("<div class=\"{foo}\"></div>", dynamic_quoted(), None, path()),
            // A bare boolean attribute has no value to quote.
            ("<div hidden></div>", None, None, path()),
            // Requoting would need escaping, so it is left alone.
            ("<div title='say \"hi\"'></div>", None, None, path()),
            // Unquoted and containing both quote characters. Svelte itself
            // rejects a quote inside an unquoted value, so this only reaches
            // the rule through the recovering markup parser.
            ("<div title=a\"b'c></div>", None, None, path()),
            // A shorthand attribute has no value.
            ("<Widget {foo} />", None, None, path()),
            // A shorthand directive has no value.
            ("<input bind:value />", None, None, path()),
            // `{foo}` needs no quotes even when asked to avoid invalid ones.
            ("<div class={foo}></div>", avoid_invalid(), None, path()),
        ];
        let fail = vec![
            ("<div class='foo'></div>", None, None, path()),
            ("<div class=foo></div>", None, None, path()),
            ("<div class=\"foo\"></div>", single(), None, path()),
            ("<div class={foo}></div>", dynamic_quoted(), None, path()),
            ("<div class=\"{foo}\"></div>", None, None, path()),
            // Unquoted but containing a double quote, so single quotes win
            // over the preferred double. Reachable only through the
            // recovering parser, as above.
            ("<div title=a\"b></div>", None, None, path()),
            // The expression contains a space, so it cannot sit unquoted.
            ("<div class={a + b}></div>", avoid_invalid(), None, path()),
            // Directives are checked too.
            ("<input bind:value=\"{value}\" />", None, None, path()),
        ];

        Tester::new(HtmlQuotes::NAME, HtmlQuotes::PLUGIN, pass, fail).test_and_snapshot();
    }
}
