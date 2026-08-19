//! Locating `svelte/store` factory calls.
//!
//! Ports eslint-plugin-svelte's `extractStoreReferences`, which uses
//! eslint-utils' `ReferenceTracker` to follow the `writable` / `readable` /
//! `derived` imports through namespace access and local aliases.

use oxc_ast::{
    AstKind,
    ast::{BindingPattern, CallExpression, Expression},
};
use oxc_span::{GetSpan, Span};
use oxc_syntax::symbol::SymbolId;
use rustc_hash::FxHashSet;

use crate::{
    AstNode, ast_util::outermost_paren_parent, context::LintContext,
    module_record::ImportImportName,
};

/// The store factories exported by `svelte/store` that take a value or a
/// callback worth checking.
pub const SVELTE_STORE_FACTORIES: [&str; 3] = ["writable", "readable", "derived"];

/// Calls `visit` for every call of a `svelte/store` factory named in
/// `factories`, passing the call and the factory's original export name
/// (`"writable"`, not the local alias).
///
/// Follows namespace imports (`store.writable(…)`) and local aliases
/// (`const w = writable; w(…)`), like upstream's `ReferenceTracker`.
pub fn for_each_svelte_store_call<'a>(
    ctx: &LintContext<'a>,
    factories: &[&str],
    visit: &mut impl FnMut(&CallExpression<'a>, &'static str),
) {
    let scoping = ctx.scoping();
    // Symbols known to hold one of the tracked factories, with the export
    // name they resolve to.
    let mut queue: Vec<(SymbolId, &'static str)> = Vec::new();

    let canonical = |name: &str| -> Option<&'static str> {
        SVELTE_STORE_FACTORIES
            .iter()
            .find(|factory| **factory == name && factories.contains(factory))
            .copied()
    };

    for entry in &ctx.module_record().import_entries {
        if entry.module_request.name() != "svelte/store" {
            continue;
        }
        match &entry.import_name {
            ImportImportName::Name(name) => {
                if let Some(factory) = canonical(name.name())
                    && let Some(symbol_id) =
                        scoping.get_root_binding(entry.local_name.name().into())
                {
                    queue.push((symbol_id, factory));
                }
            }
            ImportImportName::NamespaceObject => {
                let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                else {
                    continue;
                };
                for reference in scoping.get_resolved_references(symbol_id) {
                    let ident_node = ctx.nodes().get_node(reference.node_id());
                    let Some(member_node) = outermost_paren_parent(ident_node, ctx.semantic())
                    else {
                        continue;
                    };
                    if let Some(factory) =
                        store_member_name(member_node.kind(), ident_node.kind().span())
                            .and_then(canonical)
                    {
                        process_occurrence(ctx, member_node, factory, &mut queue, visit);
                    }
                }
            }
            ImportImportName::Default(_) => {}
        }
    }

    let mut seen: FxHashSet<SymbolId> = FxHashSet::default();
    while let Some((symbol_id, factory)) = queue.pop() {
        if !seen.insert(symbol_id) {
            continue;
        }
        let reference_node_ids: Vec<_> = scoping
            .get_resolved_references(symbol_id)
            .map(oxc_semantic::Reference::node_id)
            .collect();
        for node_id in reference_node_ids {
            process_occurrence(ctx, ctx.nodes().get_node(node_id), factory, &mut queue, visit);
        }
    }
}

/// The factory name a member expression accesses on the object spanning
/// `object_span` (`ns.writable` → `"writable"`).
fn store_member_name(kind: AstKind<'_>, object_span: Span) -> Option<&str> {
    match kind {
        AstKind::StaticMemberExpression(member)
            if member.object.get_inner_expression().span() == object_span =>
        {
            Some(member.property.name.as_str())
        }
        AstKind::ComputedMemberExpression(member)
            if member.object.get_inner_expression().span() == object_span =>
        {
            match &member.expression {
                Expression::StringLiteral(name) => Some(name.value.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// One occurrence of a factory (an identifier reference or a namespace member
/// access): either it is called, or it is aliased into another variable which
/// is then tracked as well.
fn process_occurrence<'a>(
    ctx: &LintContext<'a>,
    node: &AstNode<'a>,
    factory: &'static str,
    queue: &mut Vec<(SymbolId, &'static str)>,
    visit: &mut impl FnMut(&CallExpression<'a>, &'static str),
) {
    let Some(parent) = outermost_paren_parent(node, ctx.semantic()) else {
        return;
    };
    match parent.kind() {
        AstKind::CallExpression(call)
            if call.callee.get_inner_expression().span() == node.kind().span() =>
        {
            visit(call, factory);
        }
        // `const alias = writable;` keeps the factory trackable.
        AstKind::VariableDeclarator(decl) => {
            if let BindingPattern::BindingIdentifier(id) = &decl.id
                && decl
                    .init
                    .as_ref()
                    .is_some_and(|init| init.get_inner_expression().span() == node.kind().span())
            {
                queue.push((id.symbol_id(), factory));
            }
        }
        _ => {}
    }
}
