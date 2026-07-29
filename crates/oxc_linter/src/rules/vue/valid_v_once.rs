use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_modifiers_span, get_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-once' directives require no argument.")
        .with_help("Remove the argument; `v-once` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-once' directives require no modifier.")
        .with_help("Remove the modifier; `v-once` does not accept any.")
        .with_label(span)
}

fn unexpected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-once' directives require no attribute value.")
        .with_help("Remove the value; `v-once` is a plain marker with nothing to bind.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVOnce;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-once` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, and no value.
    ///
    /// ### Why is this bad?
    ///
    /// `v-once` accepts none of these variations; using them produces a
    /// template that either fails to compile or silently does nothing
    /// useful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-once:foo />
    ///   <div v-once.foo />
    ///   <div v-once="foo" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-once />
    /// </template>
    /// ```
    ValidVOnce,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-once` directives.",
);

impl Rule for ValidVOnce {}

impl VueTemplateRule for ValidVOnce {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "once", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            if !directive.modifiers.is_empty() {
                ctx.diagnostic(unexpected_modifier_diagnostic(directive_modifiers_span(
                    attribute,
                    ctx.source_text(),
                )));
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

    use super::ValidVOnce;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (r"<template><div v-once /></template>", None, None, Some(PathBuf::from("test.vue"))),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Argument.
            (
                r"<template><div v-once:aaa /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r"<template><div v-once.aaa /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Value.
            (
                r#"<template><div v-once="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVOnce::NAME, ValidVOnce::PLUGIN, pass, fail).test_and_snapshot();
    }
}
