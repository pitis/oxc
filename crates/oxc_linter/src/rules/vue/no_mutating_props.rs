use std::borrow::Cow;

use oxc_allocator::Allocator;
use oxc_ast::{
    AstKind,
    ast::{
        BindingPattern, CallExpression, ChainElement, Expression, ObjectExpression, UnaryOperator,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_semantic::{AstNodes, SemanticBuilder, SymbolId};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::{Attribute, Node};

use crate::{
    AstNode,
    context::LintContext,
    frameworks::FrameworkOptions,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        VueScriptProps, directive_expression, find_property, is_vue_component_options_object,
        literal_element_name, static_key_name, vue_component_prop_names, walk_nodes_with_scope,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

/// Upstream's `findMutating` array-method list, verbatim.
const MUTATING_METHODS: [&str; 9] =
    ["push", "pop", "shift", "unshift", "reverse", "splice", "sort", "copyWithin", "fill"];

fn unexpected_mutation_diagnostic(span: Span, key: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Unexpected mutation of \"{key}\" prop."))
        .with_help("Emit an event and let the parent own this value, or copy it into local state.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoMutatingProps {
    /// Report only mutation of the prop binding itself, allowing the value it
    /// points at to be mutated in place.
    shallow_only: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows a component from mutating its own props — assigning to one,
    /// incrementing one, `delete`-ing through one, calling a mutating array
    /// method on one, or binding one with `v-model`.
    ///
    /// ### Why is this bad?
    ///
    /// Props are one-way: the parent owns the value and re-renders overwrite
    /// whatever the child wrote, so the mutation is silently lost. When the
    /// prop is an object or array the write *does* stick, but it reaches into
    /// the parent's state from the outside, which is the harder version of the
    /// same bug. Vue only warns about the first form, and only at runtime.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-model="value" @click="count += 1">{{ items.push(1) }}</div>
    /// </template>
    /// <script>
    /// export default {
    ///   props: ['value', 'count', 'items'],
    ///   methods: {
    ///     reset() {
    ///       this.count = 0
    ///     },
    ///   },
    /// }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <script>
    /// export default {
    ///   props: ['count'],
    ///   data() {
    ///     return { localCount: this.count }
    ///   },
    ///   methods: {
    ///     reset() {
    ///       this.localCount = 0
    ///       this.$emit('update:count', 0)
    ///     },
    ///   },
    /// }
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// #### shallowOnly
    ///
    /// `{ type: boolean, default: false }`
    ///
    /// Report only mutation of the prop binding itself, leaving
    /// `props.items.push(1)` and `props.user.name = 'x'` alone.
    ///
    /// ```json
    /// { "vue/no-mutating-props": ["error", { "shallowOnly": true }] }
    /// ```
    NoMutatingProps,
    vue,
    correctness,
    config = NoMutatingProps,
    version = "1.80.0",
    short_description = "Disallow mutation of component props.",
);

impl Rule for NoMutatingProps {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            // `<script setup>`: the binding `defineProps()` is assigned to.
            AstKind::CallExpression(call)
                if ctx.frameworks_options() == FrameworkOptions::VueSetup
                    && matches!(call.callee, Expression::Identifier(_))
                    && call.callee_name() == Some("defineProps") =>
            {
                self.check_define_props(call, node, ctx);
            }
            // Options API: the `setup(props)` parameter.
            AstKind::ObjectExpression(object) if is_vue_component_options_object(node, ctx) => {
                self.check_setup_parameter(object, ctx);
            }
            // Options API: `this.foo`.
            AstKind::ThisExpression(_) => self.check_this_property(node, ctx),
            _ => {}
        }
    }
}

impl NoMutatingProps {
    /// Upstream's `onDefinePropsEnter`, minus the parts only the template half
    /// reads: what matters here is the binding the call's result lands in.
    fn check_define_props<'a>(
        &self,
        call: &CallExpression<'a>,
        node: &AstNode<'a>,
        ctx: &LintContext<'a>,
    ) {
        // `withDefaults(defineProps<…>(), {…})` — the binding is on the wrapper.
        let mut target = node;
        if let AstKind::CallExpression(outer) = ctx.nodes().parent_kind(node.id())
            && outer.callee_name() == Some("withDefaults")
            && outer.arguments.first().is_some_and(|first| first.span() == call.span())
        {
            target = ctx.nodes().parent_node(node.id());
        }

        let AstKind::VariableDeclarator(declarator) = ctx.nodes().parent_kind(target.id()) else {
            return;
        };
        if declarator.init.as_ref().is_none_or(|init| init.span() != target.span()) {
            return;
        }

        let mut bindings = Vec::new();
        collect_pattern_properties(&declarator.id, &[], ctx.source_text(), &mut bindings);
        for (symbol_id, path) in bindings {
            self.verify_prop_variable(symbol_id, &path, ctx);
        }
    }

    /// Upstream's `onSetupFunctionEnter`: `setup(props)` and
    /// `setup({ foo })` both hand the component its own props.
    fn check_setup_parameter<'a>(&self, object: &ObjectExpression<'a>, ctx: &LintContext<'a>) {
        let Some(setup) = find_property(object, "setup") else { return };
        let parameters = match &setup.value {
            Expression::FunctionExpression(function) => &function.params,
            Expression::ArrowFunctionExpression(arrow) => &arrow.params,
            _ => return,
        };
        let Some(first) = parameters.items.first() else { return };
        // Upstream cannot check a rest or array parameter, and neither can this.
        if matches!(first.pattern, BindingPattern::ArrayPattern(_)) {
            return;
        }
        let mut bindings = Vec::new();
        collect_pattern_properties(&first.pattern, &[], ctx.source_text(), &mut bindings);
        for (symbol_id, path) in bindings {
            self.verify_prop_variable(symbol_id, &path, ctx);
        }
    }

    /// Upstream's `'MemberExpression > :matches(Identifier, ThisExpression)'`
    /// handler, restricted to `this`: `this.someProp` inside a component.
    fn check_this_property<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let member = ctx.nodes().parent_node(node.id());
        let name = match member.kind() {
            AstKind::StaticMemberExpression(expression)
                if expression.object.span() == node.span() =>
            {
                Cow::Borrowed(expression.property.name.as_str())
            }
            AstKind::ComputedMemberExpression(expression)
                if expression.object.span() == node.span() =>
            {
                let Some(name) = literal_element_name(&expression.expression) else { return };
                name
            }
            _ => return,
        };

        let Some(props) = enclosing_component_prop_names(member, ctx) else { return };
        if !props.contains(name.as_ref()) {
            return;
        }
        let Some(mutation) = find_mutating(member, ctx.nodes(), ctx.source_text()) else {
            return;
        };
        // `this.foo` *is* the prop, not the props object, so a zero-length
        // path is already the direct mutation — upstream's `verifyMutating`
        // defaults `isRootProps` to false for exactly this call.
        if !self.is_reportable(&mutation, false) {
            return;
        }
        ctx.diagnostic(unexpected_mutation_diagnostic(mutation.span, &name));
    }

    /// Upstream's `verifyPropVariable`. `path` is empty when the binding *is*
    /// the props object (`const props = defineProps()`, `setup(props)`), and
    /// otherwise holds the property path the binding was destructured from.
    fn verify_prop_variable(&self, symbol_id: SymbolId, path: &[String], ctx: &LintContext<'_>) {
        let is_root_props = path.is_empty();
        for reference in ctx.scoping().get_resolved_references(symbol_id) {
            // A plain write to the binding itself (`props = x`) is not a prop
            // mutation, and upstream skips it here rather than in `findMutating`.
            if !reference.is_read() {
                continue;
            }
            let node = ctx.nodes().get_node(reference.node_id());
            let Some(mutation) = find_mutating(node, ctx.nodes(), ctx.source_text()) else {
                continue;
            };
            if !self.is_reportable(&mutation, is_root_props) {
                continue;
            }
            let name = if is_root_props {
                // `props` itself carries no prop name; the first step of the
                // path does. `props++` therefore reports nothing.
                let Some(first) = mutation.path.first() else { continue };
                first.display.clone()
            } else {
                // A destructured prop mutated *as a whole* (`foo = 1`, `foo++`)
                // is a write, not a read, so it never reaches here — except
                // through a mutating call, which is a read of `foo`.
                if mutation.path.is_empty() && mutation.kind != MutationKind::Call {
                    continue;
                }
                path[0].clone()
            };
            ctx.diagnostic(unexpected_mutation_diagnostic(mutation.span, &name));
        }
    }

    /// Upstream's `isShallowOnlyInvalid`: with `shallowOnly`, only replacing
    /// the prop binding counts, not writing through it.
    fn is_reportable(&self, mutation: &Mutation, is_root_props: bool) -> bool {
        !self.shallow_only
            || (mutation.path.len() == usize::from(is_root_props)
                && matches!(mutation.kind, MutationKind::Assignment | MutationKind::Update))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutationKind {
    Assignment,
    Update,
    Call,
}

/// Upstream's `findMutating`: walk outwards from a node that holds a prop and
/// decide whether the surrounding expression writes through it.
///
/// `path` mirrors upstream's `pathNodes` — the property names stepped through,
/// innermost first — because the caller needs both its length (how deep the
/// write is) and its first entry (which prop is being written).
struct Mutation {
    kind: MutationKind,
    /// The node to report, which is the whole mutating expression rather than
    /// the prop reference inside it.
    span: Span,
    path: Vec<PathStep>,
}

/// One property step of a mutated path. The two names differ for a computed
/// key: `props[a]` has no static name, but upstream still reports it, spelled
/// `[a]`.
struct PathStep {
    /// Upstream's `getStaticPropertyName`.
    static_name: Option<String>,
    /// Upstream's `getPropertyNameText`.
    display: String,
}

fn find_mutating<'a>(
    start: &AstNode<'a>,
    nodes: &AstNodes<'a>,
    source_text: &str,
) -> Option<Mutation> {
    let mut path: Vec<PathStep> = Vec::new();
    let mut current = start;

    loop {
        let parent = nodes.parent_node(current.id());
        match parent.kind() {
            AstKind::AssignmentExpression(assignment)
                if assignment.left.span() == current.span() =>
            {
                return Some(Mutation {
                    kind: MutationKind::Assignment,
                    span: assignment.span,
                    path,
                });
            }
            AstKind::UpdateExpression(update) => {
                return Some(Mutation { kind: MutationKind::Update, span: update.span, path });
            }
            AstKind::UnaryExpression(unary)
                if unary.operator == UnaryOperator::Delete
                    && unary.argument.span() == current.span() =>
            {
                return Some(Mutation { kind: MutationKind::Update, span: unary.span, path });
            }
            AstKind::CallExpression(call) => {
                if !path.is_empty() && call.callee.span() == current.span() {
                    if let Some(Some(method)) = path.last().map(|step| step.static_name.as_ref())
                        && MUTATING_METHODS.contains(&method.as_str())
                    {
                        // The method name is not part of the mutated path.
                        path.pop();
                        return Some(Mutation { kind: MutationKind::Call, span: call.span, path });
                    }
                    return None;
                }
                if is_object_assign_first_argument(call, current.span()) {
                    return Some(Mutation { kind: MutationKind::Call, span: call.span, path });
                }
                return None;
            }
            AstKind::StaticMemberExpression(member) if member.object.span() == current.span() => {
                let name = member.property.name.to_string();
                path.push(PathStep { static_name: Some(name.clone()), display: name });
                current = parent;
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == current.span() => {
                let static_name = literal_element_name(&member.expression).map(Cow::into_owned);
                let display = static_name
                    .clone()
                    .unwrap_or_else(|| format!("[{}]", &source_text[member.expression.span()]));
                path.push(PathStep { static_name, display });
                current = parent;
            }
            // Transparent wrappers. espree has no parenthesis node, so
            // upstream's walk sees through them; `oxc_parser` keeps them.
            AstKind::ChainExpression(_) | AstKind::ParenthesizedExpression(_) => {
                current = parent;
            }
            _ => return None,
        }
    }
}

/// `Object.assign(prop, …)`, whose first argument is the thing mutated.
fn is_object_assign_first_argument(call: &CallExpression<'_>, span: Span) -> bool {
    if call.arguments.first().is_none_or(|first| first.span() != span) {
        return false;
    }
    let Expression::StaticMemberExpression(callee) = call.callee.get_inner_expression() else {
        return false;
    };
    callee.property.name == "assign"
        && matches!(callee.object.get_inner_expression(), Expression::Identifier(object)
            if object.name == "Object")
}

/// Upstream's `iteratePatternProperties`: every identifier a pattern binds,
/// paired with the property path it was destructured from. An empty path means
/// the binding is the props object itself.
fn collect_pattern_properties(
    pattern: &BindingPattern<'_>,
    path: &[String],
    source_text: &str,
    out: &mut Vec<(SymbolId, Vec<String>)>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            if let Some(symbol_id) = ident.symbol_id.get() {
                out.push((symbol_id, path.to_vec()));
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_pattern_properties(&assignment.left, path, source_text, out);
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                // A computed key still names a prop as far as the message is
                // concerned — upstream spells it `[expr]` — so the binding is
                // kept rather than skipped.
                let name = static_key_name(&property.key).map_or_else(
                    || format!("[{}]", &source_text[property.key.span()]),
                    std::borrow::Cow::into_owned,
                );
                let mut nested = path.to_vec();
                nested.push(name);
                collect_pattern_properties(&property.value, &nested, source_text, out);
            }
            if let Some(rest) = &object.rest {
                collect_pattern_properties(&rest.argument, path, source_text, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for (index, element) in array.elements.iter().enumerate() {
                let Some(element) = element else { continue };
                let mut nested = path.to_vec();
                nested.push(index.to_string());
                collect_pattern_properties(element, &nested, source_text, out);
            }
            if let Some(rest) = &array.rest {
                collect_pattern_properties(&rest.argument, path, source_text, out);
            }
        }
    }
}

/// The prop names of the component options object nearest to `node`, which is
/// upstream's `propsMap.get(vueNode)` resolved by walking out instead of by
/// remembering the object the visitor entered.
fn enclosing_component_prop_names<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) -> Option<FxHashSet<String>> {
    ctx.nodes().ancestors(node.id()).find_map(|ancestor| {
        let AstKind::ObjectExpression(object) = ancestor.kind() else { return None };
        if !is_vue_component_options_object(ancestor, ctx) {
            return None;
        }
        Some(vue_component_prop_names(object))
    })
}

impl VueTemplateRule for NoMutatingProps {
    fn needs_script_props(&self) -> bool {
        true
    }

    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let props = ctx.script_props().clone();
        if props.names.is_empty() && props.object_name.is_none() {
            return;
        }

        let mut reports: Vec<(Span, String)> = Vec::new();
        walk_nodes_with_scope(nodes, &FxHashSet::default(), &mut |node, scope| match node {
            Node::Interpolation(interpolation) => {
                self.collect_expression_mutations(
                    interpolation.expression,
                    interpolation.expression_span.start,
                    false,
                    scope,
                    &props,
                    &mut reports,
                );
            }
            Node::Element(element) => {
                for attribute in &element.attributes {
                    let Some(directive) = &attribute.directive else { continue };
                    if let Some(report) = self.two_way_binding_mutation(attribute, scope, &props) {
                        reports.push(report);
                    }
                    if let Some((text, span)) = directive_expression(attribute) {
                        self.collect_expression_mutations(
                            text,
                            span.start,
                            directive.name == "on",
                            scope,
                            &props,
                            &mut reports,
                        );
                    }
                }
            }
            _ => {}
        });

        // The walk visits attributes before children, so sort for a source-ordered report.
        reports.sort_unstable_by_key(|(span, _)| span.start);
        for (span, name) in reports {
            ctx.diagnostic(unexpected_mutation_diagnostic(span, &name));
        }
    }
}

/// `(\n` — see [`crate::utils::TemplateExpressionKind`], whose wrappers these
/// mirror so a template expression is parsed here exactly as
/// `vue/no-parsing-error` parses it.
const EXPRESSION_PREFIX: u32 = 2;
/// `void function($event) {\n`
const ON_STATEMENTS_PREFIX: u32 = 24;

impl NoMutatingProps {
    /// Upstream's two `VExpressionContainer` handlers, over one template
    /// expression: every free reference that names a prop, and every `this.x`.
    fn collect_expression_mutations(
        &self,
        text: &str,
        base: u32,
        is_on_statements: bool,
        scope: &FxHashSet<String>,
        props: &VueScriptProps,
        out: &mut Vec<(Span, String)>,
    ) {
        let (snippet, prefix) = if is_on_statements {
            (format!("void function($event) {{\n{text}\n}};"), ON_STATEMENTS_PREFIX)
        } else {
            (format!("(\n{text}\n);"), EXPRESSION_PREFIX)
        };

        let allocator = Allocator::new();
        let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
        if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
            return;
        }
        let program = allocator.alloc(parser_ret.program);
        let semantic = SemanticBuilder::new_linter().build(program).semantic;
        let nodes = semantic.nodes();
        let translate =
            |span: Span| Span::new(base + span.start - prefix, base + span.end - prefix);

        // Upstream's `'VExpressionContainer Identifier'` handler. A reference
        // that the snippet resolves itself, or that a `v-for`/`v-slot` binding
        // declares, is not a component reference — which is exactly what
        // upstream's `isVmReference` (`reference.variable == null`) tests.
        for (name, reference_ids) in semantic.scoping().root_unresolved_references() {
            if scope.contains(name.as_str()) {
                continue;
            }
            let is_root_props = props.object_name.as_deref() == Some(name.as_str());
            for &reference_id in reference_ids {
                let node = nodes.get_node(semantic.scoping().get_reference(reference_id).node_id());
                // On the props object itself the prop is the property being
                // read; elsewhere the reference *is* the prop.
                let reported = if is_root_props {
                    member_static_property_name(nodes, node)
                        .unwrap_or_else(|| name.as_str().to_string())
                } else {
                    name.as_str().to_string()
                };
                if !is_root_props && !props.names.contains(&reported) {
                    continue;
                }
                let Some(mutation) = find_mutating(node, nodes, &snippet) else { continue };
                if !self.is_reportable(&mutation, is_root_props) {
                    continue;
                }
                out.push((translate(mutation.span), reported));
            }
        }

        // Upstream's `'VExpressionContainer MemberExpression > ThisExpression'`.
        for node in nodes.iter() {
            if !matches!(node.kind(), AstKind::ThisExpression(_)) {
                continue;
            }
            let member = nodes.parent_node(node.id());
            let Some(name) = member_static_property_name(nodes, node) else { continue };
            if !props.names.contains(&name) {
                continue;
            }
            let Some(mutation) = find_mutating(member, nodes, &snippet) else { continue };
            if !self.is_reportable(&mutation, false) {
                continue;
            }
            out.push((translate(mutation.span), name));
        }
    }

    /// Upstream's `v-model` / `v-bind.sync` handler. Both write back through
    /// the binding, so the expression itself is the mutation and there is no
    /// assignment in the source to find.
    fn two_way_binding_mutation(
        &self,
        attribute: &Attribute<'_>,
        scope: &FxHashSet<String>,
        props: &VueScriptProps,
    ) -> Option<(Span, String)> {
        let directive = attribute.directive.as_ref()?;
        match directive.name {
            "model" => {}
            "bind" if directive.modifiers.contains(&"sync") => {}
            _ => return None,
        }
        let (text, span) = directive_expression(attribute)?;

        let snippet = format!(
            "(
{text}
);"
        );
        let allocator = Allocator::new();
        let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
        if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
            return None;
        }
        let statement = parser_ret.program.body.first()?;
        let oxc_ast::ast::Statement::ExpressionStatement(statement) = statement else {
            return None;
        };
        let expression = statement.expression.without_parentheses();

        // Upstream's `getMemberChaining`: the root, then each member outwards.
        // It unwraps optional chaining and (implicitly, since espree has no
        // such node) parentheses, but NOT TypeScript wrappers — so `prop!` has
        // a `TSNonNullExpression` root and is left alone.
        let mut steps: Vec<(Option<String>, String)> = Vec::new();
        let mut current = expression;
        loop {
            let member = match current.without_parentheses() {
                Expression::StaticMemberExpression(member) => {
                    let name = member.property.name.to_string();
                    steps.push((Some(name.clone()), name));
                    &member.object
                }
                Expression::ComputedMemberExpression(member) => {
                    let static_name = literal_element_name(&member.expression).map(Cow::into_owned);
                    let display = static_name.clone().unwrap_or_else(|| {
                        format!("[{}]", &snippet.as_str()[member.expression.span()])
                    });
                    steps.push((static_name, display));
                    &member.object
                }
                Expression::ChainExpression(chain) => match &chain.expression {
                    ChainElement::StaticMemberExpression(member) => {
                        let name = member.property.name.to_string();
                        steps.push((Some(name.clone()), name));
                        &member.object
                    }
                    ChainElement::ComputedMemberExpression(member) => {
                        let static_name =
                            literal_element_name(&member.expression).map(Cow::into_owned);
                        let display = static_name.clone().unwrap_or_else(|| {
                            format!("[{}]", &snippet.as_str()[member.expression.span()])
                        });
                        steps.push((static_name, display));
                        &member.object
                    }
                    _ => break,
                },
                _ => break,
            };
            current = member;
        }
        // Collected outermost-first; upstream indexes from the root.
        steps.reverse();
        let length = steps.len() + 1;

        let name = match current.without_parentheses() {
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                if scope.contains(name) {
                    return None;
                }
                if props.object_name.as_deref() == Some(name) {
                    if self.shallow_only && length > 2 {
                        return None;
                    }
                    steps.first().map_or_else(|| name.to_string(), |step| step.1.clone())
                } else {
                    if self.shallow_only && length > 1 {
                        return None;
                    }
                    if !props.names.contains(name) {
                        return None;
                    }
                    name.to_string()
                }
            }
            Expression::ThisExpression(_) => {
                if self.shallow_only && length > 2 {
                    return None;
                }
                let name = steps.first()?.0.clone()?;
                if !props.names.contains(&name) {
                    return None;
                }
                name
            }
            _ => return None,
        };

        let expression_span = expression.span();
        Some((
            Span::new(
                span.start + expression_span.start - EXPRESSION_PREFIX,
                span.start + expression_span.end - EXPRESSION_PREFIX,
            ),
            name,
        ))
    }
}

/// The static property name of the member expression `node` is the object of,
/// when it is one.
fn member_static_property_name<'a>(nodes: &AstNodes<'a>, node: &AstNode<'a>) -> Option<String> {
    match nodes.parent_kind(node.id()) {
        AstKind::StaticMemberExpression(member) if member.object.span() == node.span() => {
            Some(member.property.name.to_string())
        }
        AstKind::ComputedMemberExpression(member) if member.object.span() == node.span() => {
            literal_element_name(&member.expression).map(Cow::into_owned)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoMutatingProps;
    use crate::{rule::RuleMeta, tester::Tester};

    // Cases marked "upstream" are transcribed from eslint-plugin-vue's own
    // `tests/lib/rules/no-mutating-props.js` at tag v10.6.2.
    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            // upstream: the props object itself is not a prop.
            (
                "<script>export default { setup(props) { props ++; props = 1; props.push(1) } }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: a destructured prop replaced wholesale is a write, not
            // a read, so upstream never sees it.
            ("<script>export default { setup({a}) { a ++; a = 1 } }</script>", None, None, vue()),
            (
                "<script>export default { setup({...props}) { props ++; props = 1; props.push(1) } }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: not a `setup` function.
            ("<script>export default { ssss(props) { props.a ++ } }</script>", None, None, vue()),
            // upstream: reading a prop is fine.
            (
                "<script>export default { setup(props) { const a = props.a } }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: non-mutating array methods, and `this.$emit`.
            (
                "<script>export default { props: { todo: { type: Object }, items: { type: Array } }, methods: { openModal() { this.$emit('e', this.todo); const a = this.items.slice(0) } } }</script>",
                None,
                None,
                vue(),
            ),
            // A name that is not a declared prop is not this rule's business.
            (
                "<script>export default { props: ['foo'], methods: { m() { this.bar = 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            // `this` outside a component options object.
            ("<script>const o = { m() { this.foo = 1 } }</script>", None, None, vue()),
            // shallowOnly: writing *through* a prop is allowed.
            (
                "<script>export default { props: ['todo'], methods: { m() { this.todo.type = 'x' } } }</script>",
                Some(json!([{ "shallowOnly": true }])),
                None,
                vue(),
            ),
            (
                "<script setup>const props = defineProps({ value: Object })\nprops.value.x = 1</script>",
                Some(json!([{ "shallowOnly": true }])),
                None,
                vue(),
            ),
            // upstream: names that are not props.
            (
                "<template><div><input v-model=\"prop1.text\"><input v-model=\"prop2\"><input v-model=\"this.prop3.text\"><input v-model=\"this.prop4\"></div></template><script>export default { props: ['prop5', 'prop6'] }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: a `v-for` alias shadows the prop of the same name.
            (
                "<template><div v-for=\"prop in data\"><input v-model=\"prop\"><MyComp @click=\"prop.foo++\" /></div></template><script>export default { props: ['prop'] }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: non-mutating methods on a prop.
            (
                "<template><input v-for=\"i in prop.slice()\"><input v-for=\"i in prop.foo.slice()\"></template><script>export default { props: ['prop'] }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: `:data` without `.sync` is a one-way binding.
            (
                "<template><MyComponent :data=\"prop\" /><MyComponent :data=\"this.prop\" /></template><script>export default { props: ['prop'] }</script>",
                None,
                None,
                vue(),
            ),
            // A TypeScript non-null assertion is not a member chain upstream
            // can see through, so the binding root is a `TSNonNullExpression`
            // and nothing is reported. Found by differential testing.
            (
                "<template><Comp :src.sync=\"value!\" /></template><script setup lang=\"ts\">const props = defineProps<{ value: string }>()\nconst value = props.value</script>",
                None,
                None,
                vue(),
            ),
            // upstream: shallowOnly leaves writes *through* a prop alone.
            (
                "<template><input v-model=\"prop1.text\"><button @click=\"prop3.list.push(1)\"></button><button @click=\"delete prop3.parent.text\"></button></template><script>export default { props: ['prop1', 'prop3'] }</script>",
                Some(json!([{ "shallowOnly": true }])),
                None,
                vue(),
            ),
        ];

        let fail = vec![
            // upstream: every mutation shape, through `this`.
            (
                "<script>export default {
  props: { todo: { type: Object }, items: { type: Array } },
  methods: {
    openModal() {
      ++this.items
      this.todo.type = 'completed'
      this.items.push('something')
      delete this.todo.type
    }
  }
}</script>",
                None, None, vue(),
            ),
            // upstream: optional chaining, including through parentheses.
            (
                "<script>export default {
  props: ['foo', 'bar', 'baz'],
  methods: {
    openModal() {
      this?.foo?.push?.('something')
      ;(this?.bar)?.push?.('something')
      ;(this?.baz?.push)?.('something')
    }
  }
}</script>",
                None, None, vue(),
            ),
            // upstream: the `setup(props)` parameter.
            (
                "<script>export default {
  setup(props) {
    props.a ++
    props.b = 1
    props.c.push(1)
    delete props.d
  }
}</script>",
                None, None, vue(),
            ),
            // upstream: destructured parameters, including nested and holes.
            (
                "<script>export default {
  setup({a,b,c, d: [e, , f]}) {
    a.foo ++
    b.foo = 1
    c.push(1)
    c.x.push(1)
    delete c.y
    e.foo++
    f.foo++
  }
}</script>",
                None, None, vue(),
            ),
            // upstream: renamed, rest and defaulted destructuring.
            (
                "<script>export default {
  setup({a: foo, b: [...bar], c: baz = 1}) {
    foo.x ++
    delete foo.y
    bar.x = 1
    baz.push(1)
  }
}</script>",
                None, None, vue(),
            ),
            // upstream: an object rest parameter is still the props object.
            (
                "<script>export default {
  setup({...props}) {
    props.a ++
    props.b = 1
    props.c.push(1)
    delete props.d
  }
}</script>",
                None, None, vue(),
            ),
            // upstream: a computed key still names a prop, spelled `[a]`.
            ("<script>export default { setup(props) { props[a] ++ } }</script>", None, None, vue()),
            (
                "<script>export default { setup({[a]: c}) { c.foo ++ } }</script>",
                None, None, vue(),
            ),
            // upstream: `<script setup>` forms.
            (
                "<script setup>const props = defineProps({ value: String })\nprops.value++</script>",
                None, None, vue(),
            ),
            (
                "<script setup>const {value} = defineProps({ value: Object })\nvalue.value++</script>",
                None, None, vue(),
            ),
            (
                "<script setup lang=\"ts\">const props = withDefaults(defineProps<Props>(), { msg: 'hello' })\nprops.value++</script>",
                None, None, vue(),
            ),
            // upstream: a mutation inside a nested function still counts.
            (
                "<script>export default {
  setup(props) {
    props.a ++
    function foo() { props.a ++ }
  }
}</script>",
                None, None, vue(),
            ),
            // `Object.assign` onto a prop.
            (
                "<script>export default { props: ['todo'], methods: { m() { Object.assign(this.todo, {}) } } }</script>",
                None, None, vue(),
            ),
            // shallowOnly reports replacing the binding, but not writing through it.
            (
                "<script setup>const props = defineProps({ value: Object })\nprops.value = 1\nprops.value.x = 1</script>",
                Some(json!([{ "shallowOnly": true }])),
                None,
                vue(),
            ),
            // upstream: every mutation shape inside template expressions.
            (
                "<template><div>
  <div v-if=\"prop1 = [1, 2]\"></div>
  <div v-if=\"prop2++\"></div>
  <div v-text=\"prop3.shift()\"></div>
  <div v-text=\"prop4.slice(0).shift()\"></div>
  <div v-if=\"this.prop5 = [1, 2] && this.someProp\"></div>
  <div v-if=\"this.prop6++ && this.someProp < 10\"></div>
  <div v-text=\"this.prop7.shift()\"></div>
  <div v-if=\"delete prop9.a\"></div>
</div></template><script>export default { props: ['prop1','prop2','prop3','prop4','prop5','prop6','prop7','prop9'] }</script>",
                None, None, vue(),
            ),
            // upstream: optional chaining, including through parentheses.
            (
                "<template><div>
  <div v-text=\"prop1?.shift?.()\"></div>
  <div v-text=\"(prop2?.shift)?.()\"></div>
  <div v-text=\"(this?.prop3)?.shift?.()\"></div>
</div></template><script>export default { props: ['prop1','prop2','prop3'] }</script>",
                None, None, vue(),
            ),
            // upstream: `v-model` writes back, so the binding is the mutation.
            (
                "<template><div>
  <input v-model=\"prop1.text\">
  <input v-model=\"prop2\">
  <input v-model=\"this.prop3.text\">
  <input v-model=\"this.prop4\">
</div></template><script>export default { props: ['prop1','prop2','prop3','prop4'] }</script>",
                None, None, vue(),
            ),
            // upstream: only the `v-model` outside the shadowing `v-for` counts.
            (
                "<template><div><template v-for=\"(prop, i) of data\"><input v-model=\"prop\"></template><input v-model=\"prop\"></div></template><script>export default { props: ['prop'] }</script>",
                None, None, vue(),
            ),
            // upstream: a `v-for` alias never shadows `this.prop`.
            (
                "<template><div><template v-for=\"prop of data\"><input v-model=\"this.prop\"><div v-text=\"prop.shift()\"></div><div v-text=\"this.prop.shift()\"></div></template></div></template><script>export default { props: ['prop'] }</script>",
                None, None, vue(),
            ),
            // upstream: `.sync` is two-way, plain `:data` is not.
            (
                "<template><div>
  <MyComponent :data.sync=\"this.prop\" />
  <MyComponent :data.sync=\"prop\" />
  <MyComponent :data=\"this.prop\" />
  <MyComponent :data=\"prop\" />
</div></template><script>export default { props: ['prop'] }</script>",
                None, None, vue(),
            ),
            // upstream: a computed key on the props object.
            (
                "<script setup>const props = defineProps()</script><template><input v-model=\"props[foo]\"></template>",
                None, None, vue(),
            ),
            // upstream: both the bare prop and the props-object spelling.
            (
                "<template><input v-model=\"value\"><input v-model=\"props.value\"></template><script setup>const props = defineProps({ value: String })</script>",
                None, None, vue(),
            ),
            // A prop destructured *with a default* is still a prop. It is also
            // a module-scope binding, and upstream filters those out of the
            // prop set before adding the destructured names back — so getting
            // only half of that right silently loses every defaulted prop.
            // Found by differential testing.
            (
                "<template><Comp :quantity.sync=\"quantity\" :sender.sync=\"sender\" /></template><script setup lang=\"ts\">interface Props { quantity?: number; sender: string }\nconst { sender, quantity = 5 } = defineProps<Props>()</script>",
                None, None, vue(),
            ),
            // upstream: shallowOnly across both halves of the rule.
            (
                "<template>
  <button @click=\"props.a ++\"/>
  <button @click=\"a ++\"/>
  <button @click=\"props.b.push(1)\"/>
  <button @click=\"b.push(1)\"/>
  <input v-model=\"props.a\"/>
  <input v-model=\"props.a.b\"/>
</template><script setup>const props = defineProps({ a: Number, b: Array })\nprops.a ++</script>",
                Some(json!([{ "shallowOnly": true }])),
                None,
                vue(),
            ),
        ];

        Tester::new(NoMutatingProps::NAME, NoMutatingProps::PLUGIN, pass, fail).test_and_snapshot();
    }
}
