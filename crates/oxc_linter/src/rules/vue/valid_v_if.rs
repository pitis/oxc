use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{
        directive_key_span, directive_value_missing, get_directive, has_directive, walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn with_v_else_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'v-if' and 'v-else' directives can't exist on the same element. You may want 'v-else-if' directives.",
    )
    .with_help("Remove one of the directives, or change `v-else` to `v-else-if` with its own condition.")
    .with_label(span)
}

fn with_v_else_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-if' and 'v-else-if' directives can't exist on the same element.")
        .with_help("Remove one of the directives; an element can only start one branch of a conditional chain.")
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-if' directives require no argument.")
        .with_help("Remove the argument, e.g. use `v-if=\"condition\"` instead of `v-if:arg=\"condition\"`.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-if' directives require no modifier.")
        .with_help("Remove the modifier; `v-if` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-if' directives require that attribute value.")
        .with_help("Give `v-if` a condition expression, e.g. `v-if=\"isVisible\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVIf;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-if` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, a required condition value, and no combining
    /// `v-if` with `v-else`/`v-else-if` on the same element.
    ///
    /// ### Why is this bad?
    ///
    /// `v-if` accepts none of these variations; using them produces a
    /// template that either fails to compile or silently does nothing
    /// useful. Combining `v-if` with `v-else`/`v-else-if` on the same element
    /// is contradictory — an element cannot both start and continue a
    /// conditional chain.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if />
    ///   <div v-if:foo="condition" />
    ///   <div v-if.foo="condition" />
    ///   <div v-if="condition" v-else />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="condition" />
    ///   <div v-else-if="otherCondition" />
    ///   <div v-else />
    /// </template>
    /// ```
    ValidVIf,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-if` directives.",
);

impl Rule for ValidVIf {}

impl VueTemplateRule for ValidVIf {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "if", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            if has_directive(element, "else", None) {
                ctx.diagnostic(with_v_else_diagnostic(attribute.span));
            }
            if has_directive(element, "else-if", None) {
                ctx.diagnostic(with_v_else_if_diagnostic(attribute.span));
            }
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

    use super::ValidVIf;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-if="foo" /></template>"#,
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
            // No value.
            (r"<template><div v-if /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div v-if="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-if:aaa="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-if.aaa="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Combined with v-else.
            (
                r#"<template><div v-if="foo" v-else /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Combined with v-else-if.
            (
                r#"<template><div v-if="foo" v-else-if="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVIf::NAME, ValidVIf::PLUGIN, pass, fail).test_and_snapshot();
    }
}
