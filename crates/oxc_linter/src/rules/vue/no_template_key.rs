use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{get_attribute, get_directive, has_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn no_template_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'<template>' cannot be keyed. Place the key on real elements instead.")
        .with_help(
            "Move the `key`/`:key` onto the element(s) `<template>` wraps, or add `v-for` to \
             the `<template>` if that's what needs the key.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoTemplateKey;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows a `key` attribute (plain `key` or bound `v-bind:key`/`:key`)
    /// on `<template>` elements, except `<template v-for>`, which Vue 3
    /// specifically supports keying.
    ///
    /// ### Why is this bad?
    ///
    /// `<template>` is a grouping construct, not a real rendered element, so
    /// a `key` on it (outside the one case Vue actually reads it —
    /// `v-for`) has no effect and misleads readers into thinking it does.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <template key="foo"></template>
    ///   <template v-if="cond" :key="foo"></template>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div key="foo"></div>
    ///   <template v-for="item in items" :key="item.id"></template>
    /// </template>
    /// ```
    NoTemplateKey,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow `key` attribute on `<template>`.",
);

impl Rule for NoTemplateKey {}

impl VueTemplateRule for NoTemplateKey {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if element.name != "template" {
                return;
            }
            // eslint-plugin-vue's `getAttribute(node, 'key') ||
            // getDirective(node, 'bind', 'key')`: a plain `key` attribute
            // takes priority over a bound one when (implausibly) both exist.
            let key_span =
                get_attribute(element, "key").map(|attribute| attribute.span).or_else(|| {
                    get_directive(element, "bind", Some("key")).map(|attribute| attribute.span)
                });
            let Some(span) = key_span else { return };
            if has_directive(element, "for", None) {
                return;
            }
            ctx.diagnostic(no_template_key_diagnostic(span));
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoTemplateKey;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><template></template></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div key="foo"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items" :key="item.id"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items" key="a"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items" v-bind:key="item.id"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><template key="foo"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template :key="foo"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-bind:key="foo"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A structural directive other than v-for doesn't exempt the key.
            (
                r#"<template><template v-if="cond" :key="foo"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested inside another element.
            (
                r#"<template><div><template key="foo"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoTemplateKey::NAME, NoTemplateKey::PLUGIN, pass, fail).test_and_snapshot();
    }
}
