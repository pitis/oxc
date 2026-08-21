use lazy_regex::Regex;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::{Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        ScopeBindingKind, directive_expression, element_own_scope_bindings, free_reference_spans,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unused_variable_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{name}' is defined but never used."))
        .with_help("Remove the variable, or rename it to something the `ignorePattern` allows.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoUnusedVarsConfig {
    /// A regular expression; a variable whose name matches is never reported.
    /// `"^_"` is the conventional value.
    pub ignore_pattern: Option<String>,
}

// Boxed: the option is a `String`, past `RuleEnum`'s 16-byte budget.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NoUnusedVars(Box<NoUnusedVarsConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports a variable declared by a `v-for` or a slot-scope attribute that
    /// the template never uses.
    ///
    /// ### Why is this bad?
    ///
    /// An unused alias is usually a leftover from an edit, and it reads as if
    /// the loop body depended on it. It also shadows anything of the same name
    /// from the surrounding scope, so an unused `item` can silently hide the
    /// `item` a nested expression meant to reach.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items">Nothing about the item</div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items">{{ item.name }}</div>
    ///   <div v-for="(value, key) in map">{{ key }}</div>
    /// </template>
    /// ```
    ///
    /// Note the second line: `value` is unused, but removing it would change
    /// what `key` means, so it is not reported.
    ///
    /// ### Options
    ///
    /// #### ignorePattern
    ///
    /// `{ type: string }` — a variable whose name matches is never reported.
    ///
    /// ```json
    /// { "vue/no-unused-vars": ["error", { "ignorePattern": "^_" }] }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream offers a suggestion (not a fix) renaming the variable to
    /// `_name` when `ignorePattern` is exactly `"^_"`. Suggestions are not
    /// available to the `<template>` pass, so none is offered.
    NoUnusedVars,
    vue,
    correctness,
    config = NoUnusedVars,
    version = "1.80.0",
    short_description = "Disallow unused variable definitions of v-for directives or scope attributes.",
);

impl Rule for NoUnusedVars {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoUnusedVars {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let ignore = self.0.ignore_pattern.as_ref().and_then(|pattern| Regex::new(pattern).ok());
        let mut reports = Vec::new();
        check_nodes(nodes, ignore.as_ref(), &mut reports);
        reports.sort_unstable_by_key(|(span, _): &(Span, String)| span.start);
        for (span, name) in reports {
            ctx.diagnostic(unused_variable_diagnostic(span, &name));
        }
    }
}

fn check_nodes(nodes: &[Node<'_>], ignore: Option<&Regex>, reports: &mut Vec<(Span, String)>) {
    for node in nodes {
        let Node::Element(element) = node else { continue };
        let bindings = element_own_scope_bindings(element);
        for kind in [ScopeBindingKind::VFor, ScopeBindingKind::Scope] {
            let group: Vec<_> = bindings.iter().filter(|binding| binding.kind == kind).collect();
            // Walk the group backwards: once a later variable is used, an
            // earlier positional one cannot be removed without changing what
            // the later one means, so upstream stops reporting those.
            let mut has_after_used = false;
            for binding in group.iter().rev() {
                if is_used(element, &binding.name) {
                    has_after_used = true;
                    continue;
                }
                if ignore.is_some_and(|ignore| ignore.is_match(&binding.name)) {
                    continue;
                }
                if has_after_used && !binding.destructured {
                    continue;
                }
                reports.push((binding.span, binding.name.clone()));
            }
        }
        check_nodes(&element.children, ignore, reports);
    }
}

/// Whether `name`, as declared by `element`, is referenced anywhere it is in
/// scope: `element`'s own directive values and its subtree, stopping wherever
/// a descendant re-declares the same name.
fn is_used(element: &Element<'_>, name: &str) -> bool {
    if element
        .attributes
        .iter()
        .filter_map(directive_expression)
        .any(|(text, _)| !free_reference_spans(text, name).is_empty())
    {
        return true;
    }
    used_in_nodes(&element.children, name)
}

fn used_in_nodes(nodes: &[Node<'_>], name: &str) -> bool {
    nodes.iter().any(|node| match node {
        Node::Interpolation(interpolation) => {
            !free_reference_spans(interpolation.expression, name).is_empty()
        }
        Node::Element(element) => {
            if element
                .attributes
                .iter()
                .filter_map(directive_expression)
                .any(|(text, _)| !free_reference_spans(text, name).is_empty())
            {
                return true;
            }
            // A descendant that declares the same name shadows this one, so
            // nothing below it can be a use of *this* variable.
            if element_own_scope_bindings(element).iter().any(|binding| binding.name == name) {
                return false;
            }
            used_in_nodes(&element.children, name)
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoUnusedVars;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            ("<template><div v-for=\"i in xs\">{{ i }}</div></template>", None, None, vue()),
            // Used in an attribute of the declaring element.
            ("<template><div v-for=\"i in xs\" :key=\"i\" /></template>", None, None, vue()),
            // Used deeper in the subtree.
            (
                "<template><div v-for=\"i in xs\"><span><em>{{ i }}</em></span></div></template>",
                None,
                None,
                vue(),
            ),
            // An earlier positional alias whose later sibling is used.
            ("<template><div v-for=\"(v, k) in xs\">{{ k }}</div></template>", None, None, vue()),
            // Slot scope, used.
            ("<template><List v-slot=\"{ item }\">{{ item }}</List></template>", None, None, vue()),
            // Ignored by pattern.
            (
                "<template><div v-for=\"_i in xs\" /></template>",
                Some(json!([{ "ignorePattern": "^_" }])),
                None,
                vue(),
            ),
        ];

        let fail = vec![
            ("<template><div v-for=\"i in xs\" /></template>", None, None, vue()),
            // The later alias is the unused one, so it is reported.
            ("<template><div v-for=\"(v, k) in xs\">{{ v }}</div></template>", None, None, vue()),
            // A destructured name can be dropped on its own even when a later
            // sibling is used.
            (
                "<template><div v-for=\"({ a, b }, k) in xs\">{{ k }}</div></template>",
                None,
                None,
                vue(),
            ),
            // Slot scope, unused.
            ("<template><List v-slot=\"{ item }\">x</List></template>", None, None, vue()),
            // A descendant re-declaring the name does not use the outer one.
            (
                "<template><div v-for=\"i in xs\"><span v-for=\"i in ys\">{{ i }}</span></div></template>",
                None,
                None,
                vue(),
            ),
            // The pattern does not match this name.
            (
                "<template><div v-for=\"i in xs\" /></template>",
                Some(json!([{ "ignorePattern": "^_" }])),
                None,
                vue(),
            ),
        ];

        Tester::new(NoUnusedVars::NAME, NoUnusedVars::PLUGIN, pass, fail).test_and_snapshot();
    }
}
