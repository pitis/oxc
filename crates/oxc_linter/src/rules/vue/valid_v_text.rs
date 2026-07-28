use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_key_span, directive_value_missing, get_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-text' directives require no argument.")
        .with_help("Remove the argument; `v-text` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-text' directives require no modifier.")
        .with_help("Remove the modifier; `v-text` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-text' directives require that attribute value.")
        .with_help("Give `v-text` an expression that evaluates to the text to render, e.g. `v-text=\"message\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVText;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-text` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, and a required value.
    ///
    /// ### Why is this bad?
    ///
    /// `v-text` accepts none of these variations; using them produces a
    /// template that either fails to compile or silently does nothing
    /// useful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-text />
    ///   <div v-text:foo="message" />
    ///   <div v-text.foo="message" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-text="message" />
    /// </template>
    /// ```
    ValidVText,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-text` directives.",
);

impl Rule for ValidVText {}

impl VueTemplateRule for ValidVText {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "text", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            if !directive.modifiers.is_empty() {
                ctx.diagnostic(unexpected_modifier_diagnostic(directive_key_span(attribute)));
            }
            if directive_value_missing(attribute) {
                ctx.diagnostic(expected_value_diagnostic(attribute.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVText;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-text="message" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No value.
            (r"<template><div v-text /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div v-text="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-text:aaa="message" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-text.aaa="message" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVText::NAME, ValidVText::PLUGIN, pass, fail).test_and_snapshot();
    }
}
