use lazy_regex::Regex;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{Rule, TupleRuleConfig},
    utils::{
        AlwaysNever, deserialize_to_regexp_group_vec, is_custom_component, vue_casing,
        walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn must_be_hyphenated_diagnostic(text: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("v-on event '{text}' must be hyphenated.")).with_label(span)
}

fn cannot_be_hyphenated_diagnostic(text: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("v-on event '{text}' can't be hyphenated.")).with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct VOnEventHyphenationOptions {
    /// Event names (matched as a substring) that are never reported.
    ignore: Vec<String>,
    /// Tag-name patterns; a custom element whose raw tag name matches any of
    /// them is skipped entirely. Each entry is either a bare tag name
    /// (matched as an exact, case-sensitive string — not a substring) or a
    /// `"/pattern/flags"` regex literal, mirroring eslint-plugin-vue's
    /// `toRegExpGroupMatcher`.
    #[serde(deserialize_with = "deserialize_to_regexp_group_vec")]
    ignore_tags: Vec<Regex>,
    /// Accepted for compatibility with upstream's `autofix` option key; this
    /// rule never applies fixes (oxlint doesn't fix Vue template rules), so
    /// the value itself has no effect. Present only so a real-world
    /// `v-on-event-hyphenation` config (which upstream defaults to
    /// `{ autofix: true }` for Vue 3) still deserializes without error.
    #[serde(rename = "autofix")]
    _autofix: bool,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VOnEventHyphenationConfig(AlwaysNever, VOnEventHyphenationOptions);

// Boxed (like `vue/no-v-html`'s `NoVHtml`): the `Vec<String>`/`Vec<Regex>`
// options make this the largest `RuleEnum` variant unboxed; `Box` keeps this
// rule's own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct VOnEventHyphenation(Box<VOnEventHyphenationConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces `v-on` event naming style (kebab-case vs. camelCase) for
    /// static event names bound on custom components. Default `"always"`
    /// requires hyphenation; `"never"` requires camelCase instead.
    ///
    /// ### Why is this bad?
    ///
    /// Consistent event-name casing keeps the emitting component's
    /// `emits`/`$emit` call and the listening `v-on`/`@` binding easy to
    /// grep for and match up.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (default `"always"`):
    /// ```vue
    /// <template>
    ///   <MyComponent @myEvent="handler" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule (default `"always"`):
    /// ```vue
    /// <template>
    ///   <MyComponent @my-event="handler" />
    /// </template>
    /// ```
    VOnEventHyphenation,
    vue,
    style,
    config = VOnEventHyphenationConfig,
    version = "1.77.0",
    short_description = "Enforce v-on event naming style on custom components in template.",
);

impl Rule for VOnEventHyphenation {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<TupleRuleConfig<Self>>(value).map(TupleRuleConfig::into_inner)
    }
}

impl VueTemplateRule for VOnEventHyphenation {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let VOnEventHyphenationConfig(style, options) = &*self.0;
        let use_hyphenated = *style != AlwaysNever::Never;
        walk_elements(nodes, &mut |element| {
            if !is_custom_component(element)
                || options.ignore_tags.iter().any(|pattern| pattern.is_match(element.name))
            {
                return;
            }
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "on" {
                    continue;
                }
                let Some(argument) = &directive.argument else { continue };
                if argument.dynamic {
                    continue;
                }
                let name = argument.text;
                if name.is_empty() || is_ignored_attribute(name, &options.ignore, use_hyphenated) {
                    continue;
                }
                // Upstream reports `node: node.key` but with an explicit
                // `loc: node.loc` — the *whole* `VAttribute`'s location —
                // and eslint's `loc` wins over `node`. So the label covers
                // the attribute including its `="value"`, not just the key.
                let diagnostic = if use_hyphenated {
                    must_be_hyphenated_diagnostic(attribute.name, attribute.span)
                } else {
                    cannot_be_hyphenated_diagnostic(attribute.name, attribute.span)
                };
                ctx.diagnostic(diagnostic);
            }
        });
    }
}

/// eslint-plugin-vue `v-on-event-hyphenation`'s `isIgnoredAttribute`: matched
/// (skipped) when the name contains one of the user-supplied ignored
/// substrings, or when it is already compatible with the requested casing
/// (no uppercase letter when hyphenation is required; no hyphen when
/// camelCase is required).
fn is_ignored_attribute(name: &str, ignore: &[String], use_hyphenated: bool) -> bool {
    if ignore.iter().any(|attr| name.contains(attr.as_str())) {
        return true;
    }
    if use_hyphenated { !vue_casing::has_upper(name) } else { !name.contains('-') }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::VOnEventHyphenation;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Native elements are not checked.
            (
                r#"<template><div @myEvent="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default ("always"): already-hyphenated names pass.
            (
                r#"<template><MyComponent @my-event="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Longhand `v-on:`.
            (
                r#"<template><MyComponent v-on:my-event="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Single-word event names have nothing to hyphenate.
            (
                r#"<template><MyComponent @close="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-on="handlers"` object binding (no argument) is not checked.
            (
                r#"<template><MyComponent v-on="handlers" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic argument is never checked.
            (
                r#"<template><MyComponent @[myEvent]="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "never": camelCase passes.
            (
                r#"<template><MyComponent @myEvent="a" /></template>"#,
                Some(json!(["never"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignore` option.
            (
                r#"<template><MyComponent @myEvent="a" /></template>"#,
                Some(json!(["always", { "ignore": ["myEvent"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreTags` option: bare string is an exact (anchored)
            // match, not a substring search.
            (
                r#"<template><IgnoredTag @myEvent="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["IgnoredTag"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreTags` option: `"/pattern/flags"` regex-literal form.
            (
                r#"<template><IgnoredTag @myEvent="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["/^Ignored/"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `autofix` config key deserializes without error even though
            // this rule never applies fixes.
            (
                r#"<template><MyComponent @my-event="a" /></template>"#,
                Some(json!(["always", { "autofix": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Default ("always"): camelCase event name on a component.
            (
                r#"<template><MyComponent @myEvent="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Longhand `v-on:` form.
            (
                r#"<template><MyComponent v-on:myEvent="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "never": hyphenated name reported.
            (
                r#"<template><MyComponent @my-event="a" /></template>"#,
                Some(json!(["never"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreTags` bare string is an exact match, not a substring
            // search: a superstring tag name is still checked.
            (
                r#"<template><IgnoredTagExtra @myEvent="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["IgnoredTag"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // ...and a tag name that merely *contains* the pattern as a
            // substring is still checked too.
            (
                r#"<template><MyIgnoredTag @myEvent="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["IgnoredTag"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(VOnEventHyphenation::NAME, VOnEventHyphenation::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
