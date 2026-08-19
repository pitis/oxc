use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{
    AttributeKind, AttributeValue, DirectiveKind, Element, Node, ValuePart,
};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{AlwaysNever, get_plain_attribute, walk_svelte_elements},
};

fn no_target_blank_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Using target=\"_blank\" without rel=\"noopener noreferrer\" is a security risk.",
    )
    .with_help(
        "Add `rel=\"noopener noreferrer\"` so the opened page cannot reach back through `window.opener` or learn the referrer.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoTargetBlank {
    /// Whether `rel="noopener"` alone (without `noreferrer`) is enough.
    allow_referrer: bool,
    /// Whether links with a dynamic `href` are checked. `"always"` (the
    /// default) checks them; `"never"` only checks static external links.
    enforce_dynamic_links: AlwaysNever,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `target="_blank"` on links without `rel="noopener noreferrer"`,
    /// for external and dynamic `href`s.
    ///
    /// ### Why is this bad?
    ///
    /// A page opened via `target="_blank"` keeps a `window.opener` handle to
    /// the opening page (in older browsers) and receives its referrer. A
    /// malicious destination can use that handle to redirect the original
    /// tab (reverse tabnabbing) or mine the referrer. `rel="noopener"` cuts
    /// the handle and `rel="noreferrer"` also hides the referrer.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <a href="https://example.com" target="_blank">external</a>
    /// <a href={link} target="_blank">dynamic</a>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <a href="https://example.com" target="_blank" rel="noopener noreferrer">external</a>
    /// <a href="/local" target="_blank">relative links are not external</a>
    /// ```
    ///
    /// ### Options
    ///
    /// This rule takes an object with two properties:
    ///
    /// - `allowReferrer` (default `false`): when `true`, `rel="noopener"`
    ///   alone is accepted and `noreferrer` is not required.
    /// - `enforceDynamicLinks` (`"always"` | `"never"`, default `"always"`):
    ///   when `"never"`, links whose `href` is dynamic (`href={...}`,
    ///   `{href}`, `bind:href`) are not checked; static external links are
    ///   always checked.
    ///
    /// ```json
    /// {
    ///   "svelte/no-target-blank": ["error", { "allowReferrer": true, "enforceDynamicLinks": "never" }]
    /// }
    /// ```
    NoTargetBlank,
    svelte,
    restriction,
    config = NoTargetBlank,
    version = "1.80.0",
    short_description = "Disallow `target=\"_blank\"` without `rel=\"noopener noreferrer\"`.",
);

impl Rule for NoTargetBlank {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for NoTargetBlank {
    // Ports eslint-plugin-svelte's `no-target-blank`.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let enforce_dynamic_links = self.enforce_dynamic_links == AlwaysNever::Always;
        let mut spans = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let AttributeKind::Plain { name, value, .. } = &attribute.kind else {
                    continue;
                };
                // Upstream only checks fully static `target` values; a
                // dynamic `target={...}` is never `"_blank"` statically.
                if *name != "target"
                    || value.as_ref().and_then(AttributeValue::as_static_text) != Some("_blank")
                {
                    continue;
                }
                if has_secure_rel(element, self.allow_referrer) {
                    continue;
                }
                if has_external_link(element)
                    || (enforce_dynamic_links && has_dynamic_link(element))
                {
                    spans.push(attribute.span);
                }
            }
        });
        for span in spans {
            ctx.diagnostic(no_target_blank_diagnostic(span));
        }
    }
}

/// Whether the element's (first) `rel` attribute contains `noopener` and —
/// unless `allow_referrer` — `noreferrer`. Like upstream, only static text
/// parts contribute tokens, split on single spaces, case-insensitively.
fn has_secure_rel(element: &Element<'_>, allow_referrer: bool) -> bool {
    let Some((_, value)) = get_plain_attribute(element, "rel") else {
        return false;
    };
    let mut has_noopener = false;
    let mut has_noreferrer = false;
    if let Some(value) = value {
        for part in &value.parts {
            if let ValuePart::Text(text) = part {
                for token in text.value.split(' ') {
                    has_noopener |= token.eq_ignore_ascii_case("noopener");
                    has_noreferrer |= token.eq_ignore_ascii_case("noreferrer");
                }
            }
        }
    }
    has_noopener && (allow_referrer || has_noreferrer)
}

/// Whether any `href` attribute's value starts with static external-looking
/// text: a scheme (`\w+:`) or a protocol-relative `//`. Relative links are
/// same-origin and carry no tabnabbing risk.
fn has_external_link(element: &Element<'_>) -> bool {
    element.attributes.iter().any(|attribute| {
        let AttributeKind::Plain { name, value: Some(value), .. } = &attribute.kind else {
            return false;
        };
        *name == "href"
            && matches!(value.parts.first(), Some(ValuePart::Text(text)) if is_external_url(text.value))
    })
}

/// Mirror of upstream's `/^(?:\w+:|\/\/)/` test.
fn is_external_url(url: &str) -> bool {
    if url.starts_with("//") {
        return true;
    }
    match url.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        Some(colon_index) => colon_index > 0 && url.as_bytes()[colon_index] == b':',
        None => false,
    }
}

/// Whether the element's `href` is dynamic: a plain `href` with any `{expr}`
/// part, the `{href}` shorthand, or a `bind:href` directive. Like upstream,
/// when a plain `href` exists only its own parts decide.
fn has_dynamic_link(element: &Element<'_>) -> bool {
    if let Some((_, value)) = get_plain_attribute(element, "href") {
        return value.is_some_and(|value| {
            value.parts.iter().any(|part| matches!(part, ValuePart::Expression(_)))
        });
    }
    element.attributes.iter().any(|attribute| match &attribute.kind {
        AttributeKind::Shorthand { name, .. } => *name == "href",
        AttributeKind::Directive(directive) => {
            directive.kind == DirectiveKind::Bind && directive.name == "href"
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoTargetBlank;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<a href="https://example.com" target="_blank" rel="noopener noreferrer">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // rel token matching is case-insensitive, like upstream.
            (
                r#"<a href="https://example.com" target="_blank" rel="NOOPENER NOREFERRER">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // No target at all.
            (
                r#"<a href="https://example.com">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Only external (scheme or `//`) static links are checked.
            (
                r#"<a href="/relative" target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<a href="relative/path" target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Dynamic target is never statically `_blank`.
            (
                r#"<a href="https://example.com" target={target}>link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // allowReferrer accepts `noopener` alone.
            (
                r#"<a href="https://example.com" target="_blank" rel="noopener">link</a>"#,
                Some(serde_json::json!([{ "allowReferrer": true }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // enforceDynamicLinks: "never" skips dynamic hrefs.
            (
                r#"<a href={url} target="_blank">link</a>"#,
                Some(serde_json::json!([{ "enforceDynamicLinks": "never" }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<a bind:href={url} target="_blank">link</a>"#,
                Some(serde_json::json!([{ "enforceDynamicLinks": "never" }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            (
                r#"<a href="https://example.com" target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Protocol-relative links are external too.
            (
                r#"<a href="//example.com" target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Dynamic hrefs are checked by default.
            (
                r#"<a href={url} target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<a {href} target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<a bind:href={url} target="_blank">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `noopener` alone is not enough by default.
            (
                r#"<a href="https://example.com" target="_blank" rel="noopener">link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A fully dynamic rel contributes no static tokens.
            (
                r#"<a href="https://example.com" target="_blank" rel={rel}>link</a>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // allowReferrer still requires `noopener`.
            (
                r#"<a href="https://example.com" target="_blank">link</a>"#,
                Some(serde_json::json!([{ "allowReferrer": true }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // enforceDynamicLinks: "never" still checks static external links.
            (
                r#"<a href="https://example.com" target="_blank">link</a>"#,
                Some(serde_json::json!([{ "enforceDynamicLinks": "never" }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside blocks.
            (
                r#"{#if a}<a href="https://example.com" target="_blank">link</a>{/if}"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoTargetBlank::NAME, NoTargetBlank::PLUGIN, pass, fail).test_and_snapshot();
    }
}
