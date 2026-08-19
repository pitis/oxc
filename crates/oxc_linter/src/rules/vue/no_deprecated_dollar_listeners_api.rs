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

fn deprecated_listeners_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `$listeners` is deprecated.")
        .with_help(
            "Vue 3 merges listeners into `$attrs`; use `v-bind=\"$attrs\"` instead of `$listeners`.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedDollarListenersApi;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows using the deprecated `$listeners` object, removed in
    /// Vue 3.0.0+ (its contents were merged into `$attrs`).
    ///
    /// ### Why is this bad?
    ///
    /// `$listeners` no longer exists on the Vue 3 component instance;
    /// referencing it evaluates to `undefined`, silently breaking whatever
    /// depended on it (typically `v-on="$listeners"` forwarding).
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Template side only: upstream also flags `this.$listeners` in
    /// `<script>` (via a `MemberExpression` visitor scoped to `this`
    /// inside the component definition). That half needs script-level
    /// semantic analysis of the component definition and is out of scope
    /// for this template-only rule; only the `VExpressionContainer` half —
    /// any `$listeners` reference inside a template interpolation or
    /// directive value that isn't shadowed by a local declaration (a
    /// `v-for` alias, a `v-slot`/`slot-scope`/`scope` destructured
    /// parameter, or a function parameter within the same expression) — is
    /// implemented here.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-on="$listeners" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-bind="$attrs" />
    /// </template>
    /// ```
    NoDeprecatedDollarListenersApi,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow using deprecated `$listeners` (in Vue.js 3.0.0+).",
);

impl Rule for NoDeprecatedDollarListenersApi {}

impl VueTemplateRule for NoDeprecatedDollarListenersApi {
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
    // `$listeners` shadowed by a `v-for` alias in scope: every bare
    // `$listeners` reference within this subtree's expressions is that
    // alias, not the deprecated global.
    if scope.contains("$listeners") {
        return;
    }
    for relative_span in free_reference_spans(text, "$listeners") {
        ctx.diagnostic(deprecated_listeners_diagnostic(Span::new(
            container_span.start + relative_span.start,
            container_span.start + relative_span.end,
        )));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedDollarListenersApi;
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
                r#"<template><div foo="$listeners"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `$listeners` shadowed by a local function parameter within
            // the same expression.
            (
                r#"<template><div v-on="() => { function click($listeners) { fn(foo.$listeners); fn($listeners); } }"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `$listeners` shadowed as a `v-for` alias — both its own
            // declaration and every nested reference to it.
            (
                r#"<template><div v-for="$listeners in list"><div v-on="$listeners" /></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `$listeners` shadowed by a `v-slot` destructured parameter.
            // Reviewer-reported false positive, verified against real
            // eslint-plugin-vue 10.9.1: reports nothing.
            (
                r#"<template><template v-slot:default="{ $listeners }"><div v-on="$listeners" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shorthand `#` form; usage several elements deep (scope
            // persists across intermediate elements with no scope of their
            // own).
            (
                r#"<template><template #default="{ $listeners }"><div><span>{{ $listeners }}</span></div></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Deprecated `slot-scope` attribute (any element), and `scope`
            // attribute (only recognized on `<template>`).
            (
                r#"<template><div slot-scope="{ $listeners }"><span v-on="$listeners" /></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template scope="{ $listeners }"><span v-on="$listeners" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A rest element binds its own name into scope.
            (
                r#"<template><template v-slot="{ ...$listeners }"><div v-on="$listeners" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r#"<template><div v-on="$listeners"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="listener in $listeners"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo="$listeners"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ $listeners.foo }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Every distinct reference within one expression is its own
            // diagnostic.
            (
                r#"<template><div v-on="fn($listeners, $listeners)"/></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Aliased destructuring (`{ $listeners: renamed }`) binds only
            // the local name `renamed` into scope, not the source key
            // `$listeners` — so a genuine `$listeners` reference nearby
            // still reports.
            (
                r#"<template><template v-slot="{ $listeners: renamed }">{{ $listeners }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // The deprecated `scope` attribute only establishes slot scope
            // on `<template>` — on any other element it's inert markup, so
            // `$listeners` here is still a genuine, reportable reference.
            (
                r#"<template><div scope="{ $listeners }">{{ $listeners }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedDollarListenersApi::NAME,
            NoDeprecatedDollarListenersApi::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
