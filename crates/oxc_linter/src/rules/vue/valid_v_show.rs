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
    OxcDiagnostic::warn("'v-show' directives require no argument.")
        .with_help("Remove the argument; `v-show` does not accept one.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-show' directives require no modifier.")
        .with_help("Remove the modifier; `v-show` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-show' directives require that attribute value.")
        .with_help("Give `v-show` a condition expression, e.g. `v-show=\"isVisible\"`.")
        .with_label(span)
}

fn unexpected_template_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-show' directives cannot be put on <template> tags.")
        .with_help("`v-show` toggles CSS display and needs a real element to apply to; move it to a child element or use `v-if` instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVShow;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-show` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, a required condition value, and not placed on
    /// a `<template>` tag.
    ///
    /// ### Why is this bad?
    ///
    /// `v-show` accepts none of these variations, and it works by toggling
    /// an element's inline CSS `display`, which `<template>` — a non-rendered
    /// wrapper — has no DOM node to apply.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-show />
    ///   <div v-show:foo="condition" />
    ///   <template v-show="condition"><div /></template>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-show="condition" />
    /// </template>
    /// ```
    ValidVShow,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-show` directives.",
);

impl Rule for ValidVShow {}

impl VueTemplateRule for ValidVShow {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "show", None) else { return };
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
            if element.name == "template" {
                ctx.diagnostic(unexpected_template_diagnostic(attribute.span));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVShow;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-show="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No value.
            (r"<template><div v-show /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div v-show="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-show:aaa="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-show.aaa="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // On <template>.
            (
                r#"<template><template v-show="foo"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVShow::NAME, ValidVShow::PLUGIN, pass, fail).test_and_snapshot();
    }
}
