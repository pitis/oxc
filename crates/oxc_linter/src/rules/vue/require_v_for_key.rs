use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::{Element, Node};

use crate::{
    rule::Rule,
    utils::{
        element_name_eq_lower, has_directive, is_custom_component, start_tag_span, walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Elements in iteration expect to have 'v-bind:key' directives.")
        .with_help("Add a `:key` binding so Vue can track each node's identity, e.g. `v-for=\"item in items\" :key=\"item.id\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireVForKey;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `v-bind:key` with `v-for` directives in Vue `<template>` blocks.
    ///
    /// ### Why is this bad?
    ///
    /// Without a `key`, Vue patches list elements in place: when the list
    /// reorders, element state (form inputs, component state, transition
    /// state) stays at the old position instead of moving with the data.
    /// A unique `key` lets Vue track each node's identity.
    ///
    /// Custom components are not reported by this rule (that case is covered
    /// by `vue/valid-v-for` in eslint-plugin-vue).
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="todo in todos" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="todo in todos" :key="todo.id" />
    /// </template>
    /// ```
    RequireVForKey,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Require `v-bind:key` with `v-for` directives.",
);

impl Rule for RequireVForKey {}

impl VueTemplateRule for RequireVForKey {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if has_directive(element, "for", None) {
                check_key(element, ctx);
            }
        });
    }
}

/// eslint-plugin-vue `require-v-for-key`'s `checkKey`: a keyed element is
/// fine; `<template>`/`<slot>` push the requirement down to their children;
/// anything else must be keyed unless it is a custom component.
fn check_key<'a>(element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
    if has_directive(element, "bind", Some("key")) {
        return;
    }
    if element_name_eq_lower(element, "template") || element_name_eq_lower(element, "slot") {
        for child in &element.children {
            if let Node::Element(child_element) = child {
                check_key(child_element, ctx);
            }
        }
    } else if !is_custom_component(element) {
        ctx.diagnostic(require_key_diagnostic(start_tag_span(element, ctx.source_text())));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireVForKey;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-for="item in items" :key="item.id">{{ item }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item in items" v-bind:key="item.id" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `<template v-for>`: the key may sit on the template tag itself…
            (
                r#"<template><template v-for="item in items" :key="item.id"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // …or on every child element.
            (
                r#"<template><template v-for="item in items"><div :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom components are not this rule's concern.
            (
                r#"<template><MyRow v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item in items" :is="item.component" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No v-for, no requirement.
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (
                r#"<template><Template v-for="a in b"><div /></Template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item in items">{{ item }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested inside other markup, with a script block in the file.
            (
                "<script setup>\nconst items = [];\n</script>\n<template>\n  <ul>\n    <li v-for=\"item in items\">{{ item }}</li>\n  </ul>\n</template>\n",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(RequireVForKey::NAME, RequireVForKey::PLUGIN, pass, fail).test_and_snapshot();
    }
}
