use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;
use rustc_hash::FxHashSet;

use crate::{
    rule::Rule,
    utils::{directive_expression, free_reference_spans, walk_nodes_with_scope},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn deprecated_scoped_slots_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `$scopedSlots` is deprecated.")
        .with_help(
            "Vue 3 merges scoped slots into `$slots`; use `$slots` instead of `$scopedSlots`.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedDollarScopedslotsApi;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows using the deprecated `$scopedSlots` object, removed in
    /// Vue 3.0.0+ (its contents were merged into `$slots`).
    ///
    /// ### Why is this bad?
    ///
    /// `$scopedSlots` no longer exists on the Vue 3 component instance;
    /// referencing it evaluates to `undefined`, silently breaking whatever
    /// depended on it.
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Template side only: upstream also flags `this.$scopedSlots` in
    /// `<script>` (via a `MemberExpression` visitor scoped to `this`
    /// inside the component definition), with an autofix to `this.$slots`.
    /// That half needs script-level semantic analysis of the component
    /// definition and is out of scope for this template-only rule; only the
    /// `VExpressionContainer` half — any `$scopedSlots` reference inside a
    /// template interpolation or directive value that isn't shadowed by a
    /// local declaration (a `v-for` alias, a `v-slot`/`slot-scope`/`scope`
    /// destructured parameter, or a function parameter within the same
    /// expression) — is implemented here. Upstream's autofix
    /// (`$scopedSlots` → `$slots`) also isn't reproduced: this fork's Vue
    /// template pass doesn't support fixes yet (see `crate::vue_template`'s
    /// module doc).
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="$scopedSlots.default" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="$slots.default" />
    /// </template>
    /// ```
    NoDeprecatedDollarScopedslotsApi,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow using deprecated `$scopedSlots` (in Vue.js 3.0.0+).",
);

impl Rule for NoDeprecatedDollarScopedslotsApi {}

impl VueTemplateRule for NoDeprecatedDollarScopedslotsApi {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_nodes_with_scope(nodes, &FxHashSet::default(), &mut |node, scope| match node {
            Node::Element(element) => {
                for attribute in &element.attributes {
                    let Some((text, span)) = directive_expression(attribute) else { continue };
                    check_expression(text, span, scope, ctx);
                }
            }
            Node::Interpolation(interpolation) => {
                check_expression(
                    interpolation.expression,
                    interpolation.expression_span,
                    scope,
                    ctx,
                );
            }
            _ => {}
        });
    }
}

fn check_expression(
    text: &str,
    container_span: Span,
    scope: &FxHashSet<String>,
    ctx: &mut VueTemplateContext<'_>,
) {
    // `$scopedSlots` shadowed by a `v-for` alias in scope: every bare
    // `$scopedSlots` reference within this subtree's expressions is that
    // alias, not the deprecated global.
    if scope.contains("$scopedSlots") {
        return;
    }
    for relative_span in free_reference_spans(text, "$scopedSlots") {
        ctx.diagnostic(deprecated_scoped_slots_diagnostic(Span::new(
            container_span.start + relative_span.start,
            container_span.start + relative_span.end,
        )));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedDollarScopedslotsApi;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-bind="$attrs"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Plain (non-directive) attribute text is never an expression.
            (
                r#"<template><div foo="$scopedSlots"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `$scopedSlots` shadowed by a local function parameter within
            // the same expression.
            (
                r#"<template><div v-on="() => { function click($scopedSlots) { fn(foo.$scopedSlots); fn($scopedSlots); } }"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `$scopedSlots` shadowed as a `v-for` alias — both its own
            // declaration and every nested reference to it.
            (
                r#"<template><div v-for="$scopedSlots in list"><div v-on="$scopedSlots" /></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `$scopedSlots` shadowed by a `v-slot` destructured parameter
            // (longhand argument, shorthand `#`, deprecated `slot-scope`,
            // and `<template>`-only deprecated `scope`), including a rest
            // element and usage several elements deep.
            (
                r#"<template><template v-slot:default="{ $scopedSlots }"><div v-on="$scopedSlots" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template #default="{ $scopedSlots }"><div><span>{{ $scopedSlots }}</span></div></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div slot-scope="{ $scopedSlots }"><span v-on="$scopedSlots" /></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template scope="{ $scopedSlots }"><span v-on="$scopedSlots" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-slot="{ ...$scopedSlots }"><div v-on="$scopedSlots" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r#"<template><div v-if="$scopedSlots.default"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="slot in $scopedSlots"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo="$scopedSlots"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ $scopedSlots.foo() }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Every distinct reference within one expression is its own
            // diagnostic.
            (
                r#"<template><div v-on="fn($scopedSlots, $scopedSlots)"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Aliased destructuring binds only the local name, not the
            // source key — a genuine `$scopedSlots` reference nearby still
            // reports.
            (
                r#"<template><template v-slot="{ $scopedSlots: renamed }">{{ $scopedSlots }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // The deprecated `scope` attribute only establishes slot scope
            // on `<template>` — on any other element it's inert markup.
            (
                r#"<template><div scope="{ $scopedSlots }">{{ $scopedSlots }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedDollarScopedslotsApi::NAME,
            NoDeprecatedDollarScopedslotsApi::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
