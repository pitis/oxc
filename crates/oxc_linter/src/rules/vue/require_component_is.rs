use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{element_name_eq_lower, has_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_component_is_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected '<component>' elements to have 'v-bind:is' attribute.")
        .with_help("Add `:is=\"...\"` (or `v-bind:is=\"...\"`) to tell `<component>` which component to render.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireComponentIs;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a bound `v-bind:is`/`:is` attribute on every `<component>`
    /// element. A plain (non-bound) `is` attribute, or `v-is`, does not
    /// satisfy this rule — it must specifically be `v-bind:is`.
    ///
    /// ### Why is this bad?
    ///
    /// `<component>` renders nothing on its own; without `:is` telling it
    /// which component (or element) to render, it is always a no-op —
    /// almost certainly a mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <component></component>
    ///   <component is="my-component"></component>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <component :is="currentView"></component>
    /// </template>
    /// ```
    RequireComponentIs,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Require `v-bind:is` of `<component>` elements.",
);

impl Rule for RequireComponentIs {}

impl VueTemplateRule for RequireComponentIs {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if !element_name_eq_lower(element, "component") {
                return;
            }
            if !has_directive(element, "bind", Some("is")) {
                ctx.diagnostic(require_component_is_diagnostic(element.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireComponentIs;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><component :is="currentView"></component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><component v-bind:is="currentView"></component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (r"<template><Component /></template>", None, None, Some(PathBuf::from("test.vue"))),
            (
                r"<template><component></component></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><component is="my-component"></component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // v-is is a directive but not `v-bind:is`.
            (
                r#"<template><component v-is="currentView"></component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A dynamic argument never satisfies a static `is` requirement.
            (
                r#"<template><component v-bind:[attr]="currentView"></component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(RequireComponentIs::NAME, RequireComponentIs::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
