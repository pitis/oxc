use oxc_ast::{
    AstKind,
    ast::{Expression, ObjectExpression, ObjectPropertyKind},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AstNode,
    context::LintContext,
    frameworks::FrameworkOptions,
    rule::{DefaultRuleConfig, Rule},
    utils::{is_vue_component_options_object, static_key_name},
};

fn order_diagnostic(span: Span, name: &str, above: &str, line: usize) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The \"{name}\" property should be above the \"{above}\" property on line {line}."
    ))
    .with_help("Keep component options in the conventional order.")
    .with_label(span)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct OrderInComponentsConfig {
    /// The option names in the order they should appear. An array element
    /// groups several names at one position. `LIFECYCLE_HOOKS` and
    /// `ROUTER_GUARDS` expand to their respective sets.
    pub order: Option<Vec<OrderEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OrderEntry {
    One(String),
    Several(Vec<String>),
}

// Boxed: the `order` option is larger than `RuleEnum`'s 16-byte budget.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrderInComponents(Box<OrderInComponentsConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces the conventional order of options in a component object —
    /// `name` before `components` before `props` before `data` before
    /// `computed` before `methods`, and so on.
    ///
    /// ### Why is this bad?
    ///
    /// Purely a consistency rule, and the payoff is navigational: in a large
    /// component you learn where `props` is relative to `data` once, and every
    /// other component in the codebase reads the same way. The default order
    /// runs roughly outside-in — what the component *is*, then what it takes,
    /// then what it holds, then what it does.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// export default {
    ///   data() { return {} },
    ///   name: 'MyComponent',
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// export default {
    ///   name: 'MyComponent',
    ///   data() { return {} },
    /// }
    /// ```
    ///
    /// ### Options
    ///
    /// #### order
    ///
    /// The option names in order; an array element groups names at one
    /// position. Anything not named is not checked. Defaults to
    /// eslint-plugin-vue's own list.
    ///
    /// ```json
    /// { "vue/order-in-components": ["error", { "order": ["name", "data", "methods"] }] }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream offers a fix (or a suggestion, when moving the property might
    /// reorder a side effect); neither is offered here. Moving a property means
    /// relocating its comments and trailing comma, which is a fix worth doing
    /// properly or not at all.
    OrderInComponents,
    vue,
    style,
    config = OrderInComponents,
    version = "1.80.0",
    short_description = "Enforce order of properties in components.",
);

impl Rule for OrderInComponents {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectExpression(object) if is_vue_component_options_object(node, ctx) => {
                self.check_order(object, ctx);
            }
            // `defineOptions({ … })` in a `<script setup>` block.
            AstKind::CallExpression(call)
                if ctx.frameworks_options() == FrameworkOptions::VueSetup
                    && matches!(call.callee, Expression::Identifier(_))
                    && call.callee_name() == Some("defineOptions") =>
            {
                if let Some(Expression::ObjectExpression(object)) =
                    call.arguments.first().and_then(|argument| argument.as_expression())
                {
                    self.check_order(object, ctx);
                }
            }
            _ => {}
        }
    }
}

/// Upstream's `groups`.
const LIFECYCLE_HOOKS: [&str; 15] = [
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "activated",
    "deactivated",
    "beforeUnmount",
    "unmounted",
    "beforeDestroy",
    "destroyed",
    "renderTracked",
    "renderTriggered",
    "errorCaptured",
];
const ROUTER_GUARDS: [&str; 3] = ["beforeRouteEnter", "beforeRouteUpdate", "beforeRouteLeave"];

/// Upstream's `defaultOrder`, with each entry's members sharing a position.
const DEFAULT_ORDER: &[&[&str]] = &[
    &["el"],
    &["name"],
    &["key"],
    &["parent"],
    &["functional"],
    &["delimiters", "comments"],
    &["components", "directives", "filters"],
    &["extends"],
    &["mixins"],
    &["provide", "inject"],
    &["ROUTER_GUARDS"],
    &["layout"],
    &["middleware"],
    &["validate"],
    &["scrollToTop"],
    &["transition"],
    &["loading"],
    &["inheritAttrs"],
    &["model"],
    &["props", "propsData"],
    &["emits"],
    &["slots"],
    &["expose"],
    &["setup"],
    &["asyncData"],
    &["data"],
    &["fetch"],
    &["head"],
    &["computed"],
    &["watch"],
    &["watchQuery"],
    &["LIFECYCLE_HOOKS"],
    &["methods"],
    &["template", "render"],
    &["renderError"],
];

impl OrderInComponents {
    /// `option name -> position`, with the two group aliases expanded.
    fn order_map(&self) -> FxHashMap<String, usize> {
        let mut map = FxHashMap::default();
        let insert = |name: &str, index: usize, map: &mut FxHashMap<String, usize>| match name {
            "LIFECYCLE_HOOKS" => {
                for hook in LIFECYCLE_HOOKS {
                    map.insert(hook.to_string(), index);
                }
            }
            "ROUTER_GUARDS" => {
                for guard in ROUTER_GUARDS {
                    map.insert(guard.to_string(), index);
                }
            }
            _ => {
                map.insert(name.to_string(), index);
            }
        };

        match &self.0.order {
            Some(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    match entry {
                        OrderEntry::One(name) => insert(name, index, &mut map),
                        OrderEntry::Several(names) => {
                            for name in names {
                                insert(name, index, &mut map);
                            }
                        }
                    }
                }
            }
            None => {
                for (index, group) in DEFAULT_ORDER.iter().enumerate() {
                    for name in *group {
                        insert(name, index, &mut map);
                    }
                }
            }
        }
        map
    }

    fn check_order<'a>(&self, object: &ObjectExpression<'a>, ctx: &LintContext<'a>) {
        let order = self.order_map();
        // Name and position of each property, in source order. A spread has
        // neither and is skipped, as upstream's `isProperty` filter does.
        let properties: Vec<(String, usize, Span)> = object
            .properties
            .iter()
            .filter_map(|property| {
                let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
                let name = static_key_name(&property.key)?;
                let position = *order.get(name.as_ref())?;
                Some((name.into_owned(), position, property.span))
            })
            .collect();

        for (index, (name, position, span)) in properties.iter().enumerate() {
            // The earlier property that is furthest out of place decides the
            // message, matching upstream's sort-then-take-first.
            let Some((above, _, above_span)) = properties[..index]
                .iter()
                .filter(|(_, earlier, _)| earlier > position)
                .min_by_key(|(_, earlier, _)| *earlier)
            else {
                continue;
            };
            let line = ctx.source_text()[..above_span.start as usize].lines().count().max(1);
            ctx.diagnostic(order_diagnostic(*span, name, above, line));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::OrderInComponents;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            (
                "<script>export default { name: 'A', components: {}, props: {}, data() { return {} }, computed: {}, methods: {} }</script>",
                None,
                None,
                vue(),
            ),
            // Unknown options are not ordered.
            ("<script>export default { foo: 1, name: 'A', bar: 2 }</script>", None, None, vue()),
            // Lifecycle hooks share one position, so their relative order is free.
            ("<script>export default { mounted() {}, created() {} }</script>", None, None, vue()),
            // A custom order.
            (
                "<script>export default { methods: {}, name: 'A' }</script>",
                Some(json!([{ "order": ["methods", "name"] }])),
                None,
                vue(),
            ),
            // `defineOptions` in the right order.
            (
                "<script setup>defineOptions({ name: 'A', inheritAttrs: false })</script>",
                None,
                None,
                vue(),
            ),
        ];

        let fail = vec![
            (
                "<script>export default { data() { return {} }, name: 'A' }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>export default { methods: {}, computed: {}, props: {} }</script>",
                None,
                None,
                vue(),
            ),
            // A router guard sits above `data`.
            (
                "<script>export default { data() { return {} }, beforeRouteEnter() {} }</script>",
                None,
                None,
                vue(),
            ),
            // Custom order violated.
            (
                "<script>export default { name: 'A', methods: {} }</script>",
                Some(json!([{ "order": ["methods", "name"] }])),
                None,
                vue(),
            ),
            (
                "<script setup>defineOptions({ inheritAttrs: false, name: 'A' })</script>",
                None,
                None,
                vue(),
            ),
        ];

        Tester::new(OrderInComponents::NAME, OrderInComponents::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
