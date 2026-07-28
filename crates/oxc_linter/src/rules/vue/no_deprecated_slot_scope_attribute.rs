use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn slot_scope_attribute_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`slot-scope` are deprecated.")
        .with_help("Vue 3 removed `slot-scope`; use `v-slot` instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedSlotScopeAttribute;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `slot-scope` attribute (Vue 2.6+ deprecated
    /// it, and Vue 3 removed it, in favor of `v-slot`).
    ///
    /// ### Why is this bad?
    ///
    /// `slot-scope` has no effect in Vue 3; a template relying on it
    /// silently loses its scoped-slot data.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <template slot-scope="props"></template>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <template v-slot="props"></template>
    /// </template>
    /// ```
    NoDeprecatedSlotScopeAttribute,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow deprecated `slot-scope` attribute.",
);

impl Rule for NoDeprecatedSlotScopeAttribute {}

impl VueTemplateRule for NoDeprecatedSlotScopeAttribute {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue's `slot-scope-attribute` syntax module
            // matches `VAttribute[directive=true][key.name.name='slot-scope']`:
            // vue-eslint-parser recognizes the bare (no `v-` prefix)
            // deprecated `slot-scope` attribute as a directive despite the
            // missing prefix. This fork's parser does not special-case it,
            // so it surfaces as a plain attribute instead — same approach as
            // `no_lone_template.rs`/`no_useless_template_attributes.rs`.
            // Unlike bare `scope` (see `no_deprecated_scope_attribute.rs`),
            // this conversion applies on *any* element, not just
            // `<template>`. The attribute-name comparison is
            // case-SENSITIVE, though (vue-eslint-parser's SFC `getTagName`
            // returns the raw, as-written attribute name when deciding
            // whether to convert it to a directive) — verified empirically
            // against real eslint-plugin-vue: `SLOT-SCOPE="x"` never fires.
            for attribute in &element.attributes {
                if attribute.directive.is_none() && attribute.name == "slot-scope" {
                    ctx.diagnostic(slot_scope_attribute_diagnostic(attribute.name_span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedSlotScopeAttribute;
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
                r#"<template><LinkList><a slot="name" /></LinkList></template>"#,
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
            // Case-sensitive attribute-name match (unlike HTML attributes
            // generally): verified against real eslint-plugin-vue that
            // `SLOT-SCOPE="x"` never converts to the deprecated directive
            // form, so this rule doesn't fire on it.
            (
                r#"<template><LinkList><a SLOT-SCOPE="{a}" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><LinkList><template slot-scope="{a}"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No value.
            (
                r"<template><LinkList><template slot-scope><a /></template></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Not restricted to `<template>` — the deprecated selector
            // matches any element.
            (
                r#"<template><LinkList><a slot-scope="{a}" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Alongside a `slot`/`:slot` attribute: only `slot-scope` is
            // this rule's concern.
            (
                r#"<template><LinkList><template slot-scope="{a}" slot="foo"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><template slot-scope="{a}" :slot="arg"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedSlotScopeAttribute::NAME,
            NoDeprecatedSlotScopeAttribute::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
