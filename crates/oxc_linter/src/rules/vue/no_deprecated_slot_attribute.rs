use lazy_regex::Regex;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::{Attribute, Element, Node};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{deserialize_to_regexp_group_vec, directive_key_span, vue_casing},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn slot_attribute_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`slot` attributes are deprecated.")
        .with_help("Vue 3 removed the `slot`/`:slot` attributes; use `v-slot` instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoDeprecatedSlotAttributeConfig {
    /// Tag-name patterns; a `slot`/`:slot` attribute on a matching element is
    /// not reported. Each entry is matched (case-sensitively) against the
    /// element's raw name, its PascalCase form, and its kebab-case form —
    /// either as a bare tag name (an exact, anchored match) or a
    /// `"/pattern/flags"` regex literal. Default: none.
    #[serde(deserialize_with = "deserialize_to_regexp_group_vec")]
    ignore: Vec<Regex>,
    /// Like `ignore`, but matched against the raw name of the element's
    /// *parent* instead (no PascalCase/kebab-case variants). Default: none.
    #[serde(deserialize_with = "deserialize_to_regexp_group_vec")]
    ignore_parents: Vec<Regex>,
}

// Boxed (like `vue/no-v-html`'s `NoVHtml`): two `Vec<Regex>` fields would
// make this the largest `RuleEnum` variant unboxed; `Box` keeps this rule's
// own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct NoDeprecatedSlotAttribute(Box<NoDeprecatedSlotAttributeConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `slot`/`:slot` attribute (Vue 2.6+ deprecated
    /// it in favor of `v-slot`).
    ///
    /// `ignore` exempts elements by tag name (raw, PascalCase, or
    /// kebab-case); `ignoreParents` exempts elements by their parent
    /// element's raw tag name.
    ///
    /// ### Why is this bad?
    ///
    /// `slot`/`:slot` have no effect in Vue 3; a template relying on them
    /// silently loses the named-slot content it used to project.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <LinkList>
    ///     <template slot="name"></template>
    ///   </LinkList>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <LinkList>
    ///     <template v-slot:name></template>
    ///   </LinkList>
    /// </template>
    /// ```
    NoDeprecatedSlotAttribute,
    vue,
    correctness,
    config = NoDeprecatedSlotAttribute,
    version = "1.77.0",
    short_description = "Disallow deprecated `slot` attribute.",
);

impl Rule for NoDeprecatedSlotAttribute {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoDeprecatedSlotAttribute {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        // Seeded with `"template"`, not `None`: `nodes` are the *children*
        // of the SFC's own `<template>` block, and vue-eslint-parser models
        // that outer `<template>` tag itself as a real `VElement` (raw name
        // `"template"`) that is the `.parent` of every root-level node — so
        // a `slot`/`:slot` attribute at the very top of a template block
        // does have an (`ignoreParents`-matchable) parent. Verified
        // empirically against real eslint-plugin-vue: `{ ignoreParents:
        // ["template"] }` suppresses a root-level `<a slot="name" />`.
        self.walk(nodes, Some("template"), ctx);
    }
}

impl NoDeprecatedSlotAttribute {
    /// Unlike [`crate::utils::walk_elements`], this tracks the immediate
    /// parent *element*'s raw name — needed for the `ignoreParents` option,
    /// which eslint-plugin-vue matches against `component.parent.rawName`
    /// (only when that parent `utils.isVElement`).
    fn walk<'a>(
        &self,
        nodes: &[Node<'a>],
        parent_name: Option<&'a str>,
        ctx: &mut VueTemplateContext<'a>,
    ) {
        for node in nodes {
            if let Node::Element(element) = node {
                self.check_element(element, parent_name, ctx);
                self.walk(&element.children, Some(element.name), ctx);
            }
        }
    }

    fn check_element<'a>(
        &self,
        element: &Element<'a>,
        parent_name: Option<&'a str>,
        ctx: &mut VueTemplateContext<'a>,
    ) {
        for attribute in &element.attributes {
            let Some(form) = slot_attribute_form(attribute) else { continue };
            // eslint-plugin-vue's `slot-attribute.ts` has two independent
            // report functions: `reportSlot` (the plain `slot="x"` path)
            // consults `isAnyIgnored`/`isParentIgnored`; `reportVBindSlot`
            // (the `:slot`/`v-bind:slot` path) does not check either option
            // at all. Verified against real eslint-plugin-vue: with
            // `{ ignore: ["one"] }`, `<one slot="one">` is suppressed but
            // `<one :slot="one">` still fires.
            if form == SlotAttributeForm::Plain && self.is_ignored(element.name, parent_name) {
                continue;
            }
            ctx.diagnostic(slot_attribute_diagnostic(directive_key_span(attribute)));
        }
    }

    /// eslint-plugin-vue's `isAnyIgnored(componentName, pascalCase(...),
    /// kebabCase(...))` for `ignore`, then (only once that passes)
    /// `isParentIgnored(parentName)` for `ignoreParents`.
    fn is_ignored(&self, element_name: &str, parent_name: Option<&str>) -> bool {
        let pascal = vue_casing::pascal_case(element_name);
        let kebab = vue_casing::kebab_case(element_name);
        let candidates = [element_name, pascal.as_str(), kebab.as_str()];
        if self.0.ignore.iter().any(|pattern| candidates.iter().any(|name| pattern.is_match(name)))
        {
            return true;
        }
        if let Some(parent_name) = parent_name
            && self.0.ignore_parents.iter().any(|pattern| pattern.is_match(parent_name))
        {
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotAttributeForm {
    /// Plain, Vue 2 style: `slot="x"`.
    Plain,
    /// `v-bind:slot`/`:slot`, with a static argument.
    Bind,
}

/// eslint-plugin-vue's two `slot-attribute.ts` selectors:
/// `VAttribute[directive=false][key.name='slot']` (plain, Vue 2 style) and
/// `VAttribute[directive=true][key.name.name='bind'][key.argument.name='slot']`
/// (`v-bind:slot`/`:slot`, with a *static* argument — a dynamic `:[slot]`
/// never matches a fixed argument name).
fn slot_attribute_form(attribute: &Attribute<'_>) -> Option<SlotAttributeForm> {
    match &attribute.directive {
        None if attribute.name.eq_ignore_ascii_case("slot") => Some(SlotAttributeForm::Plain),
        Some(directive)
            if directive.name == "bind"
                && directive
                    .argument
                    .as_ref()
                    .is_some_and(|argument| !argument.dynamic && argument.text == "slot") =>
        {
            Some(SlotAttributeForm::Bind)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoDeprecatedSlotAttribute;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><LinkList><template v-slot:name><a /></template></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><LinkList><template #name><a /></template></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList v-slot="{a}"><a /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><LinkList><a /></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignore`: bare strings are exact matches against raw,
            // PascalCase, and kebab-case forms of the element name.
            (
                r#"<template><LinkList><one slot="one" /><my-component slot="x" /></LinkList></template>"#,
                Some(json!([{ "ignore": ["one", "my-component"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignore`: regex-literal form, matched against any of the three
            // name variants.
            (
                r#"<template><LinkList><MyComponent slot="x" /></LinkList></template>"#,
                Some(json!([{ "ignore": ["/^my-/i"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreParents`: matched against the raw parent tag name only.
            (
                r#"<template><LinkList><one slot="one" /></LinkList></template>"#,
                Some(json!([{ "ignoreParents": ["LinkList"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><one slot="one" /></LinkList></template>"#,
                Some(json!([{ "ignoreParents": ["/^Link/"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreParents`: the outer SFC `<template>` block is itself
            // modeled as a `template`-named parent, so it matches a
            // root-level slot attribute too.
            (
                r#"<template><a slot="name" /></template>"#,
                Some(json!([{ "ignoreParents": ["template"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r"<template><LinkList><template slot><a /></template></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><template slot="name"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-bind:slot` longhand.
            (
                r#"<template><LinkList><template v-bind:slot="name"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `:slot` shorthand.
            (
                r#"<template><LinkList><template :slot="slot.name"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Fires on a plain (non-component) element too.
            (
                r#"<template><LinkList><a :slot="name" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignore` configured, but this element doesn't match it.
            (
                r#"<template><LinkList><two slot="two" /></LinkList></template>"#,
                Some(json!([{ "ignore": ["one"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignore`/`ignoreParents` only apply to the plain `slot="x"`
            // form: eslint-plugin-vue's `reportVBindSlot` (the `:slot`/
            // `v-bind:slot` path) never consults either option, so `:slot`
            // still fires even when the element/parent would otherwise be
            // ignored. Verified against real eslint-plugin-vue.
            (
                r#"<template><LinkList><one :slot="one" /></LinkList></template>"#,
                Some(json!([{ "ignore": ["one"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><one v-bind:slot="one" /></LinkList></template>"#,
                Some(json!([{ "ignoreParents": ["LinkList"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreParents` configured, but this parent doesn't match it.
            (
                r#"<template><OtherList><one slot="one" /></OtherList></template>"#,
                Some(json!([{ "ignoreParents": ["LinkList"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoDeprecatedSlotAttribute::NAME, NoDeprecatedSlotAttribute::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
