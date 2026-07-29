use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn sync_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'.sync' modifier on 'v-bind' directive is deprecated. Use 'v-model:propName' instead.",
    )
    .with_help("Replace `:propName.sync=\"value\"` with `v-model:propName=\"value\"`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedVBindSync;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated `.sync` modifier on `v-bind` directives
    /// (Vue 3.0+ removed it in favor of `v-model:propName`).
    ///
    /// ### Why is this bad?
    ///
    /// `.sync` has no effect in Vue 3; a template relying on it silently
    /// loses the two-way binding it used to provide.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent :foo.sync="bar" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent v-model:foo="bar" />
    /// </template>
    /// ```
    NoDeprecatedVBindSync,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow use of deprecated `.sync` modifier on `v-bind` directive.",
);

impl Rule for NoDeprecatedVBindSync {}

impl VueTemplateRule for NoDeprecatedVBindSync {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                // eslint-plugin-vue reports on the whole `VAttribute` node
                // (`node`/`node.loc`), not just its key — verified against
                // real eslint-plugin-vue: the reported span covers the
                // entire `:foo.sync="bar"`, value included.
                if directive.name == "bind" && directive.modifiers.contains(&"sync") {
                    ctx.diagnostic(sync_modifier_diagnostic(attribute.span));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedVBindSync;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><MyComponent v-bind:foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent :foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-bind:[dynamicArg]="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent :[dynamicArg]="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><MyComponent v-bind:foo.sync="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent :foo.sync="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-bind:[dynamicArg].sync="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent :[dynamicArg].sync="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Spread `v-bind.sync` (no argument).
            (
                r#"<template><MyComponent v-bind.sync="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `.sync` alongside another modifier.
            (
                r#"<template><MyComponent :foo.sync.unknown="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoDeprecatedVBindSync::NAME, NoDeprecatedVBindSync::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
