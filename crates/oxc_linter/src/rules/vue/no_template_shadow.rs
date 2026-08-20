use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::element_own_scope_bindings,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn already_declared_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Variable '{name}' is already declared in the upper scope."))
        .with_help("Rename this variable so it does not shadow the outer one.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoTemplateShadowConfig {
    /// Variable names that may shadow an outer declaration without being
    /// reported. Useful for the conventional throwaway names a codebase
    /// reuses at every nesting level (`item`, `index`, …).
    allow: Vec<String>,
}

// Boxed: `allow: Vec<String>` alone is 24 bytes, which would blow `RuleEnum`'s
// 16-byte budget (see `no_v_html.rs` for the same pattern).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NoTemplateShadow(Box<NoTemplateShadowConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows a variable declared in a `<template>` — a `v-for` alias, or a
    /// `v-slot`/`slot-scope`/`scope` destructured parameter — from shadowing
    /// one already visible at that point: another template variable from an
    /// ancestor element, or a name the component's `<script>` exposes.
    ///
    /// ### Why is this bad?
    ///
    /// The inner declaration silently wins, so an expression that looks like
    /// it reads the prop, the `data` field or the outer loop's item reads the
    /// inner one instead. The reader has to walk back up the tree to find out
    /// which `item` a given `{{ item.id }}` refers to, and moving the markup
    /// changes its meaning.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items">
    ///     <div v-for="item in items"><!-- shadows the outer `item` --></div>
    ///   </div>
    /// </template>
    /// ```
    ///
    /// ```vue
    /// <template>
    ///   <div v-for="count in items" /><!-- shadows the `count` prop -->
    /// </template>
    /// <script>
    /// export default {
    ///   props: ['count'],
    /// }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items">
    ///     <div v-for="child in item.children" />
    ///   </div>
    /// </template>
    /// ```
    ///
    /// ### Options
    ///
    /// #### allow
    ///
    /// `{ type: string[], default: [] }`
    ///
    /// Names exempt from the check.
    ///
    /// ```json
    /// { "vue/no-template-shadow": ["error", { "allow": ["item"] }] }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// A name is taken to be script-declared when it is a module-scope binding
    /// of a `<script setup>` block, a `defineProps` prop, or a key of the
    /// component options object's `props`, `computed`, `data`, `asyncData`,
    /// `methods` or `setup` group — upstream's own two sources, resolved
    /// against this linter's own scope analysis rather than
    /// vue-eslint-parser's. A name that only exists after a type-level
    /// indirection upstream cannot follow either (an imported props type, a
    /// mapped type) is invisible to both.
    NoTemplateShadow,
    vue,
    style,
    config = NoTemplateShadow,
    version = "1.80.0",
    short_description = "Disallow variable declarations from shadowing variables declared in the outer scope.",
);

impl Rule for NoTemplateShadow {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoTemplateShadow {
    fn needs_script_names(&self) -> bool {
        true
    }

    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        // One flat stack instead of upstream's copy-the-parent's-list-per
        // element: an element's frame is everything pushed by its ancestors,
        // so entering is "remember the length" and leaving is "truncate back".
        let mut scope: Vec<String> = Vec::new();
        check_nodes(nodes, &mut scope, &self.0.allow, ctx);
    }
}

fn check_nodes<'a>(
    nodes: &[Node<'a>],
    scope: &mut Vec<String>,
    allow: &[String],
    ctx: &mut VueTemplateContext<'a>,
) {
    for node in nodes {
        let Node::Element(element) = node else { continue };
        let outer = scope.len();
        for binding in element_own_scope_bindings(element) {
            if allow.iter().any(|allowed| allowed == &binding.name) {
                continue;
            }
            // `scope` already carries what this element declared before this
            // binding, so an element declaring two different names that both
            // shadow is reported twice — upstream pushes into the same list as
            // it iterates and sees it the same way.
            if scope.contains(&binding.name) || ctx.script_names().contains(&binding.name) {
                ctx.diagnostic(already_declared_diagnostic(binding.span, &binding.name));
            } else {
                scope.push(binding.name);
            }
        }
        check_nodes(&element.children, scope, allow, ctx);
        scope.truncate(outer);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoTemplateShadow;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Sibling scopes don't see each other.
            (
                "<template><div v-for=\"i in 5\" /><div v-for=\"i in 5\" /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested, but different names.
            (
                "<template><div v-for=\"item in items\"><div v-for=\"child in item.children\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A slot-scope parameter that shadows nothing.
            (
                "<template><MyList v-slot=\"{ item }\"><span>{{ item }}</span></MyList></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // The name is exempted.
            (
                "<template><div v-for=\"i in 5\"><div v-for=\"i in 5\" /></div></template>",
                Some(json!([{ "allow": ["i"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A module-scope binding of a plain `<script>` is NOT visible to
            // the template, so it is not shadowed (upstream gates that half of
            // `jsVars` on the file having a `<script setup>` block).
            (
                "<template><div v-for=\"count in items\" /></template><script>const count = 1; export default {}</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // An option group the template can't reach either.
            (
                "<template><div v-for=\"count in items\" /></template><script>export default { watch: { count() {} } }</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `v-for` over a name declared in an *inner* scope is fine: the
            // outer one is gone by then.
            (
                "<template><div v-for=\"a in xs\" /><div><div v-for=\"a in xs\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // One value binding the same name twice declares nothing at all:
            // it is an early error in the grammar the value is parsed with, so
            // vue-eslint-parser gives the element no variables and this rule
            // has nothing to report. Verified against eslint-plugin-vue
            // 10.9.1, which reports none of these three.
            (
                "<template><div v-for=\"(a, a) in items\" /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<template><div v-for=\"[a, a] in items\" /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<template><MyList v-slot=\"{ a, b: a }\" /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Nested `v-for` reusing the alias.
            (
                "<template><div v-for=\"i in 5\"><div v-for=\"i in 5\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shadowing across a non-declaring element.
            (
                "<template><div v-for=\"item in items\"><span><div v-for=\"item in items\" /></span></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A destructured `v-for` alias shadows an outer one, and the
            // report is on the inner identifier, not the whole value.
            (
                "<template><div v-for=\"item in items\"><div v-for=\"{ item } in others\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A parenthesised alias list: the span must survive the `(`.
            (
                "<template><div v-for=\"value in items\"><div v-for=\"(value, key) in others\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A slot-scope parameter shadowing a `v-for` alias.
            (
                "<template><div v-for=\"item in items\"><MyList v-slot=\"{ item }\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // The deprecated `slot-scope` attribute declares scope too.
            (
                "<template><div v-for=\"item in items\"><MyList slot-scope=\"item\" /></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shadowing a `<script setup>` module binding.
            (
                "<template><div v-for=\"count in items\" /></template><script setup>const count = 1;</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shadowing a `defineProps` prop declared by type.
            (
                "<template><div v-for=\"msg in items\" /></template><script setup lang=\"ts\">defineProps<{ msg: string }>();</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shadowing an Options API prop.
            (
                "<template><div v-for=\"count in items\" /></template><script>export default { props: ['count'] }</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shadowing a `data()` key.
            (
                "<template><div v-for=\"count in items\" /></template><script>export default { data() { return { count: 1 } } }</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `allow` covers only the names it lists.
            (
                "<template><div v-for=\"i in 5\"><div v-for=\"i in 5\"><div v-for=\"j in 5\"><div v-for=\"j in 5\" /></div></div></div></template>",
                Some(json!([{ "allow": ["i"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoTemplateShadow::NAME, NoTemplateShadow::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
