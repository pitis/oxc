use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{get_attribute, get_directive, vue_casing, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn router_link_tag_prop_diagnostic(element: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "'tag' property on '{element}' component is deprecated. Use scoped slots instead."
    ))
    .with_help(
        "Vue Router 4 dropped `RouterLink`'s `tag` prop; render the desired element yourself \
         with `RouterLink`'s `v-slot`/custom scoped-slot API instead.",
    )
    .with_label(span)
}

fn default_components() -> Vec<String> {
    vec!["RouterLink".to_string()]
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoDeprecatedRouterLinkTagPropConfig {
    /// Component names this rule checks the `tag` prop on, in place of the
    /// default `["RouterLink"]`. Each entry is matched (case-sensitively)
    /// against a tag's exact kebab-case and PascalCase forms.
    #[serde(default = "default_components")]
    components: Vec<String>,
}

impl Default for NoDeprecatedRouterLinkTagPropConfig {
    fn default() -> Self {
        Self { components: default_components() }
    }
}

// Boxed (like `vue/no-deprecated-slot-attribute`'s
// `NoDeprecatedSlotAttribute`): a `Vec<String>` field would otherwise make
// this the largest `RuleEnum` variant; `Box` keeps this rule's own footprint
// at one pointer (8 bytes).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct NoDeprecatedRouterLinkTagProp(Box<NoDeprecatedRouterLinkTagPropConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `tag` prop (plain or bound) on `<RouterLink>`
    /// (Vue Router 4+ removed it). The `components` option replaces —
    /// rather than extends — the default `["RouterLink"]` component name
    /// list, e.g. to also (or instead) cover a custom `NuxtLink`.
    ///
    /// ### Why is this bad?
    ///
    /// `RouterLink`'s `tag` prop told Vue Router 3 to render as a different
    /// element (e.g. `tag="span"` instead of the default `<a>`). Vue Router 4
    /// dropped it; a `<RouterLink tag="...">` that relied on it silently
    /// keeps rendering an `<a>` instead.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <router-link to="/" tag="span">Home</router-link>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <router-link to="/" v-slot="{ navigate }">
    ///     <span @click="navigate">Home</span>
    ///   </router-link>
    /// </template>
    /// ```
    NoDeprecatedRouterLinkTagProp,
    vue,
    correctness,
    config = NoDeprecatedRouterLinkTagProp,
    version = "1.77.0",
    short_description = "Disallow deprecated `tag` property on `RouterLink`.",
);

impl Rule for NoDeprecatedRouterLinkTagProp {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoDeprecatedRouterLinkTagProp {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if !self.matches_component(element.name) {
                return;
            }

            // Plain `tag="..."` takes precedence (matches upstream: it only
            // falls back to the bound form when `getAttribute` finds
            // nothing). Report span is just the key text — `"tag"` itself
            // for the plain form, or the directive argument's own span
            // (excluding the `:`/`v-bind:` prefix) for the bound form —
            // mirroring eslint-plugin-vue reporting on `tagKey`
            // (`tagAttr.key` / `directive.key.argument`), never the whole
            // attribute node. Verified against real eslint-plugin-vue.
            if let Some(attribute) = get_attribute(element, "tag") {
                ctx.diagnostic(router_link_tag_prop_diagnostic(element.name, attribute.name_span));
                return;
            }
            if let Some(attribute) = get_directive(element, "bind", Some("tag")) {
                let directive = attribute.directive.as_ref().expect("matched by get_directive");
                let argument = directive.argument.as_ref().expect("matched by get_directive");
                ctx.diagnostic(router_link_tag_prop_diagnostic(element.name, argument.span));
            }
        });
    }
}

impl NoDeprecatedRouterLinkTagProp {
    /// eslint-plugin-vue's `getComponentNames`: each configured component
    /// name expands to its kebab-case and PascalCase forms, matched
    /// case-sensitively and exactly against the element's raw tag name (no
    /// `is`-attribute component detection — a plain `<div>` named literally
    /// `router-link` still matches).
    fn matches_component(&self, element_name: &str) -> bool {
        self.0.components.iter().any(|component| {
            element_name == vue_casing::kebab_case(component)
                || element_name == vue_casing::pascal_case(component)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoDeprecatedRouterLinkTagProp;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><router-link to="/">Home</router-link></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Not a configured component name.
            (
                r#"<template><nuxt-link tag="a"></nuxt-link></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Not a configured component name (custom, unrelated tag).
            (
                r#"<template><other-link tag="a"></other-link></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A dynamic argument named `tag` (via a variable) never matches
            // a static `tag` argument, dynamic or not.
            (
                r#"<template><router-link v-bind:[tag]="a"></router-link></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `components` fully replaces the default list — `RouterLink`
            // is no longer checked once configured away.
            (
                r#"<template><router-link tag="a"></router-link></template>"#,
                Some(json!([{ "components": ["NuxtLink"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Kebab-case default component name, plain `tag`.
            (
                r#"<template><router-link tag="a"></router-link></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // PascalCase default component name, plain `tag`.
            (
                r#"<template><RouterLink tag="a"></RouterLink></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shorthand bound `:tag`.
            (
                r#"<template><router-link :tag="a"></router-link></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Longhand `v-bind:tag`.
            (
                r#"<template><RouterLink v-bind:tag="a"></RouterLink></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `components` option: kebab-case and PascalCase forms of a
            // custom name both match.
            (
                r#"<template><nuxt-link tag="a"></nuxt-link></template>"#,
                Some(json!([{ "components": ["NuxtLink"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><NuxtLink tag="a"></NuxtLink></template>"#,
                Some(json!([{ "components": ["NuxtLink"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedRouterLinkTagProp::NAME,
            NoDeprecatedRouterLinkTagProp::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
