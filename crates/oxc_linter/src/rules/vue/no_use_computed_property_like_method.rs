use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashSet;
use vue_sfc_parser::ast::Node;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        computed_getter_may_return_function, directive_expression, find_property, free_call_spans,
        is_vue_component_options_object, static_key_name, walk_nodes_with_scope,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_diagnostic(span: Span, prefix: &str, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Use {prefix}{name} instead of {prefix}{name}()."))
        .with_help("A computed property is a value, not a method — read it without calling it.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoUseComputedPropertyLikeMethod;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows calling a `computed` property as though it were a method,
    /// when its getter does not return a function.
    ///
    /// ### Why is this bad?
    ///
    /// A computed property *is* its value: `this.fullName` is the string, so
    /// `this.fullName()` tries to call a string and throws
    /// `... is not a function` at runtime. The mistake reads naturally, because
    /// `methods` entries next to it in the same object are called.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// export default {
    ///   computed: { total() { return 1 } },
    ///   methods: { show() { return this.total() } },
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// export default {
    ///   computed: {
    ///     total() { return 1 },
    ///     // A computed that really does return a function may be called.
    ///     formatter() { return (x) => x },
    ///   },
    ///   methods: { show() { return this.total + this.formatter('x') } },
    /// }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Whether a getter can return a function is decided from the getter's own
    /// `return` statements, treating anything opaque — a call, an identifier, a
    /// conditional — as "might be a function" and therefore not reported.
    /// Upstream additionally follows returned identifiers back to the component
    /// members they name, so it reports some cases this leaves alone. The
    /// difference is under-reporting only: neither reports a getter it cannot
    /// prove returns a non-function.
    NoUseComputedPropertyLikeMethod,
    vue,
    correctness,
    version = "1.80.0",
    short_description = "Disallow use of computed property like method.",
);

impl Rule for NoUseComputedPropertyLikeMethod {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // `this.someComputed()` inside the component.
        let AstKind::ThisExpression(_) = node.kind() else { return };
        let member = ctx.nodes().parent_node(node.id());
        let AstKind::StaticMemberExpression(expression) = member.kind() else { return };
        if expression.object.span() != node.span() {
            return;
        }
        let call = ctx.nodes().parent_node(member.id());
        let AstKind::CallExpression(call_expression) = call.kind() else { return };
        if call_expression.callee.span() != member.span() {
            return;
        }

        let name = expression.property.name.as_str();
        if !non_function_computed_names(member, ctx).contains(name) {
            return;
        }
        ctx.diagnostic(unexpected_diagnostic(call_expression.span, "this.", name));
    }
}

impl VueTemplateRule for NoUseComputedPropertyLikeMethod {
    fn needs_script_computed(&self) -> bool {
        true
    }

    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let computed = ctx.script_computed().clone();
        if computed.is_empty() {
            return;
        }

        let mut reports = Vec::new();
        walk_nodes_with_scope(nodes, &FxHashSet::default(), &mut |node, scope| {
            let (text, base) = match node {
                Node::Interpolation(interpolation) => {
                    (interpolation.expression, interpolation.expression_span.start)
                }
                Node::Element(element) => {
                    for attribute in &element.attributes {
                        let Some((text, span)) = directive_expression(attribute) else { continue };
                        collect(text, span.start, scope, &computed, &mut reports);
                    }
                    return;
                }
                _ => return,
            };
            collect(text, base, scope, &computed, &mut reports);
        });

        reports.sort_unstable_by_key(|(span, _): &(Span, String)| span.start);
        for (span, name) in reports {
            ctx.diagnostic(unexpected_diagnostic(span, "", &name));
        }
    }
}

fn collect(
    text: &str,
    base: u32,
    scope: &FxHashSet<String>,
    computed: &FxHashSet<String>,
    out: &mut Vec<(Span, String)>,
) {
    for name in computed {
        if scope.contains(name) {
            continue;
        }
        for span in free_call_spans(text, name) {
            out.push((Span::new(base + span.start, base + span.end), name.clone()));
        }
    }
}

/// The component nearest to `node`, reduced to the `computed` names whose
/// getter cannot return a function.
fn non_function_computed_names<'a>(node: &AstNode<'a>, ctx: &LintContext<'a>) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let Some(object) = ctx.nodes().ancestors(node.id()).find_map(|ancestor| {
        let AstKind::ObjectExpression(object) = ancestor.kind() else { return None };
        is_vue_component_options_object(ancestor, ctx).then_some(object)
    }) else {
        return names;
    };
    let Some(computed) = find_property(object, "computed") else { return names };
    let Expression::ObjectExpression(computed) = &computed.value else { return names };
    for property in &computed.properties {
        let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else { continue };
        let Some(name) = static_key_name(&property.key) else { continue };
        if !computed_getter_may_return_function(&property.value) {
            names.insert(name.into_owned());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUseComputedPropertyLikeMethod;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            // Read, not called.
            (
                "<script>export default { computed: { total() { return 1 } }, methods: { show() { return this.total } } }</script>",
                None,
                None,
                vue(),
            ),
            // A computed that really returns a function may be called.
            (
                "<script>export default { computed: { fmt() { return (x) => x } }, methods: { show() { return this.fmt('x') } } }</script>",
                None,
                None,
                vue(),
            ),
            // A method may be called.
            (
                "<script>export default { methods: { total() { return 1 }, show() { return this.total() } } }</script>",
                None,
                None,
                vue(),
            ),
            // An opaque getter is never reported.
            (
                "<script>export default { computed: { total() { return makeIt() } }, methods: { show() { return this.total() } } }</script>",
                None,
                None,
                vue(),
            ),
            // Template: read, not called.
            (
                "<template><div>{{ total }}</div></template><script>export default { computed: { total() { return 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            // A `v-for` alias shadows the computed name.
            (
                "<template><div v-for=\"total in xs\">{{ total() }}</div></template><script>export default { computed: { total() { return 1 } } }</script>",
                None,
                None,
                vue(),
            ),
        ];

        let fail = vec![
            (
                "<script>export default { computed: { total() { return 1 } }, methods: { show() { return this.total() } } }</script>",
                None,
                None,
                vue(),
            ),
            // Getter with an object literal.
            (
                "<script>export default { computed: { conf() { return {} } }, methods: { show() { return this.conf() } } }</script>",
                None,
                None,
                vue(),
            ),
            // The `get()` form.
            (
                "<script>export default { computed: { total: { get() { return 1 } } }, methods: { show() { return this.total() } } }</script>",
                None,
                None,
                vue(),
            ),
            // Template call.
            (
                "<template><div>{{ total() }}</div></template><script>export default { computed: { total() { return 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            // Template call in a directive value.
            (
                "<template><div :title=\"total()\" /></template><script>export default { computed: { total() { return 1 } } }</script>",
                None,
                None,
                vue(),
            ),
        ];

        Tester::new(
            NoUseComputedPropertyLikeMethod::NAME,
            NoUseComputedPropertyLikeMethod::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
