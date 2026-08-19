use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_modifiers_span, directive_value_missing, get_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-html' directives require no argument.")
        .with_help("Remove the argument; `v-html` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-html' directives require no modifier.")
        .with_help("Remove the modifier; `v-html` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-html' directives require that attribute value.")
        .with_help("Give `v-html` an expression that evaluates to the HTML string to render, e.g. `v-html=\"rawHtml\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVHtml;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-html` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, and a required value.
    ///
    /// ### Why is this bad?
    ///
    /// `v-html` accepts none of these variations; using them produces a
    /// template that either fails to compile or silently does nothing
    /// useful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-html />
    ///   <div v-html:foo="rawHtml" />
    ///   <div v-html.foo="rawHtml" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-html="rawHtml" />
    /// </template>
    /// ```
    ValidVHtml,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-html` directives.",
);

impl Rule for ValidVHtml {}

impl VueTemplateRule for ValidVHtml {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "html", None) else { return };
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
            if directive_value_missing(attribute) {
                ctx.diagnostic(expected_value_diagnostic(attribute.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVHtml;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-html="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No value.
            (r"<template><div v-html /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div v-html="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-html:aaa="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-html.aaa="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVHtml::NAME, ValidVHtml::PLUGIN, pass, fail).test_and_snapshot();
    }
}
