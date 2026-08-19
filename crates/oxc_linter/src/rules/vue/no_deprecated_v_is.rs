use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_key_span, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn v_is_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`v-is` directive is deprecated.")
        .with_help(
            "Vue 3.1 deprecated `v-is`; use a dynamic `is` binding instead, e.g. `:is=\"'vue:' + name\"`.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedVIs;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `v-is` directive (Vue 3.1+ deprecated it in
    /// favor of a dynamic `is` binding prefixed with `vue:`).
    ///
    /// ### Why is this bad?
    ///
    /// `v-is` is deprecated and scheduled for removal; new code should use
    /// `:is="'vue:' + name"` (or a bound `is` on `<component>`) instead.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-is="'MyComponent'" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div :is="'vue:MyComponent'" />
    /// </template>
    /// ```
    NoDeprecatedVIs,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow deprecated `v-is` directive.",
);

impl Rule for NoDeprecatedVIs {}

impl VueTemplateRule for NoDeprecatedVIs {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                if attribute.directive.as_ref().is_some_and(|directive| directive.name == "is") {
                    ctx.diagnostic(v_is_diagnostic(directive_key_span(attribute)));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedVIs;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div is="vue:MyComponent" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><component is="MyComponent" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :is="MyComponent" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![(
            r#"<template><div v-is="'MyComponent'" /></template>"#,
            None,
            None,
            Some(PathBuf::from("test.vue")),
        )];

        Tester::new(NoDeprecatedVIs::NAME, NoDeprecatedVIs::PLUGIN, pass, fail).test_and_snapshot();
    }
}
