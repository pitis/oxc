use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn scope_attribute_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`scope` attributes are deprecated.")
        .with_help("Vue 2.5 replaced `scope` with `slot-scope`, and Vue 3 replaced both with `v-slot`; use `v-slot` instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedScopeAttribute;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `scope` attribute (Vue 2.5+ deprecated it in
    /// favor of `slot-scope`, itself removed in Vue 3 in favor of `v-slot`).
    ///
    /// ### Why is this bad?
    ///
    /// `scope` has no effect in Vue 3; a template relying on it silently
    /// loses its scoped-slot data.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <foo scope="props"></foo>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <foo v-slot="props"></foo>
    /// </template>
    /// ```
    NoDeprecatedScopeAttribute,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow deprecated `scope` attribute.",
);

impl Rule for NoDeprecatedScopeAttribute {}

impl VueTemplateRule for NoDeprecatedScopeAttribute {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue's `scope-attribute` syntax module matches
            // `VAttribute[directive=true] > VDirectiveKey[name.name='scope']`:
            // vue-eslint-parser recognizes the bare (no `v-` prefix)
            // deprecated `scope` attribute as a directive despite the
            // missing prefix. This fork's parser does not special-case it,
            // so it surfaces as a plain attribute instead — same approach as
            // `no_lone_template.rs`/`no_useless_template_attributes.rs`.
            for attribute in &element.attributes {
                if attribute.directive.is_none() && attribute.name.eq_ignore_ascii_case("scope") {
                    ctx.diagnostic(scope_attribute_diagnostic(attribute.name_span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedScopeAttribute;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><LinkList><a v-slot:name /></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><LinkList><a #name /></LinkList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><a v-slot="{a}" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><a #default="{a}" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Deprecated `slot`/`slot-scope` attributes are not this rule's
            // concern (that's `no-deprecated-slot-attribute`/
            // `no-deprecated-slot-scope-attribute`).
            (
                r#"<template><LinkList><a slot="name" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><LinkList><a slot-scope="{a}" /></LinkList></template>"#,
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
        ];

        let fail = vec![
            (
                r#"<template><LinkList><template scope="{a}"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `slot` attribute alongside `scope`: only `scope` is this
            // rule's concern.
            (
                r#"<template><LinkList><template slot="name" scope="{a}"><a /></template></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Not restricted to `<template>` — the deprecated selector
            // matches any element.
            (
                r#"<template><LinkList><a scope="{a}" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Case-insensitive attribute-name match, like HTML attributes
            // generally.
            (
                r#"<template><LinkList><a SCOPE="{a}" /></LinkList></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedScopeAttribute::NAME,
            NoDeprecatedScopeAttribute::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
