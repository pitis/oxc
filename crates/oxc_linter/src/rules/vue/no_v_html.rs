use lazy_regex::Regex;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Attribute, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{deserialize_regex_option, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn no_v_html_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-html' directive can lead to XSS attack.")
        .with_help("Avoid `v-html`; sanitize untrusted content before rendering it, or render it as plain text instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoVHtmlConfig {
    /// A regex pattern; when the `v-html` value's expression text matches
    /// it, the report is suppressed (e.g. an identifier or member expression
    /// known to already hold sanitized HTML). Default: none (every `v-html`
    /// is reported).
    #[serde(default, deserialize_with = "deserialize_regex_option")]
    ignore_pattern: Option<Regex>,
}

// Boxed (like `vue/valid-v-on`'s `ValidVOn`): keeps this rule's own footprint
// at one pointer (8 bytes) so it doesn't grow `RuleEnum` past 16 bytes.
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct NoVHtml(Box<NoVHtmlConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows use of `v-html` in Vue `<template>` blocks. The
    /// `ignorePattern` option exempts `v-html` values whose expression text
    /// matches a regex (e.g. a variable already known to hold sanitized
    /// HTML).
    ///
    /// ### Why is this bad?
    ///
    /// Content passed to `v-html` is injected as raw HTML with no escaping.
    /// If it ever includes attacker-controlled data, that is a cross-site
    /// scripting (XSS) vector.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-html="userSuppliedHtml"></div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-text="userSuppliedText"></div>
    /// </template>
    /// ```
    NoVHtml,
    vue,
    restriction,
    config = NoVHtml,
    version = "1.77.0",
    short_description = "Disallow use of `v-html` to prevent XSS attack.",
);

impl Rule for NoVHtml {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoVHtml {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue visits every `VAttribute[directive=true][key.name.name='html']`
            // independently; an element can only sensibly carry one `v-html`,
            // but iterating attributes (rather than using `get_directive`,
            // which would only find the first) keeps that per-node reporting
            // granularity if it ever somehow did.
            for attribute in &element.attributes {
                if attribute.directive.as_ref().is_some_and(|directive| directive.name == "html")
                    && !self.should_ignore(attribute)
                {
                    ctx.diagnostic(no_v_html_diagnostic(attribute.span));
                }
            }
        });
    }
}

impl NoVHtml {
    /// eslint-plugin-vue's `shouldIgnore`: with an `ignorePattern` configured
    /// and a value present, suppress the report when the pattern matches the
    /// value's expression text. Upstream branches on `expression.type ===
    /// "Identifier"` (testing `expression.name`) vs. anything else (testing
    /// `sourceCode.getText(expression)`), but both resolve to the same raw
    /// source text for a directive value that's just an identifier, so this
    /// tests the (trimmed) raw text directly rather than reproducing the
    /// branch — verified against real eslint-plugin-vue for both an
    /// identifier (`v-html="trustedHtml"`) and a member expression
    /// (`v-html="trusted.value"`) value.
    fn should_ignore(&self, attribute: &Attribute<'_>) -> bool {
        let Some(pattern) = &self.0.ignore_pattern else { return false };
        let Some(value) = &attribute.value else { return false };
        pattern.is_match(value.text.trim())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoVHtml;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-text="text" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `<!-- eslint-disable … -->` HTML comment directives inside the
            // template. Every form below was cross-checked against real
            // eslint-plugin-vue 10.10.0 (`vue/comment-directive` + its
            // processor): each produces 0 diagnostics there too.
            (
                "<template>\n<!-- eslint-disable -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<template>\n<!-- eslint-disable vue/no-v-html -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Comma-separated rule list, plus a `-- description` suffix.
            (
                "<template>\n<!-- eslint-disable vue/no-lone-template, vue/no-v-html -- trusted -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<template>\n<!-- eslint-disable-next-line vue/no-v-html -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<template>\n<div v-html=\"html\" /> <!-- eslint-disable-line vue/no-v-html -->\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested inside an element, and covering several elements until
            // the matching `eslint-enable`.
            (
                "<template>\n<div>\n<!-- eslint-disable vue/no-v-html -->\n<p v-html=\"a\" />\n<p v-html=\"b\" />\n<!-- eslint-enable vue/no-v-html -->\n</div>\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Upstream quirk: a rule-less `eslint-enable` clears only the
            // "disable everything" state, never per-rule disables — so
            // `vue/no-v-html` stays suppressed past it.
            (
                "<template>\n<!-- eslint-disable vue/no-v-html -->\n<!-- eslint-enable -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Mixed rule list: one entry is a bare/typo'd name that never
            // matches anything, but `vue/no-v-html` in the same list still
            // suppresses.
            (
                "<template>\n<!-- eslint-disable typo/no-v-html, vue/no-v-html -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Directive matching is file-offset-based like the rest of this
            // pass: locks in that a `<template>` preceded by a `<script>`
            // block (so its content starts at a nonzero file offset) still
            // has its directive comments matched correctly.
            (
                "<script setup>\nconst html = trust();\n</script>\n\n<template>\n<!-- eslint-disable vue/no-v-html -->\n<div v-html=\"html\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignorePattern`: an identifier value matching the pattern.
            (
                r#"<template><div v-html="trustedHtml" /></template>"#,
                Some(json!([{ "ignorePattern": "^trusted" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignorePattern`: a member expression value matching the
            // pattern (verified against real eslint-plugin-vue that both
            // identifier and non-identifier values are matched against
            // their full raw text the same way).
            (
                r#"<template><div v-html="trusted.value" /></template>"#,
                Some(json!([{ "ignorePattern": "^trusted" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r#"<template><div v-html="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A disable naming a *different* rule doesn't suppress this one.
            (
                "<template>\n<!-- eslint-disable vue/no-lone-template -->\n<div v-html=\"rawHtml\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A different plugin's rule of the same name must NOT suppress
            // `vue/no-v-html` — matching upstream's exact `ruleId` string
            // equality (see `rule_name_matches`), rather than oxlint's
            // cross-plugin bare-name matching used for `<script>` comments.
            (
                "<template>\n<!-- eslint-disable typo/no-v-html -->\n<div v-html=\"rawHtml\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A bare (unprefixed) rule name must NOT suppress either:
            // upstream's reported `ruleId` for a plugin rule is always the
            // full `"vue/no-v-html"` string, so a `Map` lookup with a bare
            // key never hits.
            (
                "<template>\n<!-- eslint-disable no-v-html -->\n<div v-html=\"rawHtml\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `eslint-disable-next-line` only covers the next line.
            (
                "<template>\n<!-- eslint-disable-next-line vue/no-v-html -->\n<div v-html=\"ok\" />\n<div v-html=\"rawHtml\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A matching `eslint-enable` re-enables the rule.
            (
                "<template>\n<!-- eslint-disable vue/no-v-html -->\n<!-- eslint-enable vue/no-v-html -->\n<div v-html=\"rawHtml\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-html="rawHtml">child content ignored by the check</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Component targets are still reported by no-v-html (the
            // component-specific case is `no-v-text-v-html-on-component`'s
            // job, not this rule's).
            (
                r#"<template><MyComp v-html="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div v-html /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // `ignorePattern` configured, but this value doesn't match it.
            (
                r#"<template><div v-html="untrustedHtml" /></template>"#,
                Some(json!([{ "ignorePattern": "^trusted" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoVHtml::NAME, NoVHtml::PLUGIN, pass, fail).test_and_snapshot();
    }
}
