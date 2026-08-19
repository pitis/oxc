use oxc_ast::{
    AstKind,
    ast::{Argument, BindingPattern, CallExpression, Expression},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use oxc_syntax::symbol::SymbolId;
use rustc_hash::FxHashSet;

use crate::{
    AstNode,
    ast_util::outermost_paren_parent,
    context::{ContextHost, LintContext},
    module_record::ImportImportName,
    rule::Rule,
};

fn no_store_async_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Do not pass async functions to svelte stores.").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoStoreAsync;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows passing async functions to the svelte store factories
    /// `writable`, `readable`, and `derived`.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte stores rely on the start/stop (or derive) function returning a
    /// cleanup callback synchronously. An async function always returns a
    /// `Promise` instead, so the store's auto-unsubscribing features break
    /// and the cleanup logic never runs.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { writable } from 'svelte/store';
    ///
    ///   const store = writable(false, async () => {});
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { writable } from 'svelte/store';
    ///
    ///   const store = writable(false, () => {});
    /// </script>
    /// ```
    NoStoreAsync,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow async functions in Svelte stores.",
);

const STORE_NAMES: [&str; 3] = ["writable", "readable", "derived"];

impl Rule for NoStoreAsync {
    fn run_once(&self, ctx: &LintContext) {
        let scoping = ctx.scoping();

        // Symbols known to hold one of the `svelte/store` factory functions,
        // like eslint-utils' `ReferenceTracker` in the upstream rule.
        let mut queue: Vec<SymbolId> = Vec::new();

        for entry in &ctx.module_record().import_entries {
            if entry.module_request.name() != "svelte/store" {
                continue;
            }
            match &entry.import_name {
                ImportImportName::Name(name) if STORE_NAMES.contains(&name.name()) => {
                    if let Some(symbol_id) =
                        scoping.get_root_binding(entry.local_name.name().into())
                    {
                        queue.push(symbol_id);
                    }
                }
                ImportImportName::NamespaceObject => {
                    let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                    else {
                        continue;
                    };
                    // Looking for `ns.writable`, `ns.readable`, `ns.derived`.
                    for reference in scoping.get_resolved_references(symbol_id) {
                        let ident_node = ctx.nodes().get_node(reference.node_id());
                        let Some(member_node) = outermost_paren_parent(ident_node, ctx.semantic())
                        else {
                            continue;
                        };
                        if is_store_member(member_node.kind(), ident_node.kind().span()) {
                            process_occurrence(ctx, member_node, &mut queue);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut seen: FxHashSet<SymbolId> = FxHashSet::default();
        while let Some(symbol_id) = queue.pop() {
            if !seen.insert(symbol_id) {
                continue;
            }
            let reference_node_ids: Vec<_> = scoping
                .get_resolved_references(symbol_id)
                .map(oxc_semantic::Reference::node_id)
                .collect();
            for node_id in reference_node_ids {
                process_occurrence(ctx, ctx.nodes().get_node(node_id), &mut queue);
            }
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Is `kind` a member expression accessing a store factory (`.writable`,
/// `.readable`, `.derived`) on the object spanning `object_span`?
fn is_store_member(kind: AstKind, object_span: Span) -> bool {
    match kind {
        AstKind::StaticMemberExpression(member) => {
            STORE_NAMES.contains(&member.property.name.as_str())
                && member.object.get_inner_expression().span() == object_span
        }
        AstKind::ComputedMemberExpression(member) => {
            matches!(
                &member.expression,
                Expression::StringLiteral(name) if STORE_NAMES.contains(&name.value.as_str())
            ) && member.object.get_inner_expression().span() == object_span
        }
        _ => false,
    }
}

/// Handle one occurrence of a store factory (an identifier reference or a
/// namespace member access): either it is called directly, or it is aliased
/// into another variable which is then tracked as well.
fn process_occurrence<'a>(ctx: &LintContext<'a>, node: &AstNode<'a>, queue: &mut Vec<SymbolId>) {
    let Some(parent) = outermost_paren_parent(node, ctx.semantic()) else {
        return;
    };
    match parent.kind() {
        AstKind::CallExpression(call)
            if call.callee.get_inner_expression().span() == node.kind().span() =>
        {
            check_store_call(ctx, call);
        }
        // `const alias = writable;` keeps the store factory trackable.
        AstKind::VariableDeclarator(decl) => {
            if let BindingPattern::BindingIdentifier(id) = &decl.id
                && decl
                    .init
                    .as_ref()
                    .is_some_and(|init| init.get_inner_expression().span() == node.kind().span())
            {
                queue.push(id.symbol_id());
            }
        }
        _ => {}
    }
}

/// Report if the store factory call receives an async function as its second
/// argument (the start/stop notifier or the derive callback).
fn check_store_call<'a>(ctx: &LintContext<'a>, call: &CallExpression<'a>) {
    let Some(argument) = call.arguments.get(1).and_then(Argument::as_expression) else {
        return;
    };
    let function_span = match argument.get_inner_expression() {
        Expression::ArrowFunctionExpression(func) if func.r#async => func.span,
        Expression::FunctionExpression(func) if func.r#async => func.span,
        _ => return,
    };
    // Like upstream, only the `async` keyword is highlighted.
    ctx.diagnostic(no_store_async_diagnostic(Span::sized(function_span.start, 5)));
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        (
            "<script>
	import { writable, readable, derived } from 'svelte/store';

	const w1 = writable(false, () => {
		/** do nothing */
	});
	const w2 = writable(false);
	const r1 = readable(false, () => {
		/** do nothing */
	});
	const r2 = readable(false);
	const d1 = derived(a1, ($a1) => {
		/** do nothing */
	});
	const d2 = derived(a1);
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Only the second argument is checked, mirroring upstream.
        (
            "<script>
	import { writable } from 'svelte/store';

	const w = writable(async () => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Same names from another module are ignored.
        (
            "<script>
	import { writable, readable, derived } from './my-store';

	const w = writable(false, async () => {});
	const r = readable(false, async () => {});
	const d = derived(a1, async ($a1) => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Local declarations shadow nothing; not the svelte store factory.
        (
            "<script>
	const writable = (a, b) => {};
	const w = writable(false, async () => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Async function passed elsewhere is fine.
        (
            "<script>
	import { writable } from 'svelte/store';

	setTimeout(async () => {
		writable(false, () => {});
	}, 100);
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<script>
	import { writable, readable, derived } from 'svelte/store';

	const w2 = writable(false, async () => {
		/** do nothing */
	});
	const r2 = readable(false, async () => {
		/** do nothing */
	});
	const d2 = derived(a1, async ($a1) => {
		/** do nothing */
	});
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import * as stores from 'svelte/store';

	const w2 = stores.writable(false, async () => {
		/** do nothing */
	});
	const r2 = stores.readable(false, async () => {
		/** do nothing */
	});
	const d2 = stores.derived(a1, async ($a1) => {
		/** do nothing */
	});
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { writable as A, readable as B, derived as C } from 'svelte/store';

	const w2 = A(false, async () => {
		/** do nothing */
	});
	const r2 = B(false, async () => {
		/** do nothing */
	});
	const d2 = C(a1, async ($a1) => {
		/** do nothing */
	});
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { writable, readable, derived } from 'svelte/store';

	const w2 = writable(false, async function () {
		/** do nothing */
	});
	const r2 = readable(false, async function () {
		/** do nothing */
	});
	const d2 = derived(a1, async function ($a1) {
		/** do nothing */
	});
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Aliased through a variable, like eslint-utils' ReferenceTracker.
        (
            "<script>
	import { writable } from 'svelte/store';

	const myWritable = writable;
	const w = myWritable(false, async () => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Computed member access on the namespace import.
        (
            "<script>
	import * as stores from 'svelte/store';

	const w = stores['writable'](false, async () => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(NoStoreAsync::NAME, NoStoreAsync::PLUGIN, pass, fail).test_and_snapshot();
}
