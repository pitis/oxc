use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{
        directive_modifiers_span, directive_value_missing, get_directive, has_directive,
        prev_element_sibling, walk_elements_with_siblings,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn missing_v_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'v-else-if' directives require being preceded by the element which has a 'v-if' or 'v-else-if' directive.",
    )
    .with_help("Add a preceding element with `v-if`/`v-else-if`, or change this to `v-if`.")
    .with_label(span)
}

fn with_v_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else-if' and 'v-if' directives can't exist on the same element.")
        .with_help("Remove one of the directives; an element can only start one branch of a conditional chain.")
        .with_label(span)
}

fn with_v_else_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else-if' and 'v-else' directives can't exist on the same element.")
        .with_help(
            "Remove one of the directives; an element can only end a conditional chain once.",
        )
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else-if' directives require no argument.")
        .with_help("Remove the argument; `v-else-if` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else-if' directives require no modifier.")
        .with_help("Remove the modifier; `v-else-if` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-else-if' directives require that attribute value.")
        .with_help("Give `v-else-if` a condition expression, e.g. `v-else-if=\"otherCondition\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVElseIf;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-else-if` directives in Vue `<template>` blocks:
    /// must immediately follow an element with `v-if`/`v-else-if`, no
    /// argument, no modifiers, and a required condition value.
    ///
    /// ### Why is this bad?
    ///
    /// `v-else-if` only makes sense as a continuation of a `v-if` chain.
    /// Without a matching preceding `v-if`/`v-else-if`, or when combined
    /// with them on the same element, or missing the condition it requires,
    /// the directive is either ignored by Vue or produces a compile error.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-else-if="foo" />
    ///   <div v-if="foo" />
    ///   <div v-else-if />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="condition" />
    ///   <div v-else-if="otherCondition" />
    /// </template>
    /// ```
    ValidVElseIf,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-else-if` directives.",
);

impl Rule for ValidVElseIf {}

impl VueTemplateRule for ValidVElseIf {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements_with_siblings(nodes, &mut |element, siblings, index| {
            let Some(attribute) = get_directive(element, "else-if", None) else { return };
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
            if has_directive(element, "else", None) {
                ctx.diagnostic(with_v_else_diagnostic(attribute.span));
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
            if directive_value_missing(attribute) {
                ctx.diagnostic(expected_value_diagnostic(attribute.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVElseIf;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-if="foo" /><div v-else-if="bar" /></template>"#,
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
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No preceding v-if/v-else-if.
            (
                r#"<template><div v-else-if="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div /><div v-else-if="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Combined with v-if.
            (
                r#"<template><div v-if="foo" v-else-if="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Combined with v-else.
            (
                r#"<template><div v-if="foo" /><div v-else v-else-if="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-if="foo" /><div v-else-if:aaa="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-if="foo" /><div v-else-if.aaa="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No value.
            (
                r#"<template><div v-if="foo" /><div v-else-if /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty value.
            (
                r#"<template><div v-if="foo" /><div v-else-if="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVElseIf::NAME, ValidVElseIf::PLUGIN, pass, fail).test_and_snapshot();
    }
}
