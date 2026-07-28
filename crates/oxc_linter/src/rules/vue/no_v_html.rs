use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn no_v_html_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-html' directive can lead to XSS attack.")
        .with_help("Avoid `v-html`; sanitize untrusted content before rendering it, or render it as plain text instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoVHtml;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows use of `v-html` in Vue `<template>` blocks.
    ///
    /// ### Why is this bad?
    ///
    /// Content passed to `v-html` is injected as raw HTML with no escaping.
    /// If it ever includes attacker-controlled data, that is a cross-site
    /// scripting (XSS) vector.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-html="userSuppliedHtml"></div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-text="userSuppliedText"></div>
    /// </template>
    /// ```
    NoVHtml,
    vue,
    restriction,
    version = "1.77.0",
    short_description = "Disallow use of `v-html` to prevent XSS attack.",
);

impl Rule for NoVHtml {}

impl VueTemplateRule for NoVHtml {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue visits every `VAttribute[directive=true][key.name.name='html']`
            // independently; an element can only sensibly carry one `v-html`,
            // but iterating attributes (rather than using `get_directive`,
            // which would only find the first) keeps that per-node reporting
            // granularity if it ever somehow did.
            for attribute in &element.attributes {
                if attribute.directive.as_ref().is_some_and(|directive| directive.name == "html") {
                    ctx.diagnostic(no_v_html_diagnostic(attribute.span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoVHtml;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-text="text" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r#"<template><div v-html="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-html="rawHtml">child content ignored by the check</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Component targets are still reported by no-v-html (the
            // component-specific case is `no-v-text-v-html-on-component`'s
            // job, not this rule's).
            (
                r#"<template><MyComp v-html="rawHtml" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div v-html /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        Tester::new(NoVHtml::NAME, NoVHtml::PLUGIN, pass, fail).test_and_snapshot();
    }
}
