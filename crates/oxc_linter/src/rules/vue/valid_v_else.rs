use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{
        directive_modifiers_span, get_directive, has_directive, prev_element_sibling,
        walk_elements_with_siblings,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn missing_v_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'v-else' directives require being preceded by the element which has a 'v-if' or 'v-else-if' directive.",
    )
    .with_help("Add a preceding element with `v-if`/`v-else-if`, or remove this `v-else`.")
    .with_label(span)
}

fn with_v_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'v-else' and 'v-if' directives can't exist on the same element. You may want 'v-else-if' directives.",
    )
    .with_help("Remove one of the directives, or change `v-if` to `v-else-if` with its own condition.")
    .with_label(span)
}

fn with_v_else_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else' and 'v-else-if' directives can't exist on the same element.")
        .with_help(
            "Remove one of the directives; an element can only end a conditional chain once.",
        )
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else' directives require no argument.")
        .with_help("Remove the argument; `v-else` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else' directives require no modifier.")
        .with_help("Remove the modifier; `v-else` does not accept any.")
        .with_label(span)
}

fn unexpected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else' directives require no attribute value.")
        .with_help("Remove the value; `v-else=\"...\"` doesn't take a condition, use `v-else-if` for that.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVElse;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-else` directives in Vue `<template>` blocks: must
    /// immediately follow an element with `v-if`/`v-else-if`, no argument,
    /// no modifiers, and no value.
    ///
    /// ### Why is this bad?
    ///
    /// `v-else` only makes sense as the tail of a `v-if`/`v-else-if` chain.
    /// Without a matching preceding `v-if`/`v-else-if`, or when combined with
    /// them on the same element, or given an argument/modifier/value it
    /// doesn't accept, the directive is either ignored by Vue or produces a
    /// compile error.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-else />
    ///   <div v-if="foo" />
    ///   <div v-else="bar" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="condition" />
    ///   <div v-else />
    /// </template>
    /// ```
    ValidVElse,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-else` directives.",
);

impl Rule for ValidVElse {}

impl VueTemplateRule for ValidVElse {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements_with_siblings(nodes, &mut |element, siblings, index| {
            let Some(attribute) = get_directive(element, "else", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            let prev_has_if = prev_element_sibling(siblings, index).is_some_and(|prev| {
                has_directive(prev, "if", None) || has_directive(prev, "else-if", None)
            });
            if !prev_has_if {
                ctx.diagnostic(missing_v_if_diagnostic(attribute.span));
            }
            if has_directive(element, "if", None) {
                ctx.diagnostic(with_v_if_diagnostic(attribute.span));
            }
            if has_directive(element, "else-if", None) {
                ctx.diagnostic(with_v_else_if_diagnostic(attribute.span));
            }
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

    use super::ValidVElse;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-if="foo" /><div v-else /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-if="foo" /><div v-else-if="bar" /><div v-else /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Whitespace text and comments between branches don't break the chain.
            (
                "<template><div v-if=\"foo\" /> \n <!-- comment --> \n <div v-else /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No preceding v-if/v-else-if.
            (r"<template><div v-else /></template>", None, None, Some(PathBuf::from("test.vue"))),
            (
                r"<template><div /><div v-else /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Combined with v-if.
            (
                r#"<template><div v-if="foo" v-else /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Combined with v-else-if.
            (
                r#"<template><div v-if="foo" /><div v-else-if="bar" v-else /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-if="foo" /><div v-else:aaa /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-if="foo" /><div v-else.aaa /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Value.
            (
                r#"<template><div v-if="foo" /><div v-else="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVElse::NAME, ValidVElse::PLUGIN, pass, fail).test_and_snapshot();
    }
}
