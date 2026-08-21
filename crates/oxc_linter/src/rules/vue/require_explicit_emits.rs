use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::Node;

use crate::{
    AstNode,
    context::LintContext,
    frameworks::FrameworkOptions,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        VueScriptProps, directive_expression, find_property, is_vue_component_options_object,
        literal_element_name, static_key_name, vue_casing::capitalize, walk_nodes_with_scope,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn missing_diagnostic(span: Span, name: &str, from_define: bool) -> OxcDiagnostic {
    let kind = if from_define { "`defineEmits`" } else { "`emits` option" };
    OxcDiagnostic::warn(format!(
        "The \"{name}\" event has been triggered but not declared on {kind}."
    ))
    .with_help(format!("Declare \"{name}\" so the component's events are part of its contract."))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct RequireExplicitEmits {
    /// Also accept an event that is declared as an `onXxx` prop rather than an
    /// emit, which is how a component takes a callback prop instead.
    pub allow_props: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires every event a component emits to be declared, in the `emits`
    /// option or via `defineEmits`.
    ///
    /// ### Why is this bad?
    ///
    /// The declaration is the component's outward contract: it is what the
    /// reader, the editor and the type checker consult to find out what the
    /// component can emit. Without it an event is discoverable only by
    /// grepping for `$emit`.
    ///
    /// It also changes behaviour. An undeclared event still lands in
    /// `$attrs`, so it is applied to the root element as a native listener as
    /// well as being emitted — which means a `click` you forgot to declare
    /// fires twice.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <script>
    /// export default {
    ///   methods: { go() { this.$emit('change', 1) } },
    /// }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <script>
    /// export default {
    ///   emits: ['change'],
    ///   methods: { go() { this.$emit('change', 1) } },
    /// }
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// #### allowProps
    ///
    /// `{ type: boolean, default: false }` — also accept an event backed by an
    /// `onXxx` prop, for a component that takes a callback prop instead.
    ///
    /// ```json
    /// { "vue/require-explicit-emits": ["error", { "allowProps": true }] }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream attaches suggestions that write the missing name into the
    /// declaration (creating the `emits` option or `defineEmits` call when
    /// there is none). Suggestions are not offered here.
    ///
    /// An `emits` declaration this linter cannot read statically — a spread, a
    /// computed key, an identifier referring to a list defined elsewhere —
    /// suppresses the rule for that component rather than reporting every
    /// event, matching upstream's treatment of an emit whose name is unknown.
    RequireExplicitEmits,
    vue,
    style,
    config = RequireExplicitEmits,
    version = "1.80.0",
    short_description = "Require `emits` option with event names that it triggers.",
);

impl Rule for RequireExplicitEmits {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else { return };
        let Some(name_argument) =
            call.arguments.first().and_then(|argument| argument.as_expression())
        else {
            return;
        };
        // Upstream's `getNameParamNode`: only a statically known event name
        // can be checked.
        let Some(name) = literal_element_name(name_argument.without_parentheses()) else { return };

        if !is_emit_call(call, ctx) {
            return;
        }
        let Some(declared) = enclosing_declaration(node, ctx) else { return };
        if declared.accepts(&name, self.allow_props) {
            return;
        }
        ctx.diagnostic(missing_diagnostic(name_argument.span(), &name, declared.from_define));
    }
}

impl VueTemplateRule for RequireExplicitEmits {
    fn needs_script_props(&self) -> bool {
        true
    }

    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let props = ctx.script_props().clone();
        let declared = Declared {
            emits: props.emits.iter().cloned().collect(),
            unknown: props.emits_unknown,
            props: props.names.clone(),
            from_define: props.emits_from_define,
        };
        let binding = props.emit_name;
        if declared.unknown {
            return;
        }

        let mut reports = Vec::new();
        walk_nodes_with_scope(nodes, &FxHashSet::default(), &mut |node, _scope| {
            let (text, base) = match node {
                Node::Interpolation(interpolation) => {
                    (interpolation.expression, interpolation.expression_span.start)
                }
                Node::Element(element) => {
                    for attribute in &element.attributes {
                        let Some((text, span)) = directive_expression(attribute) else { continue };
                        collect_template_emits(
                            text,
                            span.start,
                            binding.as_deref(),
                            &declared,
                            self,
                            &mut reports,
                        );
                    }
                    return;
                }
                _ => return,
            };
            collect_template_emits(text, base, binding.as_deref(), &declared, self, &mut reports);
        });

        reports.sort_unstable_by_key(|(span, _): &(Span, String)| span.start);
        for (span, name) in reports {
            ctx.diagnostic(missing_diagnostic(span, &name, declared.from_define));
        }
    }
}

/// What the component declares, and whether a given event is covered by it.
struct Declared {
    emits: FxHashSet<String>,
    unknown: bool,
    props: FxHashSet<String>,
    from_define: bool,
}

impl Declared {
    fn accepts(&self, name: &str, allow_props: bool) -> bool {
        if self.unknown || self.emits.contains(name) {
            return true;
        }
        // `allowProps`: an `onFoo` prop is how a component takes the same
        // thing as a callback instead of an event.
        allow_props && self.props.contains(&format!("on{}", capitalize(name)))
    }
}

/// `$emit('x')` written in a template expression.
fn collect_template_emits(
    text: &str,
    base: u32,
    binding: Option<&str>,
    declared: &Declared,
    rule: &RequireExplicitEmits,
    out: &mut Vec<(Span, String)>,
) {
    // Cheap reject before parsing: an emit is spelled either `$emit` or, in a
    // `<script setup>` block, the name `defineEmits()` was bound to.
    if !text.contains("$emit") && !binding.is_some_and(|binding| text.contains(binding)) {
        return;
    }
    for (span, name) in crate::utils::template_emit_calls(text, binding) {
        if declared.accepts(&name, rule.allow_props) {
            continue;
        }
        out.push((Span::new(base + span.start, base + span.end), name));
    }
}

/// Whether this call is an emit: `this.$emit(…)`, `ctx.emit(…)`, or the
/// binding a `defineEmits()` / `setup(props, { emit })` produced.
fn is_emit_call<'a>(call: &oxc_ast::ast::CallExpression<'a>, ctx: &LintContext<'a>) -> bool {
    match call.callee.get_inner_expression() {
        Expression::StaticMemberExpression(member) => {
            matches!(member.property.name.as_str(), "emit" | "$emit")
        }
        Expression::Identifier(identifier) => {
            // A bare `emit(...)` counts when it is the `defineEmits` result or
            // a destructured setup context.
            let Some(symbol_id) =
                ctx.scoping().get_reference(identifier.reference_id()).symbol_id()
            else {
                return false;
            };
            let declaration = ctx.nodes().get_node(ctx.scoping().symbol_declaration(symbol_id));
            match declaration.kind() {
                AstKind::VariableDeclarator(declarator) => {
                    declarator.init.as_ref().is_some_and(|init| {
                        matches!(init.get_inner_expression(), Expression::CallExpression(call)
                            if call.callee_name() == Some("defineEmits"))
                    })
                }
                // `setup(props, { emit })`.
                _ => identifier.name == "emit",
            }
        }
        _ => false,
    }
}

/// The component declaration in scope at `node`: `defineEmits` in a
/// `<script setup>` block, or the nearest component options object.
fn enclosing_declaration<'a>(node: &AstNode<'a>, ctx: &LintContext<'a>) -> Option<Declared> {
    if ctx.frameworks_options() == FrameworkOptions::VueSetup {
        let mut declared = Declared {
            emits: FxHashSet::default(),
            unknown: false,
            props: FxHashSet::default(),
            from_define: true,
        };
        let mut found = false;
        for candidate in ctx.nodes() {
            let AstKind::CallExpression(call) = candidate.kind() else { continue };
            if !matches!(call.callee, Expression::Identifier(_)) {
                continue;
            }
            match call.callee_name() {
                Some("defineEmits") => {
                    found = true;
                    let mut props = VueScriptProps::default();
                    crate::utils::vue_define_emit_names(call, &mut props);
                    declared.emits.extend(props.emits);
                    declared.unknown |= props.emits_unknown;
                }
                Some("defineProps") => {
                    declared
                        .props
                        .extend(crate::utils::vue_define_props_names(call, ctx.semantic()));
                }
                _ => {}
            }
        }
        // A `<script setup>` block with no `defineEmits` still has an empty
        // declaration, which is what makes every emit reportable.
        let _ = found;
        return Some(declared);
    }

    let object = ctx.nodes().ancestors(node.id()).find_map(|ancestor| {
        let AstKind::ObjectExpression(object) = ancestor.kind() else { return None };
        is_vue_component_options_object(ancestor, ctx).then_some(object)
    })?;

    let mut declared = Declared {
        emits: FxHashSet::default(),
        unknown: false,
        props: FxHashSet::default(),
        from_define: false,
    };
    if let Some(property) = find_property(object, "emits") {
        let mut props = VueScriptProps::default();
        crate::utils::vue_collect_emit_names_from(&property.value, &mut props);
        declared.emits.extend(props.emits);
        declared.unknown |= props.emits_unknown;
    }
    if let Some(property) = find_property(object, "props")
        && let Expression::ObjectExpression(props) = &property.value
    {
        for entry in &props.properties {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(entry) = entry else { continue };
            if let Some(name) = static_key_name(&entry.key) {
                declared.props.insert(name.into_owned());
            }
        }
    }
    Some(declared)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::RequireExplicitEmits;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            (
                "<script>export default { emits: ['change'], methods: { go() { this.$emit('change') } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>export default { emits: { change: null }, methods: { go() { this.$emit('change') } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script setup>const emit = defineEmits(['change'])\nemit('change')</script>",
                None,
                None,
                vue(),
            ),
            // A dynamic name cannot be checked.
            (
                "<script>export default { methods: { go(n) { this.$emit(n) } } }</script>",
                None,
                None,
                vue(),
            ),
            // An `emits` this linter cannot read suppresses the component.
            (
                "<script>export default { emits: SHARED, methods: { go() { this.$emit('change') } } }</script>",
                None,
                None,
                vue(),
            ),
            // allowProps.
            (
                "<script>export default { props: { onChange: Function }, methods: { go() { this.$emit('change') } } }</script>",
                Some(json!([{ "allowProps": true }])),
                None,
                vue(),
            ),
            // Template, declared.
            (
                "<template><button @click=\"$emit('change')\" /></template><script>export default { emits: ['change'] }</script>",
                None,
                None,
                vue(),
            ),
        ];

        let fail = vec![
            (
                "<script>export default { methods: { go() { this.$emit('change') } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>export default { emits: ['other'], methods: { go() { this.$emit('change') } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script setup>const emit = defineEmits(['other'])\nemit('change')</script>",
                None,
                None,
                vue(),
            ),
            // Template emit of an undeclared event.
            (
                "<template><button @click=\"$emit('change')\" /></template><script>export default { emits: ['other'] }</script>",
                None,
                None,
                vue(),
            ),
            // A `<script setup>` block exposes the `defineEmits` binding to
            // the template, so calling it there is an emit too — the template
            // half cannot look only for `$emit`. Found by differential testing.
            (
                "<template><button @click=\"emit('change')\" /></template><script setup>const emit = defineEmits(['other'])</script>",
                None,
                None,
                vue(),
            ),
            // allowProps off (the default) does not accept the prop.
            (
                "<script>export default { props: { onChange: Function }, methods: { go() { this.$emit('change') } } }</script>",
                None,
                None,
                vue(),
            ),
        ];

        Tester::new(RequireExplicitEmits::NAME, RequireExplicitEmits::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
