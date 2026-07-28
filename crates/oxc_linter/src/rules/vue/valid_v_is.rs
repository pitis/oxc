use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{
        directive_key_span, directive_value_missing, get_directive, is_reserved_element_name,
        walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-is' directives require no argument.")
        .with_help("Remove the argument, e.g. use `v-is=\"componentName\"`.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-is' directives require no modifier.")
        .with_help("Remove the modifier; `v-is` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-is' directives require that attribute value.")
        .with_help("Give `v-is` a component name expression, e.g. `v-is=\"componentName\"`.")
        .with_label(span)
}

fn owner_must_be_html_element_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "'v-is' directive must be owned by a native HTML element, but '{name}' is not."
    ))
    .with_help("Use `v-is` only on native HTML tags (e.g. `<component>` is not a native tag); use `:is` on components instead.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVIs;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-is` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, a required value, and it must be used on a
    /// native (well-known) HTML element rather than a component.
    ///
    /// ### Why is this bad?
    ///
    /// `v-is` is a workaround for native-element parsing restrictions (e.g.
    /// inside `<table>`) and only makes sense on native HTML tags; using it
    /// on a custom component, or with an argument/modifier/no value, either
    /// fails to compile or does nothing useful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-is />
    ///   <div v-is:foo="componentName" />
    ///   <div v-is.foo="componentName" />
    ///   <MyComponent v-is="componentName" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <tr v-is="componentName" />
    /// </template>
    /// ```
    ValidVIs,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-is` directives.",
);

impl Rule for ValidVIs {}

impl VueTemplateRule for ValidVIs {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "is", None) else { return };
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
            if !is_reserved_element_name(element.name) {
                ctx.diagnostic(owner_must_be_html_element_diagnostic(element.name, attribute.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVIs;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><tr v-is="componentName" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-is="componentName" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No value.
            (r"<template><tr v-is /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (r#"<template><tr v-is="" /></template>"#, None, None, Some(PathBuf::from("test.vue"))),
            // Argument.
            (
                r#"<template><tr v-is:foo="componentName" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><tr v-is.foo="componentName" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Owner is a custom component, not a native HTML element.
            (
                r#"<template><MyComponent v-is="componentName" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><my-component v-is="componentName" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVIs::NAME, ValidVIs::PLUGIN, pass, fail).test_and_snapshot();
    }
}
