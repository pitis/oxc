use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::Sfc;

use crate::{
    rule::Rule,
    vue_template::{VueSfcRule, VueTemplateContext},
};

fn functional_template_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `functional` template are deprecated.")
        .with_help(
            "Vue 3 removed functional SFC templates; use a plain function component or a \
             regular component with the same effect instead.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedFunctionalTemplate;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `functional` attribute on an SFC's own
    /// `<template>` block (Vue 3 removed `functional` templates).
    ///
    /// ### Why is this bad?
    ///
    /// `<template functional>` declared a stateless, instance-less
    /// "functional component" template in Vue 2. Vue 3 dropped the
    /// distinction (every `<script setup>` component is already
    /// functional-ish), so `functional` on the template block has no effect
    /// — it neither errors nor changes behavior, it's just dead markup.
    ///
    /// Only the SFC's own outer `<template>` block is checked — a
    /// same-named `functional` attribute on a *nested* `<template>` element
    /// inside the markup (e.g. one used with `v-slot`) is unrelated and not
    /// reported, mirroring eslint-plugin-vue's `program.templateBody` check.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template functional>
    ///   <div>{{ props.msg }}</div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>{{ msg }}</div>
    /// </template>
    /// ```
    NoDeprecatedFunctionalTemplate,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow deprecated `functional` template.",
);

impl Rule for NoDeprecatedFunctionalTemplate {}

impl VueSfcRule for NoDeprecatedFunctionalTemplate {
    fn run_on_sfc<'a>(&self, sfc: &Sfc<'a>, _path: &Path, ctx: &mut VueTemplateContext<'a>) {
        for block in &sfc.blocks {
            if block.name != "template" {
                continue;
            }
            // eslint-plugin-vue's `utils.getAttribute(element, "functional")`
            // matches the parser's (lowercased) `key.name`, not `rawName` —
            // unlike `no-deprecated-inline-template` (see that rule's doc
            // comment), this *is* case-insensitive. Verified against real
            // eslint-plugin-vue: `<template FUNCTIONAL>` is reported.
            let Some(attribute) = block.attributes.iter().find(|attribute| {
                attribute.directive.is_none() && attribute.name.eq_ignore_ascii_case("functional")
            }) else {
                continue;
            };
            ctx.diagnostic(functional_template_diagnostic(attribute.span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedFunctionalTemplate;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<template><div></div></template>", None, None, Some(PathBuf::from("test.vue"))),
            // A `functional` attribute on a *nested* `<template>` element
            // (not the SFC's own outer block) is unrelated to this rule.
            (
                "<template><div><template functional><span></span></template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No `<template>` block at all: nothing to check.
            (
                "<script setup>\nconst a = 1;\n</script>\n",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                "<template functional><div></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // With a value.
            (
                r#"<template functional="true"><div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Case-insensitive (unlike `no-deprecated-inline-template`'s
            // `rawName`-based check).
            (
                "<template FUNCTIONAL><div></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedFunctionalTemplate::NAME,
            NoDeprecatedFunctionalTemplate::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
