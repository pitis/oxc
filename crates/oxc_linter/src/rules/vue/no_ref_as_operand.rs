use oxc_ast::{
    AstKind,
    ast::{
        AssignmentOperator, AssignmentTarget, BindingPattern, Expression, VariableDeclarationKind,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::SymbolId;
use oxc_span::{GetSpan, Span};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AstNode,
    ast_util::variable_declaration_kind,
    context::LintContext,
    frameworks::FrameworkOptions,
    module_record::ImportImportName,
    rule::Rule,
    utils::{find_property, is_vue_component_options_object, literal_element_name},
};

/// The keys of upstream's ref-factory trace map (`ref-object-references.js`'s
/// `iterateDefineRefs`).
const FACTORY_NAMES: [&str; 6] = ["ref", "computed", "toRef", "customRef", "shallowRef", "toRefs"];
const DEFINE_MODEL: &str = "defineModel";

/// Upstream's `createCompositionApiTraceMap`. `#imports` is Nuxt/unimport's
/// virtual module, and is already accepted by `is_vue_computed_call`.
const VUE_MODULES: [&str; 3] = ["vue", "@vue/composition-api", "#imports"];

fn require_dot_value_diagnostic(span: Span, method: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Must use `.value` to read or write the value wrapped by `{method}()`."
    ))
    .with_help("Add `.value` to read the value the ref wraps.")
    .with_label(span)
}

/// One binding that holds a ref object.
#[derive(Clone, Copy)]
struct RefBinding {
    /// The *imported* factory name, which is what upstream substitutes into
    /// the message: `import { ref as r } from 'vue'` reports `` `ref()` ``,
    /// not `` `r()` ``.
    method: &'static str,
    /// Upstream's `isRefInit`, which requires the binding's own
    /// `VariableDeclarator.init` to be the node the ref arrived through. A ref
    /// that arrived by plain assignment (`let foo; foo = ref(0)`) is tracked
    /// so it can propagate to an alias, but is never itself reported.
    reportable: bool,
    /// Only the `LogicalExpression` case reads this: upstream reports
    /// `foo || other` only for a `const` binding.
    is_const: bool,
}

#[derive(Debug, Default, Clone)]
pub struct NoRefAsOperand;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows using a variable that holds a ref object — the result of
    /// `ref()`, `computed()`, `shallowRef()`, `toRef()`, `customRef()`,
    /// `toRefs()` or the `defineModel()` macro — directly as an operand,
    /// instead of reading through its `.value`.
    ///
    /// ### Why is this bad?
    ///
    /// A ref is a wrapper object, so the mistake does not throw and does not
    /// fail to type-check in plain JavaScript — it silently computes the wrong
    /// thing. `` `${count}` `` renders `[object Object]`, `count > 5` is always
    /// false, and `count++` replaces the ref with `NaN`, which quietly breaks
    /// every other reader of that ref.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// import { ref } from 'vue'
    /// const count = ref(0)
    ///
    /// count++
    /// console.log(count + 1)
    /// if (count) { /* always truthy: a ref object */ }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { ref } from 'vue'
    /// const count = ref(0)
    ///
    /// count.value++
    /// console.log(count.value + 1)
    /// if (count.value) {}
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// A ref reached through a property of a `toRefs()` result object is only
    /// tracked when it is destructured (`const { foo } = toRefs(props)`), not
    /// when the object is kept and read later (`const r = toRefs(props);
    /// r.foo`). Upstream follows the second form through a general
    /// property-reference engine that nothing else in this linter needs.
    ///
    /// The factory has to be called directly: upstream tracks
    /// `const myRef = ref; myRef(0)`, this does not.
    ///
    /// `typeof someRef` *is* reported, matching upstream — its
    /// `UnaryExpression > Identifier` selector has no operator filter, so
    /// `typeof` and `void` are caught along with `-`, `+`, `!` and `~`.
    ///
    /// A ref declared in a `<script>` block and used in a `<script setup>`
    /// block of the same file is not tracked, because each block is analysed
    /// as its own program here while vue-eslint-parser merges the two. This is
    /// a property of the whole linter, not of this rule.
    ///
    /// `require('vue')` is not tracked — and neither is it upstream, whose
    /// `iterateReferencesTraceMap` covers ESM imports and globals only.
    NoRefAsOperand,
    vue,
    correctness,
    fix,
    version = "1.80.0",
    short_description = "Disallow use of a value wrapped by `ref()` as an operand.",
);

impl Rule for NoRefAsOperand {
    // Driven from references rather than from a node visit: the alias case
    // (`let bar = someRef`) needs the factory call's binding to be known before
    // the alias is classified, which a per-node `run` cannot do without shared
    // mutable state. A file that never names a ref factory costs one pass over
    // the import list and a handful of hash lookups, and touches no AST node.
    fn run_once(&self, ctx: &LintContext) {
        let sites = define_sites(ctx);
        if sites.is_empty() {
            return;
        }
        let mut refs: FxHashMap<SymbolId, RefBinding> = FxHashMap::default();
        // Upstream's `_processedIds`, shared across every site: see [`register`].
        let mut processed: FxHashSet<u32> = FxHashSet::default();
        for site in sites {
            process_define_site(site, ctx, &mut refs, &mut processed);
        }
        report(ctx, &refs);
    }
}

/// One factory call that produces a ref object.
#[derive(Clone, Copy)]
struct DefineSite<'n> {
    call: &'n AstNode<'n>,
    method: &'static str,
    /// Where this call falls in upstream's processing order; see
    /// [`define_sites`].
    order: (u8, u32, u32),
}

/// Every factory call in the file, in the order upstream processes them.
///
/// This order is load-bearing, not cosmetic. Upstream re-registers *every*
/// reference of a binding each time it reaches it, and the last registration
/// wins, so when a ref is copied into another variable (`b = a`) it is the
/// processing order that decides whether `b` keeps counting as its own ref.
///
/// `ReferenceTracker` drains `iterateEsmReferences` before
/// `iterateGlobalReferences`, and within the ESM half it walks the *import
/// specifiers* in source order — so `import { computed, ref }` processes
/// `ref` last and `import { ref, computed }` processes `computed` last, and
/// the two spellings genuinely disagree about `b`. Globals follow, in
/// trace-map key order, and the `defineModel` macro is a separate pass after
/// both.
fn define_sites<'n>(ctx: &'n LintContext<'_>) -> Vec<DefineSite<'n>> {
    let mut sites = Vec::new();
    collect_import_sites(ctx, &mut sites);
    collect_global_sites(ctx, &mut sites);
    sites.sort_unstable_by_key(|site| site.order);
    sites
}

/// Upstream's `iterateEsmReferences` half: named (and aliased) imports of a
/// factory, plus `import * as vue from 'vue'` member calls.
fn collect_import_sites<'n>(ctx: &'n LintContext<'_>, sites: &mut Vec<DefineSite<'n>>) {
    let scoping = ctx.scoping();
    for entry in &ctx.module_record().import_entries {
        if !VUE_MODULES.contains(&entry.module_request.name()) {
            continue;
        }
        let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into()) else {
            continue;
        };
        match &entry.import_name {
            ImportImportName::Name(name_span) => {
                let Some(method) = factory_name(name_span.name()) else { continue };
                for reference in scoping.get_resolved_references(symbol_id) {
                    let node = ctx.nodes().get_node(reference.node_id());
                    let AstKind::CallExpression(call) = ctx.nodes().parent_kind(node.id()) else {
                        continue;
                    };
                    if call.callee.span() == node.span() {
                        sites.push(DefineSite {
                            call: ctx.nodes().parent_node(node.id()),
                            method,
                            order: (0, entry.local_name.span.start, call.span.start),
                        });
                    }
                }
            }
            // `import * as vue from 'vue'; vue.ref(0)`.
            ImportImportName::NamespaceObject => {
                for reference in scoping.get_resolved_references(symbol_id) {
                    let node = ctx.nodes().get_node(reference.node_id());
                    let member_node = ctx.nodes().parent_node(node.id());
                    let AstKind::StaticMemberExpression(member) = member_node.kind() else {
                        continue;
                    };
                    if member.object.span() != node.span() {
                        continue;
                    }
                    let Some(method) = factory_name(&member.property.name) else { continue };
                    let call_node = ctx.nodes().parent_node(member_node.id());
                    let AstKind::CallExpression(call) = call_node.kind() else { continue };
                    if call.callee.span() == member_node.span() {
                        sites.push(DefineSite {
                            call: call_node,
                            method,
                            order: (0, entry.local_name.span.start, call.span.start),
                        });
                    }
                }
            }
            ImportImportName::Default(_) => {}
        }
    }
}

/// Upstream's `iterateGlobalReferences` half, plus the `defineModel()` macro.
///
/// `is_global_defined` is the exact counterpart of eslint-utils' requirement
/// that the name exist in `globalScope.set`: an auto-imported `ref` is only
/// tracked when the config actually declares it as a global, which is what
/// upstream's own auto-import test case configures. `defineModel` is different
/// — it is a compiler macro, so upstream reads `globalScope.through` directly
/// and no `globals` entry is needed, and no `<script setup>` gate either.
fn collect_global_sites<'n>(ctx: &'n LintContext<'_>, sites: &mut Vec<DefineSite<'n>>) {
    let scoping = ctx.scoping();
    for (index, name) in FACTORY_NAMES.into_iter().chain(std::iter::once(DEFINE_MODEL)).enumerate()
    {
        let is_macro = name == DEFINE_MODEL;
        if !is_macro && !ctx.is_global_defined(name) {
            continue;
        }
        let Some(reference_ids) = scoping.root_unresolved_references().get(name) else { continue };
        // eslint-utils' `isModifiedGlobal`: a global that the file assigns to
        // is not the library's function any more.
        if !is_macro && reference_ids.iter().any(|&id| scoping.get_reference(id).is_write()) {
            continue;
        }
        let Some(method) = (if is_macro { Some(DEFINE_MODEL) } else { factory_name(name) }) else {
            continue;
        };
        for &reference_id in reference_ids {
            let node = ctx.nodes().get_node(scoping.get_reference(reference_id).node_id());
            let call_node = ctx.nodes().parent_node(node.id());
            let AstKind::CallExpression(call) = call_node.kind() else { continue };
            if call.callee.span() == node.span() {
                // Globals come after every ESM reference, and `defineModel` is
                // a pass of its own after those.
                let group = if is_macro { 2 } else { 1 };
                sites.push(DefineSite {
                    call: call_node,
                    method,
                    order: (group, u32::try_from(index).unwrap_or(u32::MAX), call.span.start),
                });
            }
        }
    }
}

/// The name as upstream would report it, so an alias resolves to the imported
/// spelling rather than the local one.
fn factory_name(name: &str) -> Option<&'static str> {
    FACTORY_NAMES.into_iter().find(|factory| *factory == name)
}

/// Upstream's `processDefineRef` / `processDefineModel`: bind the ref this
/// call produces to whatever pattern receives it, then let it flow outwards.
fn process_define_site(
    site: DefineSite<'_>,
    ctx: &LintContext<'_>,
    refs: &mut FxHashMap<SymbolId, RefBinding>,
    processed: &mut FxHashSet<u32>,
) {
    let DefineSite { call, method, .. } = site;
    match ctx.nodes().parent_kind(call.id()) {
        AstKind::VariableDeclarator(declarator) => {
            if declarator.init.as_ref().is_none_or(|init| init.span() != call.span()) {
                return;
            }
            let mut bound = Vec::new();
            match method {
                // Only `toRefs` unpacks an object pattern. `const { value } =
                // ref(0)` is deliberately NOT a ref binding upstream, which is
                // why `processPattern` is reached for every other factory but
                // this one goes through the property-reference extractor.
                "toRefs" => {
                    let BindingPattern::ObjectPattern(pattern) = &declarator.id else { return };
                    bound.extend(pattern.properties.iter().filter_map(|p| binding_key(&p.value)));
                }
                // `const [model, modifiers] = defineModel()` — only the first
                // element is the ref.
                DEFINE_MODEL => match &declarator.id {
                    BindingPattern::ArrayPattern(pattern) => {
                        if let Some(Some(first)) = pattern.elements.first() {
                            bound.extend(binding_key(first));
                        }
                    }
                    pattern => bound.extend(binding_key(pattern)),
                },
                _ => bound.extend(binding_key(&declarator.id)),
            }
            for (key, symbol_id) in bound {
                register(key, symbol_id, method, &[call.span().start], ctx, refs, processed);
            }
        }
        // `let foo; foo = ref(0)`. Tracked so an alias of `foo` can be
        // reported, but `foo` itself never is: `isRefInit` looks at the
        // binding's declarator initializer, which here is not the call.
        AstKind::AssignmentExpression(assignment) => {
            if assignment.operator != AssignmentOperator::Assign
                || assignment.right.span() != call.span()
            {
                return;
            }
            let AssignmentTarget::AssignmentTargetIdentifier(ident) = &assignment.left else {
                return;
            };
            let Some(symbol_id) = ctx.scoping().get_reference(ident.reference_id()).symbol_id()
            else {
                return;
            };
            register(
                ident.span.start,
                symbol_id,
                method,
                &[call.span().start],
                ctx,
                refs,
                processed,
            );
        }
        _ => {}
    }
}

/// The identifier a pattern binds — its span start, which stands in for
/// upstream's identity-keyed `_processedIds`, and its symbol. Looks through a
/// default, so `{ foo = 1 }` still binds `foo`.
fn binding_key(pattern: &BindingPattern<'_>) -> Option<(u32, SymbolId)> {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            ident.symbol_id.get().map(|symbol_id| (ident.span.start, symbol_id))
        }
        BindingPattern::AssignmentPattern(assignment) => binding_key(&assignment.left),
        _ => None,
    }
}

/// The symbol a pattern binds, when it is a plain identifier.
fn binding_symbol(pattern: &BindingPattern<'_>) -> Option<SymbolId> {
    binding_key(pattern).map(|(_, symbol_id)| symbol_id)
}

/// Upstream's `processIdentifierPattern` + `processExpression`: record that
/// `symbol_id` holds a ref, then follow it into anything it is copied into.
///
/// `chain` is upstream's `defineChain` — the factory call this started from,
/// plus every identifier the ref has been copied through to get here. A
/// binding is reportable exactly when its own declarator initializer is one of
/// those nodes (upstream's `isRefInit`), which is why `const bar = someRef` is
/// reportable while `bar = someRef` normally is not: the assignment leaves
/// `bar`'s initializer outside the chain. The exception is a ref assigned back
/// into the binding it came from, whose initializer is still the chain's root.
///
/// `key` is the span start of the *identifier node* being processed, standing
/// in for upstream's `_processedIds`, which is keyed by node identity and
/// shared across every source. That set is what makes the outcome depend on
/// which source reaches a binding first: once a propagation has gone through a
/// given identifier, a later source reaching that same identifier does nothing
/// at all, so the binding keeps the earlier source's chain.
///
/// Registration otherwise *overwrites*, because upstream's `references.set`
/// does — which is why [`define_sites`]'s order matters too.
fn register(
    key: u32,
    symbol_id: SymbolId,
    method: &'static str,
    chain: &[u32],
    ctx: &LintContext<'_>,
    refs: &mut FxHashMap<SymbolId, RefBinding>,
    processed: &mut FxHashSet<u32>,
) {
    if !processed.insert(key) {
        return;
    }

    // Upstream's `isRefInit`, plus the `kind === 'const'` the logical-operand
    // case needs; both read the binding's own declaration.
    let (reportable, is_const) =
        match ctx.nodes().get_node(ctx.scoping().symbol_declaration(symbol_id)).kind() {
            AstKind::VariableDeclarator(declarator) => (
                declarator.init.as_ref().is_some_and(|init| chain.contains(&init.span().start)),
                variable_declaration_kind(declarator, ctx) == VariableDeclarationKind::Const,
            ),
            _ => (false, false),
        };
    refs.insert(symbol_id, RefBinding { method, reportable, is_const });

    // Collect first: the recursive call needs `refs` and `processed` mutably.
    let mut onwards = Vec::new();
    for reference in ctx.scoping().get_resolved_references(symbol_id) {
        if !reference.is_read() {
            continue;
        }
        let node = ctx.nodes().get_node(reference.node_id());
        match ctx.nodes().parent_kind(node.id()) {
            AstKind::VariableDeclarator(declarator) => {
                if declarator.init.as_ref().is_none_or(|init| init.span() != node.span()) {
                    continue;
                }
                if let Some((alias_key, alias)) = binding_key(&declarator.id) {
                    onwards.push((alias_key, alias, node.span().start));
                }
            }
            AstKind::AssignmentExpression(assignment) => {
                if assignment.operator != AssignmentOperator::Assign
                    || assignment.right.span() != node.span()
                {
                    continue;
                }
                let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left else {
                    continue;
                };
                if let Some(target_id) =
                    ctx.scoping().get_reference(target.reference_id()).symbol_id()
                {
                    onwards.push((target.span.start, target_id, node.span().start));
                }
            }
            _ => {}
        }
    }
    for (next_key, next, via) in onwards {
        let mut next_chain = Vec::with_capacity(chain.len() + 1);
        next_chain.push(via);
        next_chain.extend_from_slice(chain);
        register(next_key, next, method, &next_chain, ctx, refs, processed);
    }
}

fn report(ctx: &LintContext<'_>, refs: &FxHashMap<SymbolId, RefBinding>) {
    let mut reports: Vec<(Span, &'static str)> = Vec::new();
    for (&symbol_id, binding) in refs {
        if !binding.reportable {
            continue;
        }
        for reference in ctx.scoping().get_resolved_references(symbol_id) {
            let node = ctx.nodes().get_node(reference.node_id());
            if is_operand_use(node, *binding, ctx) {
                reports.push((node.span(), binding.method));
            }
        }
    }
    // `refs` is a hash map, so emit in source order for a stable snapshot.
    reports.sort_unstable_by_key(|(span, _)| span.start);
    for (span, method) in reports {
        ctx.diagnostic_with_fix(require_dot_value_diagnostic(span, method), |fixer| {
            fixer.insert_text_after_range(span, ".value")
        });
    }
}

/// Whether this reference sits in one of the positions upstream's ten
/// selectors match. `A > B` in ESLint is a *direct* child, which does most of
/// the narrowing for free: `if (bar) foo` puts an `ExpressionStatement`
/// between the `IfStatement` and the identifier, and `case foo:` hangs off a
/// `SwitchCase`, so neither matches.
fn is_operand_use(node: &AstNode<'_>, binding: RefBinding, ctx: &LintContext<'_>) -> bool {
    // espree has no parenthesis node, so upstream's selectors see through
    // them; oxc parses with `preserve_parens`. Climb out, remembering the
    // outermost span so the "is this the left operand?" checks still line up.
    let mut child_span = node.span();
    let mut parent = ctx.nodes().parent_node(node.id());
    while let AstKind::ParenthesizedExpression(paren) = parent.kind() {
        child_span = paren.span;
        parent = ctx.nodes().parent_node(parent.id());
    }

    match parent.kind() {
        // Unconditional positions:
        // - `IfStatement`'s only expression child is its test, and
        //   `SwitchStatement`'s is its discriminant;
        // - `UnaryExpression` has no operator filter upstream, so `typeof ref`
        //   and `void ref` are reported alongside `-ref`, `+ref`, `!ref`, `~ref`;
        // - `BinaryExpression` reports either operand — `&&`/`||`/`??` are a
        //   `LogicalExpression`, handled below, exactly as in ESTree.
        AstKind::IfStatement(_)
        | AstKind::SwitchStatement(_)
        | AstKind::UnaryExpression(_)
        | AstKind::UpdateExpression(_)
        | AstKind::BinaryExpression(_) => true,
        // `ref = x` and `ref += x` and `x += ref`, but not `x = ref`.
        AstKind::AssignmentExpression(assignment) => {
            assignment.operator != AssignmentOperator::Assign
                || assignment.left.span() == child_span
        }
        AstKind::LogicalExpression(logical) => {
            binding.is_const && logical.left.span() == child_span
        }
        AstKind::ConditionalExpression(conditional) => conditional.test.span() == child_span,
        AstKind::TemplateLiteral(_) => {
            !matches!(ctx.nodes().parent_kind(parent.id()), AstKind::TaggedTemplateExpression(_))
        }
        AstKind::StaticMemberExpression(member) => {
            member.object.span() == child_span
                && !matches!(member.property.name.as_str(), "value" | "effect")
        }
        AstKind::ComputedMemberExpression(member) => {
            member.object.span() == child_span
                && literal_element_name(&member.expression)
                    .is_some_and(|name| !matches!(name.as_ref(), "value" | "effect"))
        }
        // A ref handed to `emit('name', ref)` is sent to the parent as a ref
        // object rather than as its value.
        AstKind::CallExpression(call) => {
            call.callee.span() != child_span
                && call.arguments.iter().any(|argument| argument.span() == child_span)
                && is_emit_call(parent, ctx)
        }
        _ => false,
    }
}

/// Whether `call_node` is an `emit('name', …)` — either the `<script setup>`
/// `const emit = defineEmits(…)` binding, or the Options API's
/// `setup(props, context)` / `setup(props, { emit })` parameter.
///
/// Upstream gates on the first argument being a string literal
/// (`getNameParamNode`), which is why `emit(dynamicName, someRef)` is left
/// alone.
fn is_emit_call(call_node: &AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    let AstKind::CallExpression(call) = call_node.kind() else { return false };
    if !call
        .arguments
        .first()
        .and_then(|argument| argument.as_expression())
        .is_some_and(|expression| matches!(expression, Expression::StringLiteral(_)))
    {
        return false;
    }

    match call.callee.get_inner_expression() {
        // `emit(…)` — either `defineEmits`' result, or a destructured
        // `setup(props, { emit })` parameter.
        Expression::Identifier(ident) => {
            let Some(symbol_id) = ctx.scoping().get_reference(ident.reference_id()).symbol_id()
            else {
                return false;
            };
            is_define_emits_binding(symbol_id, ctx)
                || is_setup_context_binding(symbol_id, ctx, true)
        }
        // `context.emit(…)` from `setup(props, context)`.
        Expression::StaticMemberExpression(member) => {
            if member.property.name != "emit" {
                return false;
            }
            let Expression::Identifier(object) = member.object.get_inner_expression() else {
                return false;
            };
            let Some(symbol_id) = ctx.scoping().get_reference(object.reference_id()).symbol_id()
            else {
                return false;
            };
            is_setup_context_binding(symbol_id, ctx, false)
        }
        _ => false,
    }
}

/// `const emit = defineEmits(…)` in a `<script setup>` block. Upstream
/// registers this under `defineScriptSetupVisitor`, hence the gate.
fn is_define_emits_binding(symbol_id: SymbolId, ctx: &LintContext<'_>) -> bool {
    if ctx.frameworks_options() != FrameworkOptions::VueSetup {
        return false;
    }
    let AstKind::VariableDeclarator(declarator) =
        ctx.nodes().get_node(ctx.scoping().symbol_declaration(symbol_id)).kind()
    else {
        return false;
    };
    declarator.init.as_ref().is_some_and(|init| {
        matches!(init.get_inner_expression(), Expression::CallExpression(call)
            if call.callee_name() == Some("defineEmits"))
    })
}

/// Whether `symbol_id` is the second parameter of a `setup(…)` function on a
/// Vue component options object — either the whole `context` object, or the
/// `emit` picked out of it by destructuring.
fn is_setup_context_binding(
    symbol_id: SymbolId,
    ctx: &LintContext<'_>,
    destructured_emit: bool,
) -> bool {
    // A plain `context` parameter declares its symbol on the `FormalParameter`
    // node itself, while a destructured `{ emit }` declares it further down.
    // `ancestors` starts at the parent, hence the explicit first element.
    let declaration = ctx.nodes().get_node(ctx.scoping().symbol_declaration(symbol_id));
    let Some(parameter) = std::iter::once(declaration)
        .chain(ctx.nodes().ancestors(declaration.id()))
        .find(|ancestor| matches!(ancestor.kind(), AstKind::FormalParameter(_)))
    else {
        return false;
    };
    let AstKind::FormalParameter(formal) = parameter.kind() else { return false };

    // Upstream's `skipDefaultParamValue`, then either `context` itself or the
    // `emit` property of `{ emit }`.
    let pattern = match &formal.pattern {
        BindingPattern::AssignmentPattern(assignment) => &assignment.left,
        pattern => pattern,
    };
    if destructured_emit {
        let BindingPattern::ObjectPattern(object) = pattern else { return false };
        if !object.properties.iter().any(|property| {
            property.key.static_name().is_some_and(|name| name == "emit")
                && binding_symbol(&property.value) == Some(symbol_id)
        }) {
            return false;
        }
    } else if binding_symbol(pattern) != Some(symbol_id) {
        return false;
    }

    let parameters = ctx.nodes().parent_node(parameter.id());
    let AstKind::FormalParameters(list) = parameters.kind() else { return false };
    // Second parameter only.
    if list.items.get(1).is_none_or(|second| second.span != formal.span) {
        return false;
    }
    is_setup_function_of_vue_component(parameters, ctx)
}

/// Whether the function owning these parameters is the `setup` property of an
/// object this linter recognises as Vue component options (`export default
/// {…}`, `defineComponent({…})`, `new Vue({…})`, …).
fn is_setup_function_of_vue_component(parameters: &AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    let function = ctx.nodes().parent_node(parameters.id());
    let function_span = match function.kind() {
        AstKind::Function(function) => function.span,
        AstKind::ArrowFunctionExpression(arrow) => arrow.span,
        _ => return false,
    };
    ctx.nodes().ancestors(function.id()).any(|ancestor| {
        let AstKind::ObjectExpression(object) = ancestor.kind() else { return false };
        find_property(object, "setup")
            .is_some_and(|property| property.value.span() == function_span)
            && is_vue_component_options_object(ancestor, ctx)
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoRefAsOperand;
    use crate::{rule::RuleMeta, tester::Tester};

    // Cases marked "upstream" are transcribed from eslint-plugin-vue's own
    // `tests/lib/rules/no-ref-as-operand.js` at tag v10.6.2 (29 valid, 18
    // invalid); npm ships no `tests/` directory, so it was fetched from
    // GitHub. The rest close gaps that upstream's suite leaves open.
    #[test]
    fn test() {
        let js = || Some(PathBuf::from("test.js"));
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            // upstream: the canonical correct form.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\nconsole.log(count.value)\ncount.value++",
                None,
                None,
                js(),
            ),
            // upstream: every operand position, read through `.value`.
            (
                "import { ref } from 'vue'
                 const count = ref(0)
                 if (count.value) {}
                 switch (count.value) {}
                 var a = -count.value
                 var b = +count.value
                 count.value++
                 count.value--
                 count.value + 1
                 1 - count.value
                 count.value || other
                 count.value && other
                 var c = count.value ? x : y",
                None,
                None,
                js(),
            ),
            // upstream: the alternate module specifier.
            (
                "<script>import { ref } from '@vue/composition-api'\nexport default { setup() { const count = ref(0); count.value++; return { count } } }</script>",
                None,
                None,
                vue(),
            ),
            // Nuxt's virtual module, which this linter already treats as vue.
            (
                "import { ref } from '#imports'\nconst count = ref(0)\ncount.value++",
                None,
                None,
                js(),
            ),
            // upstream: imported from somewhere that is not vue.
            ("import { ref } from 'unknown'\nconst count = ref(0)\ncount++", None, None, js()),
            // upstream: the factory is aliased but never called.
            ("import { ref } from 'vue'\nconst count = ref\ncount++", None, None, js()),
            // upstream: `foo` is the consequent statement, not the `if` test —
            // ESLint's `IfStatement > Identifier` is a *direct* child.
            ("import { ref } from 'vue'\nconst foo = ref(true)\nif (bar) foo", None, None, js()),
            // upstream: right operand of a logical, and a `let` on the left.
            (
                "import { ref } from 'vue'
                 const foo = ref(true)
                 var a = other || foo
                 var b = other && foo
                 let bar = ref(true)
                 var c = bar || other",
                None,
                None,
                js(),
            ),
            // upstream: a conditional's branches, only its test is checked.
            (
                "import { ref } from 'vue'\nconst foo = ref(0)\nconst bar = ref(0)\nvar baz = x ? foo : bar",
                None,
                None,
                js(),
            ),
            // upstream: "probably wrong, but not checked by this rule" — only
            // `toRefs` unpacks an object pattern.
            ("import { ref } from 'vue'\nconst {value} = ref(0)\nvalue++", None, None, js()),
            // upstream: an inner declaration shadows the ref.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\nfunction foo() { let count = 0; count++ }",
                None,
                None,
                js(),
            ),
            // upstream: the `=` right-hand side is not an operand use.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\nfoo = count\nconst bar = count",
                None,
                None,
                js(),
            ),
            // upstream: a computed property that is not statically known.
            (
                "<script>import { shallowRef } from 'vue'\nconst foo = shallowRef({})\nfoo[bar] = 123</script>",
                None,
                None,
                vue(),
            ),
            // upstream: `effect` is WritableComputedRef's own property.
            (
                "<script>import { shallowRef } from 'vue'\nconst foo = shallowRef({})\nconst isComp = foo.effect</script>",
                None,
                None,
                vue(),
            ),
            // upstream: assigned a ref after declaration, so `isRefInit` fails
            // and the binding itself is never reported.
            (
                "<script>import { ref } from 'vue'\nlet foo;\nif (!foo) { foo = ref(5); }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>import { ref } from 'vue'\nlet foo = undefined;\nif (!foo) { foo = ref(5); }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: a plain call argument is fine — only `emit()` is checked.
            (
                "<script>import { ref } from 'vue'\nconst foo = ref(0)\nfunc(foo)\nfunction func(foo) {}</script>",
                None,
                None,
                vue(),
            ),
            // upstream: a tagged template gets the raw parts, so it is exempt.
            (
                "<script>import { ref } from 'vue'\nconst foo = ref(0)\ntag`${foo}`\nfunction tag(arr, ...args) {}</script>",
                None,
                None,
                vue(),
            ),
            // upstream: `defineModel()`, both shapes, read through `.value`.
            (
                "<script setup>const model = defineModel();\nconsole.log(model.value);\nfunction update(v) { model.value = v; }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script setup>const [model, mod] = defineModel();\nconsole.log(model.value);\nfunction update(v) { model.value = v; }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: emitting the unwrapped value.
            (
                "<script setup>const emit = defineEmits(['test'])\nconst [model, mod] = defineModel();\nfunction update() { emit('test', model.value) }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: the Options API `setup(props, context)` emit forms.
            (
                "<script>import { ref, defineComponent } from 'vue'\nexport default defineComponent({ emits: ['inc'], setup(_, ctx) { const counter = ref(0); ctx.emit('inc', counter.value); return { counter } } })</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>import { ref, defineComponent } from 'vue'\nexport default defineComponent({ emits: ['inc'], setup(_, { emit }) { const counter = ref(0); emit('inc', counter.value, 'xxx'); return { counter } } })</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>import { ref, defineComponent } from 'vue'\nexport default defineComponent({ emits: ['inc'], setup(_, { emit }) { const counter = ref(0); emit('inc'); return { counter } } })</script>",
                None,
                None,
                vue(),
            ),
            // A `case` clause hangs off a `SwitchCase`, not the `SwitchStatement`.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\nswitch (x) { case count: break }",
                None,
                None,
                js(),
            ),
            // Destructuring assignment targets are `ArrayAssignmentTarget` /
            // `AssignmentTargetPropertyIdentifier`, not `AssignmentExpression`.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\n;[count] = arr\n;({ count } = obj)",
                None,
                None,
                js(),
            ),
            // A ref in callee position matches no selector.
            ("import { ref } from 'vue'\nconst count = ref(0)\ncount(0)", None, None, js()),
            // `toRefs` keeps its result object; only the destructured form is
            // tracked (documented deviation).
            (
                "import { toRefs } from 'vue'\nconst refs = toRefs(props)\nif (refs.foo) {}",
                None,
                None,
                js(),
            ),
            // An auto-imported name that the config does *not* declare as a
            // global is not the vue factory as far as either linter can tell.
            ("let count = ref(0)\ncount++", None, None, js()),
            // Assigning a ref into another binding stops that binding being a
            // ref *of its own*, because upstream re-registers its references
            // under the source's define chain and `isRefInit` then fails.
            // `defineModel` is processed after every factory, so it always wins.
            (
                "<script setup>import { ref } from 'vue'\nconst model = defineModel()\nlet b = ref(1)\nb = model\nb++</script>",
                None,
                None,
                vue(),
            ),
            // Same-factory: `a` is declared first, so `b`'s own registration
            // happens before `a` re-registers it, and `a` wins.
            (
                "import { ref } from 'vue'\nlet b = ref(1)\nconst a = ref(0)\nb = a\nb++",
                None,
                None,
                js(),
            ),
            // `computed` is processed after `ref`, so it wins regardless of
            // source order.
            (
                "import { ref, computed } from 'vue'\nlet b = ref(1)\nconst a = computed(() => 1)\nb = a\nb++",
                None,
                None,
                js(),
            ),
        ];

        let fail = vec![
            // upstream: the three canonical misuses.
            (
                "import { ref } from 'vue'\nlet count = ref(0)\ncount++\nconsole.log(count + 1)\nconsole.log(1 + count)",
                None,
                None,
                js(),
            ),
            // upstream: inside an Options API `setup()`, in an SFC — proves
            // the spans survive the `<script>` block offset.
            (
                "<script>import { ref } from 'vue'\nexport default { setup() { let count = ref(0); count++; console.log(count + 1); return { count } } }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: `if` test and `switch` discriminant.
            ("import { ref } from 'vue'\nconst foo = ref(true)\nif (foo) {}", None, None, js()),
            ("import { ref } from 'vue'\nconst foo = ref(true)\nswitch (foo) {}", None, None, js()),
            // upstream: all four unary operators the docs mention.
            (
                "import { ref } from 'vue'\nconst foo = ref(0)\nvar a = -foo\nvar b = +foo\nvar c = !foo\nvar d = ~foo",
                None,
                None,
                js(),
            ),
            // upstream: compound assignment, on both sides.
            (
                "import { ref } from 'vue'\nlet foo = ref(0)\nfoo += 1\nfoo -= 1\nbaz += foo\nbaz -= foo",
                None,
                None,
                js(),
            ),
            // upstream: logical left operand, `const` binding.
            (
                "import { ref } from 'vue'\nconst foo = ref(true)\nvar a = foo || other\nvar b = foo && other",
                None,
                None,
                js(),
            ),
            // upstream: conditional test.
            (
                "import { ref } from 'vue'\nlet foo = ref(true)\nvar a = foo ? x : y",
                None,
                None,
                js(),
            ),
            // upstream: every factory reports under its own name.
            (
                "<script>import { ref, computed, toRef, customRef, shallowRef } from 'vue'
                 let count = ref(0)
                 let cntcnt = computed(() => count.value + count.value)
                 const fooRef = toRef(state, 'foo')
                 const cref = customRef((track, trigger) => ({ get() {}, set(v) {} }))
                 const foo = shallowRef({})
                 count++
                 cntcnt++
                 const s = `${fooRef} : ${cref}`
                 const n = foo + 1</script>",
                None,
                None,
                vue(),
            ),
            // upstream: a member write on the ref itself.
            (
                "<script>import { shallowRef } from 'vue'\nconst foo = shallowRef({})\nfoo.bar = 123</script>",
                None,
                None,
                vue(),
            ),
            // upstream: optional chaining still puts the ref in object position.
            (
                "<script>import { ref } from 'vue'\nconst foo = ref(123)\nconst bar = foo?.bar</script>",
                None,
                None,
                vue(),
            ),
            // upstream: the alias case. `foo` is a ref but not reportable (it
            // was assigned, not initialized); `bar` copies it in its own
            // initializer and so *is* reportable. Exactly one report, on `bar`.
            (
                "<script>import { ref } from 'vue'\nlet foo = undefined;\nif (!foo) { foo = ref(5); }\nlet bar = foo;\nbar = 4;</script>",
                None,
                None,
                vue(),
            ),
            // upstream: `defineModel()` in a plain `<script>` — the macro is
            // resolved as a global, with no `<script setup>` gate.
            (
                "<script>let model = defineModel();\nfunction process() { if (model) console.log('foo') }\nfunction update(v) { model = v; }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: the array form of `defineModel()`.
            (
                "<script setup>let [model, mod] = defineModel();\nfunction process() { if (model) console.log('foo') }\nfunction update(v) { model = v; }</script>",
                None,
                None,
                vue(),
            ),
            // upstream: a ref handed to `emit()`, all three shapes.
            (
                "<script setup>import { ref } from 'vue'\nconst emits = defineEmits(['test'])\nconst count = ref(0)\nfunction update() { emits('test', count) }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>import { ref, defineComponent } from 'vue'\nexport default defineComponent({ emits: ['inc'], setup(_, ctx) { const counter = ref(0); ctx.emit('inc', counter); return { counter } } })</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>import { ref, defineComponent } from 'vue'\nexport default defineComponent({ emits: ['inc'], setup(_, { emit }) { const counter = ref(0); emit('inc', 'xxx', counter); return { counter } } })</script>",
                None,
                None,
                vue(),
            ),
            // upstream: auto-import, which needs the global to be declared —
            // exactly as upstream's own case sets `globals: { ref: 'readonly' }`.
            (
                "let count = ref(0)\ncount++\nconsole.log(count + 1)",
                None,
                Some(json!({ "globals": { "ref": "readonly" } })),
                js(),
            ),
            // The message names the *imported* factory, not the local alias.
            ("import { ref as r } from 'vue'\nconst c = r(0)\nc++", None, None, js()),
            // A namespace import of vue.
            ("import * as vue from 'vue'\nconst c = vue.ref(0)\nc++", None, None, js()),
            // `toRefs` destructuring, reported under its own name.
            (
                "import { toRefs } from 'vue'\nconst { foo } = toRefs(props)\nif (foo) {}",
                None,
                None,
                js(),
            ),
            // `UnaryExpression > Identifier` has no operator filter upstream,
            // so `typeof` and `void` are reported too.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\nif (typeof count === 'undefined') {}\nvoid count",
                None,
                None,
                js(),
            ),
            // Parentheses: espree has no paren node, so upstream matches
            // through them and so must this.
            (
                "import { ref } from 'vue'\nconst count = ref(0)\nif ((count)) {}\n;(count) || x",
                None,
                None,
                js(),
            ),
            // An alias of a ref inherits reportability, and the `const` gate
            // for the logical case follows the *alias*, not its source.
            (
                "import { ref } from 'vue'\nlet a = ref(0)\nconst b = a\nvar x = b || other",
                None,
                None,
                js(),
            ),
            // The mirror of the demotion pass cases: here `b`'s own
            // registration is the last one, so it survives.
            (
                "import { ref } from 'vue'\nconst a = ref(0)\nlet b = ref(1)\nb = a\nb++\na++",
                None,
                None,
                js(),
            ),
            (
                "import { ref, computed } from 'vue'\nlet b = computed(() => 1)\nconst a = ref(0)\nb = a\nb++",
                None,
                None,
                js(),
            ),
            // Demotion silences `b` but leaves the `defineModel` ref itself
            // reportable.
            (
                "<script setup>import { ref } from 'vue'\nconst model = defineModel()\nlet b = ref(1)\nb = model\nb++\nmodel++</script>",
                None,
                None,
                vue(),
            ),
            // Import-specifier order decides the winner. Same code as the
            // "`computed` demotes `ref`" pass case above with the two
            // specifiers swapped: `ref` is processed last, so `c` keeps its
            // own ref and both lines report.
            (
                "import { computed, ref } from 'vue'\nconst a = computed(() => 1)\nconst c = ref('')\nc = a\nc++",
                None,
                None,
                js(),
            ),
            // `model = search` propagates through the `search` identifier
            // first, so when the later `defineModel` pass reaches the same
            // identifier upstream's `_processedIds` makes it a no-op and
            // `search` keeps its own `computed()`. Without that set, the
            // `search = model` line would silence both `search` reports.
            (
                "<script setup>import { computed } from 'vue'\nconst model = defineModel()\nconst search = computed({ get: () => 1, set: () => {} })\nfunction go() {\n  model = search\n  search = model\n  if (search) {}\n}</script>",
                None,
                None,
                vue(),
            ),
        ];

        let fix = vec![
            (
                "import { ref } from 'vue'\nlet count = ref(0)\ncount++\nconsole.log(count + 1)\nconsole.log(1 + count)",
                "import { ref } from 'vue'\nlet count = ref(0)\ncount.value++\nconsole.log(count.value + 1)\nconsole.log(1 + count.value)",
                None,
                js(),
            ),
            (
                "<script>import { shallowRef } from 'vue'\nconst foo = shallowRef({})\nfoo.bar = 123</script>",
                "<script>import { shallowRef } from 'vue'\nconst foo = shallowRef({})\nfoo.value.bar = 123</script>",
                None,
                vue(),
            ),
            (
                "<script>import { ref } from 'vue'\nconst foo = ref(123)\nconst bar = foo?.bar</script>",
                "<script>import { ref } from 'vue'\nconst foo = ref(123)\nconst bar = foo.value?.bar</script>",
                None,
                vue(),
            ),
            (
                "<script>import { ref } from 'vue'\nlet foo = undefined;\nif (!foo) { foo = ref(5); }\nlet bar = foo;\nbar = 4;</script>",
                "<script>import { ref } from 'vue'\nlet foo = undefined;\nif (!foo) { foo = ref(5); }\nlet bar = foo;\nbar.value = 4;</script>",
                None,
                vue(),
            ),
        ];

        Tester::new(NoRefAsOperand::NAME, NoRefAsOperand::PLUGIN, pass, fail)
            .expect_fix(fix)
            .test_and_snapshot();
    }
}
