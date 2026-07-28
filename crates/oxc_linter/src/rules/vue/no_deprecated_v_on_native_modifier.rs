use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_modifier_span, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn native_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'.native' modifier on 'v-on' directive is deprecated.")
        .with_help(
            "Vue 3 removed the `.native` modifier; listen for native DOM events on the root element with `emits`/`inheritAttrs: false` instead, or bind directly to a plain element.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedVOnNativeModifier;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `.native` modifier on `v-on` directives
    /// (Vue 3.0+ removed it).
    ///
    /// ### Why is this bad?
    ///
    /// `.native` has no effect in Vue 3; a listener relying on it silently
    /// stops receiving the native DOM event it used to.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent @click.native="onClick" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent @click="onClick" />
    /// </template>
    /// ```
    NoDeprecatedVOnNativeModifier,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow using deprecated `.native` modifiers.",
);

impl Rule for NoDeprecatedVOnNativeModifier {}

impl VueTemplateRule for NoDeprecatedVOnNativeModifier {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "on" {
                    continue;
                }
                // Every occurrence of a `native` *modifier* is reported —
                // this only ever looks at `Directive::modifiers`, so a
                // `native` used as the event name/argument instead (e.g.
                // `@native.enter`, where `native` is the event and `enter`
                // is the modifier) is correctly not flagged, matching
                // eslint-plugin-vue's `!node.parent.modifiers.includes(node)`
                // guard.
                for (index, modifier) in directive.modifiers.iter().enumerate() {
                    if *modifier == "native" {
                        ctx.diagnostic(native_modifier_diagnostic(directive_modifier_span(
                            attribute,
                            ctx.source_text(),
                            index,
                        )));
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedVOnNativeModifier;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><input v-on:keyup.enter='fire'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.enter='fire'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `native` is the directive *name* here, not `on`.
            (
                r"<template><input v-native:foo.native.foo.bar='fire'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `native` is the event *argument*, not a modifier.
            (
                r"<template><input @native.enter='fire'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `:` is `v-bind`, not `v-on`.
            (
                r"<template><input :keydown.native='fire'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r"<template><input v-on:keyup.native='fore'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input v-on:keyup.foo.native.bar='fore'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @click.native='onClick'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedVOnNativeModifier::NAME,
            NoDeprecatedVOnNativeModifier::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
