use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{is_custom_component, vue_casing, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn disallow_diagnostic(directive_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Using v-{directive_name} on component may break component's content."
    ))
    .with_help(
        "Custom components render their own content; `v-text`/`v-html` on them discards or clashes with it. Use a prop or a slot instead.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoVTextVHtmlOnComponentConfig {
    /// Component names allowed to carry `v-text`/`v-html` despite being
    /// custom components. Matched against the element's name as written, its
    /// `PascalCase` form, and its `kebab-case` form (mirrors
    /// eslint-plugin-vue's `allow` option). Default: none.
    allow: Vec<String>,
}

// Boxed (like `vue/valid-v-on`'s `ValidVOn`): `allow: Vec<String>` is 24
// bytes unboxed, which would make this the largest `RuleEnum` variant and
// grow every rule's in-memory representation from 16 to 24 bytes. `Box`
// keeps this rule's own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct NoVTextVHtmlOnComponent(Box<NoVTextVHtmlOnComponentConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `v-text`/`v-html` on a custom component in Vue `<template>`
    /// blocks.
    ///
    /// ### Why is this bad?
    ///
    /// `v-text`/`v-html` overwrite an element's rendered content directly.
    /// On a custom component, that content is normally produced by the
    /// component's own template (and any slots passed to it); forcing it
    /// via `v-text`/`v-html` from the outside may silently discard the
    /// component's own markup or conflict with it.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent v-text="text" />
    ///   <MyComponent v-html="html" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-text="text" />
    ///   <div v-html="html" />
    /// </template>
    /// ```
    NoVTextVHtmlOnComponent,
    vue,
    correctness,
    config = NoVTextVHtmlOnComponent,
    version = "1.77.0",
    short_description = "Disallow `v-text` / `v-html` on component.",
);

impl Rule for NoVTextVHtmlOnComponent {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoVTextVHtmlOnComponent {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if !is_custom_component(element) || self.is_allowed(element.name) {
                return;
            }
            // eslint-plugin-vue visits `v-text` and `v-html` independently
            // (two separate selectors sharing one handler), so an element
            // carrying both gets two reports.
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name == "text" || directive.name == "html" {
                    ctx.diagnostic(disallow_diagnostic(directive.name, attribute.span));
                }
            }
        });
    }
}

impl NoVTextVHtmlOnComponent {
    /// eslint-plugin-vue's `isAllowedComponent`: the element's raw name, its
    /// `PascalCase` form, or its `kebab-case` form must be in the `allow`
    /// list.
    fn is_allowed(&self, raw_name: &str) -> bool {
        if self.0.allow.is_empty() {
            return false;
        }
        let pascal = vue_casing::pascal_case(raw_name);
        let kebab = vue_casing::kebab_case(raw_name);
        self.0.allow.iter().any(|name| name == raw_name || *name == pascal || *name == kebab)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoVTextVHtmlOnComponent;
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
            (
                r#"<template><div v-html="html" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Native SVG element: not a custom component either.
            (
                r#"<template><svg v-html="html" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Allowed via the `allow` option, matched by its kebab-case form.
            (
                r#"<template><MyComp v-text="text" /></template>"#,
                Some(json!([{ "allow": ["my-comp"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Allowed, matched by its exact raw name.
            (
                r#"<template><MyComp v-html="html" /></template>"#,
                Some(json!([{ "allow": ["MyComp"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><MyComp v-text="text" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComp v-html="html" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Both directives on the same component: two reports.
            (
                r#"<template><MyComp v-text="a" v-html="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `allow` doesn't cover this component name.
            (
                r#"<template><MyComp v-text="text" /></template>"#,
                Some(json!([{ "allow": ["OtherComp"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Kebab-cased custom element tag.
            (
                r#"<template><my-comp v-html="html" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoVTextVHtmlOnComponent::NAME, NoVTextVHtmlOnComponent::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
