use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_key_span, get_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-cloak' directives require no argument.")
        .with_help("Remove the argument; `v-cloak` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-cloak' directives require no modifier.")
        .with_help("Remove the modifier; `v-cloak` does not accept any.")
        .with_label(span)
}

fn unexpected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-cloak' directives require no attribute value.")
        .with_help("Remove the value; `v-cloak` is a plain marker with nothing to bind.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVCloak;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-cloak` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, and no value.
    ///
    /// ### Why is this bad?
    ///
    /// `v-cloak` accepts none of these variations; using them produces a
    /// template that either fails to compile or silently does nothing
    /// useful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-cloak:foo />
    ///   <div v-cloak.foo />
    ///   <div v-cloak="foo" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-cloak />
    /// </template>
    /// ```
    ValidVCloak,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-cloak` directives.",
);

impl Rule for ValidVCloak {}

impl VueTemplateRule for ValidVCloak {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "cloak", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            if !directive.modifiers.is_empty() {
                ctx.diagnostic(unexpected_modifier_diagnostic(directive_key_span(attribute)));
            }
            if let Some(value) = &attribute.value {
                ctx.diagnostic(unexpected_value_diagnostic(value.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVCloak;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (r"<template><div v-cloak /></template>", None, None, Some(PathBuf::from("test.vue"))),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Argument.
            (
                r"<template><div v-cloak:aaa /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r"<template><div v-cloak.aaa /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Value.
            (
                r#"<template><div v-cloak="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVCloak::NAME, ValidVCloak::PLUGIN, pass, fail).test_and_snapshot();
    }
}
