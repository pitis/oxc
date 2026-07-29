use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn inline_template_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`inline-template` are deprecated.")
        .with_help(
            "Vue 3 removed the `inline-template` attribute; use a scoped slot or a real \
             single-file component instead.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedInlineTemplate;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `inline-template` attribute (Vue 3 removed
    /// it entirely).
    ///
    /// ### Why is this bad?
    ///
    /// `inline-template` told Vue 2 to compile a component's light DOM
    /// (its children as written in the parent) as that component's own
    /// template, instead of treating it as slot content. Vue 3 dropped the
    /// feature, so `inline-template` has no effect — a component that relied
    /// on it silently falls back to normal slot behavior.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <my-component inline-template>
    ///     <p>{{ message }}</p>
    ///   </my-component>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <my-component>
    ///     <template #default="{ message }">
    ///       <p>{{ message }}</p>
    ///     </template>
    ///   </my-component>
    /// </template>
    /// ```
    NoDeprecatedInlineTemplate,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow deprecated `inline-template` attribute.",
);

impl Rule for NoDeprecatedInlineTemplate {}

impl VueTemplateRule for NoDeprecatedInlineTemplate {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                // eslint-plugin-vue's selector is
                // `VAttribute[directive=false] > VIdentifier[rawName='inline-template']`
                // — `rawName`, not the (lowercased) `name`, so this is a
                // case-sensitive exact match, unlike most other plain
                // attribute lookups in this codebase (see
                // `crate::utils::get_attribute`'s case-insensitive doc
                // comment). Verified against real eslint-plugin-vue:
                // `<my-component INLINE-TEMPLATE>` is *not* reported, while
                // `<my-component inline-template>` is. `Attribute::name` is
                // already the raw source text, so this compares directly
                // against it rather than going through `get_attribute`.
                if attribute.directive.is_none() && attribute.name == "inline-template" {
                    ctx.diagnostic(inline_template_diagnostic(attribute.name_span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedInlineTemplate;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><my-component></my-component></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Directive form (`:inline-template`) doesn't match the plain
            // (`directive=false`) selector.
            (
                r#"<template><my-component :inline-template="foo"></my-component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Case-sensitive: the selector matches `rawName`, not the
            // lowercased name.
            (
                r"<template><my-component INLINE-TEMPLATE></my-component></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Bare boolean form.
            (
                r"<template><my-component inline-template></my-component></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // With a value.
            (
                r#"<template><my-component inline-template="foo"></my-component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Fires on a plain (non-component) element too — the rule
            // doesn't check whether the element is a component.
            (
                r"<template><div inline-template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedInlineTemplate::NAME,
            NoDeprecatedInlineTemplate::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
