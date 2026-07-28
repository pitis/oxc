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
        self.walk(nodes, None, ctx);
    }
}

impl NoDeprecatedSlotAttribute {
    /// Unlike [`crate::utils::walk_elements`], this tracks the immediate
    /// parent *element*'s raw name (`None` at the top of the `<template>`
    /// block) — needed for the `ignoreParents` option, which
    /// eslint-plugin-vue matches against `component.parent.rawName` (only
    /// when that parent `utils.isVElement`).
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
            if !is_slot_attribute(attribute) {
                continue;
            }
            if self.is_ignored(element.name, parent_name) {
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

/// eslint-plugin-vue's two `slot-attribute.ts` selectors:
/// `VAttribute[directive=false][key.name='slot']` (plain, Vue 2 style) and
/// `VAttribute[directive=true][key.name.name='bind'][key.argument.name='slot']`
/// (`v-bind:slot`/`:slot`, with a *static* argument — a dynamic `:[slot]`
/// never matches a fixed argument name).
fn is_slot_attribute(attribute: &Attribute<'_>) -> bool {
    match &attribute.directive {
        None => attribute.name.eq_ignore_ascii_case("slot"),
        Some(directive) => {
            directive.name == "bind"
                && directive
                    .argument
                    .as_ref()
                    .is_some_and(|argument| !argument.dynamic && argument.text == "slot")
        }
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
