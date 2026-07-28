use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_key_span, directive_value_missing, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

/// eslint-plugin-vue `valid-v-bind`'s `VALID_MODIFIERS`. `sync` is kept even
/// though the `.sync` modifier itself was removed in Vue 3 (superseded by
/// `v-model:foo`) because upstream still accepts it as a *known* (if
/// pointless) modifier rather than an unsupported one — copied verbatim.
const VALID_MODIFIERS: &[&str] = &["prop", "camel", "sync", "attr"];

fn unsupported_modifier_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'v-bind' directives don't support the modifier '{name}'."))
        .with_help(
            "Remove the modifier; `v-bind` only supports `.prop`, `.camel`, `.sync`, and `.attr`.",
        )
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-bind' directives require an attribute value.")
        .with_help(
            "Give the binding a value, e.g. `:foo=\"bar\"`, or use a plain attribute instead.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVBind;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-bind` directives in Vue `<template>` blocks: only
    /// the `prop`, `camel`, `sync`, and `attr` modifiers are recognized, and
    /// a binding must have a value.
    ///
    /// ### Why is this bad?
    ///
    /// An unsupported modifier or a value-less binding either fails to
    /// compile or silently does nothing useful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div :foo.bar="baz" />
    ///   <div :foo />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div :foo="baz" />
    ///   <div :foo.camel="baz" />
    ///   <div v-bind="allProps" />
    /// </template>
    /// ```
    ValidVBind,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-bind` directives.",
);

impl Rule for ValidVBind {}

impl VueTemplateRule for ValidVBind {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "bind" {
                    continue;
                }

                for modifier in &directive.modifiers {
                    if !VALID_MODIFIERS.contains(modifier) {
                        ctx.diagnostic(unsupported_modifier_diagnostic(
                            modifier,
                            directive_key_span(attribute),
                        ));
                    }
                }

                if directive_value_missing(attribute) {
                    ctx.diagnostic(expected_value_diagnostic(attribute.span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVBind;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-bind:foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo.camel="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo.prop="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo.attr="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo.sync="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `.prop` shorthand.
            (
                r#"<template><div .foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic argument.
            (
                r#"<template><div :[foo]="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Spread bind, no argument.
            (
                r#"<template><div v-bind="allProps" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r#"<template><div v-bind:foo.bar="baz" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo.bar="baz" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No value.
            (r"<template><div :foo /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div :foo="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Spread with unsupported modifier.
            (
                r#"<template><div v-bind.bar="allProps" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Multiple unsupported modifiers reported individually.
            (
                r#"<template><div :foo.bar.baz="qux" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVBind::NAME, ValidVBind::PLUGIN, pass, fail).test_and_snapshot();
    }
}
