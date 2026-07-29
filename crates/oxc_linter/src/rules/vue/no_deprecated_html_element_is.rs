use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{get_attribute, get_directive, is_html_svg_or_math_element_name, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn html_element_is_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `is` attribute on HTML element are deprecated.")
        .with_help(
            "Vue 3 only allows `is`/`:is` on native HTML elements when the value is prefixed \
             with `vue:` (e.g. `is=\"vue:my-component\"`); otherwise rename the tag to the \
             component directly.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedHtmlElementIs;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `is`/`v-bind:is`/`:is` attribute on native HTML, SVG, or
    /// MathML elements, unless (for the plain `is="..."` form) its value is
    /// prefixed with `vue:`.
    ///
    /// ### Why is this bad?
    ///
    /// Vue 2 used `is` on a native element to work around HTML parsing
    /// restrictions (e.g. `<table><tr is="my-row">`). Vue 3 repurposed
    /// unprefixed `is` on native elements to the [customized built-in
    /// element](https://html.spec.whatwg.org/multipage/custom-elements.html#custom-elements-customized-builtin-example)
    /// Web Components feature instead, so the old Vue 2 meaning silently
    /// stops applying; a `vue:` prefix (or renaming the tag to the component
    /// itself) is required to keep resolving it as a Vue component.
    ///
    /// A dynamically bound `:is`/`v-bind:is` on a native element is always
    /// reported, since its value can't be checked for a `vue:` prefix
    /// statically.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div is="my-component"></div>
    ///   <table>
    ///     <tr :is="rowComponent"></tr>
    ///   </table>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <my-component></my-component>
    ///   <div is="vue:my-component"></div>
    /// </template>
    /// ```
    NoDeprecatedHtmlElementIs,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow deprecated `is` attribute on HTML elements.",
);

impl Rule for NoDeprecatedHtmlElementIs {}

impl VueTemplateRule for NoDeprecatedHtmlElementIs {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // Only native HTML/SVG/MathML elements are in scope; a custom
            // component (or an element made custom by its own `is` value —
            // eslint-plugin-vue's `isValidElement` checks the *tag name*
            // only, never `is_custom_component`'s `is`-attribute carve-out)
            // is unaffected.
            if !is_html_svg_or_math_element_name(element.name) {
                return;
            }

            // `v-bind:is`/`:is` (any static or dynamic argument text other
            // than a literal `"is"` doesn't reach here — `get_directive`
            // already requires a static match): always reported, regardless
            // of value, matching eslint-plugin-vue's bound-form handler,
            // which never inspects `node.value` at all. Verified against
            // real eslint-plugin-vue: `<div :is="'vue:x'">` still fires.
            if let Some(attribute) = get_directive(element, "bind", Some("is")) {
                ctx.diagnostic(html_element_is_diagnostic(attribute.span));
            }

            // Plain `is="..."`: suppressed only when the value is present
            // and starts with the literal `vue:` prefix (case-sensitive,
            // matching upstream's `startsWith("vue:")`).
            if let Some(attribute) = get_attribute(element, "is") {
                let has_vue_prefix =
                    attribute.value.as_ref().is_some_and(|value| value.text.starts_with("vue:"));
                if !has_vue_prefix {
                    ctx.diagnostic(html_element_is_diagnostic(attribute.span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedHtmlElementIs;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // `vue:`-prefixed plain `is` on a native element is the correct
            // Vue 3 replacement.
            (
                r#"<template><div is="vue:my-component"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A custom element's own `is` attribute is unaffected — only
            // the *tag name* gates this rule, not whether the element ends
            // up custom.
            (
                r#"<template><custom-element is="x"></custom-element></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><custom-element :is="x"></custom-element></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A PascalCase tag is a component reference, never a native
            // element name match (case-sensitive, exact match).
            (
                r#"<template><MyComponent is="x"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No `is` attribute at all.
            (r"<template><div></div></template>", None, None, Some(PathBuf::from("test.vue"))),
            // A dynamic argument named (via a variable) `dyn`, not a static
            // `is` argument — never matches, dynamic or not.
            (
                r#"<template><div v-bind:[dyn]="x"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Plain `is` without the `vue:` prefix, on a native element.
            (
                r#"<template><div is="my-component"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Bound `:is` on a native element: always reported, regardless
            // of the (dynamic, unknowable) value.
            (
                r#"<template><div :is="something"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-bind:is` longhand.
            (
                r#"<template><div v-bind:is="something"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Native elements found via a name pushed down through the
            // start-tag scan: table, svg, math (HTML/SVG/MathML sets).
            (
                r#"<template><table is="my-table"></table></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><svg is="my-svg"></svg></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><math is="my-math"></math></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoDeprecatedHtmlElementIs::NAME, NoDeprecatedHtmlElementIs::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
