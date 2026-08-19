use oxc_ast::{
    AstKind,
    ast::{
        AssignmentTarget, BindingPattern, Expression, IdentifierReference, LabeledStatement,
        MemberExpression,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::AstNodes;
use oxc_span::{GetSpan, Span};
use oxc_syntax::{node::NodeId, symbol::SymbolId};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    context::{ContextHost, LintContext},
    module_record::ImportImportName,
    rule::Rule,
};

fn infinite_reactive_loop_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Possibly it may occur an infinite reactive loop.").with_label(span)
}

fn infinite_reactive_loop_call_diagnostic(span: Span, variable_name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Possibly it may occur an infinite reactive loop because this function may update `{variable_name}`."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct InfiniteReactiveLoop;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports reactive statements (`$:`) that update one of their own
    /// dependencies from a different microtask: after an `await`, inside a
    /// `Promise#then`/`Promise#catch` callback, inside `setTimeout`,
    /// `setInterval`, or `queueMicrotask` callbacks, or after awaiting
    /// Svelte's `tick()`. Functions called from the reactive statement are
    /// checked as well.
    ///
    /// ### Why is this bad?
    ///
    /// The Svelte runtime prevents a reactive statement from re-triggering
    /// itself within the same microtask, but not across different
    /// microtasks. If a reactive statement asynchronously updates a variable
    /// it also depends on, every update re-runs the statement and schedules
    /// yet another update, producing an infinite reactive loop.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let count = 0;
    ///
    ///   $: (async () => {
    ///     console.log(count);
    ///     await tick();
    ///     count = count + 1;
    ///   })();
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let count = 0;
    ///
    ///   $: (async () => {
    ///     count = count + 1; // still in the same microtask
    ///     await new Promise((resolve) => {});
    ///   })();
    /// </script>
    /// ```
    InfiniteReactiveLoop,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow reactive statements that cause an infinite loop.",
);

impl Rule for InfiniteReactiveLoop {
    fn run_once(&self, ctx: &LintContext) {
        let nodes = ctx.nodes();

        // Svelte turns top-level `$:` labeled statements into reactive
        // statements.
        let reactive_statements: Vec<(NodeId, &LabeledStatement)> = nodes
            .iter()
            .filter_map(|node| {
                if let AstKind::LabeledStatement(labeled) = node.kind()
                    && labeled.label.name == "$"
                    && matches!(nodes.parent_kind(node.id()), AstKind::Program(_))
                {
                    Some((node.id(), labeled))
                } else {
                    None
                }
            })
            .collect();
        if reactive_statements.is_empty() {
            return;
        }

        let tracked_calls = collect_tracked_calls(ctx);
        let reactive_refs = collect_reactive_variable_references(ctx);

        // Child lists (in document order) so the verifier can re-create
        // upstream's `traverseNodes` enter/leave traversal.
        let mut children: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
        for (node_id, _) in nodes.iter_enumerated() {
            let parent_id = nodes.parent_id(node_id);
            if parent_id != node_id {
                children.entry(parent_id).or_default().push(node_id);
            }
        }

        for (stmt_id, labeled) in reactive_statements {
            // Variables the reactive statement depends on: every reactive
            // variable referenced anywhere inside the `$:` statement.
            let dep_names: FxHashSet<&str> = reactive_refs
                .iter()
                .filter_map(|&ref_id| {
                    let kind = nodes.kind(ref_id);
                    if labeled.span.contains_inclusive(kind.span())
                        && let AstKind::IdentifierReference(ident) = kind
                    {
                        return Some(ident.name.as_str());
                    }
                    None
                })
                .collect();

            let Some(body_id) = children
                .get(&stmt_id)
                .and_then(|stmt_children| {
                    stmt_children
                        .iter()
                        .find(|&&child| !matches!(nodes.kind(child), AstKind::LabelIdentifier(_)))
                })
                .copied()
            else {
                continue;
            };

            let mut verifier = Verifier {
                ctx,
                children: &children,
                tracked_calls: &tracked_calls,
                reactive_refs: &reactive_refs,
                dep_names: &dep_names,
                processed: FxHashSet::default(),
                call_func_idents: Vec::new(),
            };
            verifier.verify(body_id, true, /* root_in_reactive */ true);
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Traversal state for one `verifyInternal` frame (one verified subtree).
struct Frame {
    /// Whether the current position still runs in the same microtask as the
    /// reactive statement.
    same_micro_task: bool,
    /// Nodes that switched `same_micro_task` off when entered; leaving them
    /// switches it back on, mirroring upstream's
    /// `differentMicroTaskEnterNodes`.
    diff_micro_task_nodes: Vec<NodeId>,
    /// Whether this frame verifies the reactive statement's body itself (as
    /// opposed to the body of a function it calls).
    root_in_reactive: bool,
}

struct Verifier<'a, 'b> {
    ctx: &'b LintContext<'a>,
    children: &'b FxHashMap<NodeId, Vec<NodeId>>,
    /// Call expressions of `tick` (from `svelte`) and of the global
    /// `setTimeout`, `setInterval`, and `queueMicrotask` (aliases included).
    tracked_calls: &'b FxHashSet<NodeId>,
    /// References to reactive variables: top-level variables plus `$store`
    /// auto-subscriptions, excluding direct function calls.
    reactive_refs: &'b FxHashSet<NodeId>,
    /// Names of the reactive variables the current `$:` statement uses.
    dep_names: &'b FxHashSet<&'a str>,
    /// Verified subtree roots, to avoid infinite recursion on recursive
    /// functions.
    processed: FxHashSet<NodeId>,
    /// Identifiers of the function-call chain that led to the current frame.
    call_func_idents: Vec<NodeId>,
}

impl Verifier<'_, '_> {
    fn verify(&mut self, root: NodeId, p_is_same_task: bool, root_in_reactive: bool) {
        if !self.processed.insert(root) {
            return;
        }
        let mut frame = Frame {
            same_micro_task: p_is_same_task,
            diff_micro_task_nodes: Vec::new(),
            root_in_reactive,
        };
        self.walk(root, &mut frame);
    }

    fn walk(&mut self, node_id: NodeId, frame: &mut Frame) {
        self.enter(node_id, frame);
        let children = self.children;
        if let Some(child_ids) = children.get(&node_id) {
            for &child_id in child_ids {
                self.walk(child_id, frame);
            }
        }
        self.leave(node_id, frame);
    }

    fn enter(&mut self, node_id: NodeId, frame: &mut Frame) {
        let nodes = self.ctx.nodes();
        let kind = nodes.kind(node_id);

        // An arrow function passed to `.then()` / `.catch()` runs in a later
        // microtask.
        if matches!(kind, AstKind::ArrowFunctionExpression(_))
            && is_then_or_catch_argument(nodes, node_id)
        {
            frame.diff_micro_task_nodes.push(node_id);
            frame.same_micro_task = false;
        }

        // Anything inside a `tick()`/`setTimeout()`/`setInterval()`/
        // `queueMicrotask()` call runs in a later (micro)task.
        if !self.tracked_calls.is_empty()
            && nodes.ancestor_ids(node_id).any(|ancestor| self.tracked_calls.contains(&ancestor))
        {
            frame.diff_micro_task_nodes.push(node_id);
            frame.same_micro_task = false;
        }

        // The left side of `x = await ...` is assigned in a later microtask.
        if let AstKind::AssignmentExpression(assignment) = nodes.parent_kind(node_id)
            && matches!(assignment.right.get_inner_expression(), Expression::AwaitExpression(_))
            && assignment.left.span() == kind.span()
        {
            frame.diff_micro_task_nodes.push(node_id);
            frame.same_micro_task = false;
        }

        if let AstKind::IdentifierReference(ident) = kind {
            // Traverse the body of a called function, carrying the current
            // microtask state into it.
            if is_function_call(nodes, node_id, ident)
                && let Some(body_id) = function_declaration_body(self.ctx, ident)
            {
                self.call_func_idents.push(node_id);
                let same_micro_task = frame.same_micro_task;
                self.verify(body_id, same_micro_task, /* root_in_reactive */ false);
                self.call_func_idents.pop();
            }

            if !frame.same_micro_task
                && self.reactive_refs.contains(&node_id)
                && self.dep_names.contains(ident.name.as_str())
                && is_assignment_to(nodes, node_id, ident)
            {
                self.ctx.diagnostic(infinite_reactive_loop_diagnostic(ident.span));
                for &call_ident_id in &self.call_func_idents {
                    self.ctx.diagnostic(infinite_reactive_loop_call_diagnostic(
                        nodes.kind(call_ident_id).span(),
                        ident.name.as_str(),
                    ));
                }
            }
        }
    }

    fn leave(&self, node_id: NodeId, frame: &mut Frame) {
        let nodes = self.ctx.nodes();

        if matches!(nodes.kind(node_id), AstKind::AwaitExpression(_)) {
            // In the reactive statement itself, `await` only crosses a
            // microtask boundary when used directly (not inside a locally
            // declared async function). In called function bodies it always
            // does.
            if !frame.root_in_reactive || !is_inside_of_function(nodes, node_id) {
                frame.same_micro_task = false;
            }
        }

        if frame.diff_micro_task_nodes.contains(&node_id) {
            frame.same_micro_task = true;
        }
    }
}

/// The nearest non-parenthesis ancestor, i.e. the parent an ESTree-based
/// implementation would see.
fn estree_parent_id(nodes: &AstNodes, node_id: NodeId) -> Option<NodeId> {
    nodes
        .ancestor_ids(node_id)
        .find(|&id| !matches!(nodes.kind(id), AstKind::ParenthesizedExpression(_)))
}

fn estree_parent_kind<'a>(nodes: &AstNodes<'a>, node_id: NodeId) -> Option<(NodeId, AstKind<'a>)> {
    estree_parent_id(nodes, node_id).map(|id| (id, nodes.kind(id)))
}

/// `foo(...)` where `node_id` is an identifier named like the callee
/// (mirrors upstream's `isFunctionCall`).
fn is_function_call(nodes: &AstNodes, node_id: NodeId, ident: &IdentifierReference) -> bool {
    let Some((_, AstKind::CallExpression(call))) = estree_parent_kind(nodes, node_id) else {
        return false;
    };
    matches!(
        call.callee.get_inner_expression(),
        Expression::Identifier(callee) if callee.name == ident.name
    )
}

/// `a = ...` / `a += ...` where `node_id` is `a`, or `a.b = ...` where
/// `node_id` is `a` (mirrors upstream's `isNodeForAssign`).
fn is_assignment_to(nodes: &AstNodes, node_id: NodeId, ident: &IdentifierReference) -> bool {
    let Some((parent_id, parent_kind)) = estree_parent_kind(nodes, node_id) else {
        return false;
    };
    match parent_kind {
        AstKind::AssignmentExpression(assignment) => matches!(
            &assignment.left,
            AssignmentTarget::AssignmentTargetIdentifier(target) if target.name == ident.name
        ),
        AstKind::StaticMemberExpression(_)
        | AstKind::ComputedMemberExpression(_)
        | AstKind::PrivateFieldExpression(_) => {
            let Some((_, AstKind::AssignmentExpression(assignment))) =
                estree_parent_kind(nodes, parent_id)
            else {
                return false;
            };
            let Some(member) = assignment.left.as_member_expression() else {
                return false;
            };
            matches!(
                member.object().get_inner_expression(),
                Expression::Identifier(object) if object.name == ident.name
            )
        }
        _ => false,
    }
}

/// An arrow function directly passed to `promise.then(...)` or
/// `promise.catch(...)` (mirrors upstream's `isPromiseThenOrCatchBody`).
fn is_then_or_catch_argument(nodes: &AstNodes, node_id: NodeId) -> bool {
    let Some((_, AstKind::CallExpression(call))) = estree_parent_kind(nodes, node_id) else {
        return false;
    };
    let Some(member) = call.callee.get_inner_expression().as_member_expression() else {
        return false;
    };
    // Upstream only matches non-computed `.then` / `.catch` accesses.
    matches!(
        member,
        MemberExpression::StaticMemberExpression(static_member)
            if matches!(static_member.property.name.as_str(), "then" | "catch")
    )
}

/// Whether the awaited expression at `node_id` is wrapped in an async
/// function declaration or an async function assigned to a variable
/// (mirrors upstream's `isInsideOfFunction`).
fn is_inside_of_function(nodes: &AstNodes, node_id: NodeId) -> bool {
    nodes.ancestor_kinds(node_id).any(|kind| match kind {
        AstKind::Function(func) => func.is_declaration() && func.r#async,
        AstKind::VariableDeclarator(declarator) => {
            matches!(
                declarator.init.as_ref().map(Expression::get_inner_expression),
                Some(Expression::FunctionExpression(func)) if func.r#async
            ) || matches!(
                declarator.init.as_ref().map(Expression::get_inner_expression),
                Some(Expression::ArrowFunctionExpression(arrow)) if arrow.r#async
            )
        }
        _ => false,
    })
}

/// Resolve an identifier used as `foo()` to the body of the function it
/// names: a function declaration, or a variable initialized with a function
/// or arrow expression (mirrors upstream's `getFunctionDeclarationNode`).
fn function_declaration_body(ctx: &LintContext, ident: &IdentifierReference) -> Option<NodeId> {
    let symbol_id = ctx.scoping().get_reference(ident.reference_id()).symbol_id()?;
    let declaration = ctx.nodes().get_node(ctx.scoping().symbol_declaration(symbol_id));
    match declaration.kind() {
        AstKind::Function(func) if func.is_declaration() => {
            func.body.as_ref().map(|body| body.node_id())
        }
        AstKind::VariableDeclarator(declarator) => {
            match declarator.init.as_ref().map(Expression::get_inner_expression) {
                Some(Expression::FunctionExpression(func)) => {
                    func.body.as_ref().map(|body| body.node_id())
                }
                Some(Expression::ArrowFunctionExpression(arrow)) => Some(arrow.body.node_id()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Collect the call expressions of `tick` imported from `svelte` and of the
/// global timer/microtask functions, following `const alias = fn;`
/// re-assignments like eslint-utils' `ReferenceTracker`.
fn collect_tracked_calls(ctx: &LintContext) -> FxHashSet<NodeId> {
    let scoping = ctx.scoping();
    let nodes = ctx.nodes();

    let mut calls: FxHashSet<NodeId> = FxHashSet::default();
    let mut queue: Vec<SymbolId> = Vec::new();

    for entry in &ctx.module_record().import_entries {
        if entry.module_request.name() != "svelte" {
            continue;
        }
        match &entry.import_name {
            ImportImportName::Name(name) if name.name() == "tick" => {
                if let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into()) {
                    queue.push(symbol_id);
                }
            }
            ImportImportName::NamespaceObject => {
                let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                else {
                    continue;
                };
                // Looking for `ns.tick`.
                for reference in scoping.get_resolved_references(symbol_id) {
                    let ident_id = reference.node_id();
                    let Some((member_id, AstKind::StaticMemberExpression(member))) =
                        estree_parent_kind(nodes, ident_id)
                    else {
                        continue;
                    };
                    if member.property.name == "tick"
                        && member.object.get_inner_expression().span()
                            == nodes.kind(ident_id).span()
                    {
                        process_callable_occurrence(ctx, member_id, &mut calls, &mut queue);
                    }
                }
            }
            _ => {}
        }
    }

    for global_name in ["setTimeout", "setInterval", "queueMicrotask"] {
        if let Some(reference_ids) = scoping.root_unresolved_references().get(global_name) {
            for &reference_id in reference_ids {
                let node_id = scoping.get_reference(reference_id).node_id();
                process_callable_occurrence(ctx, node_id, &mut calls, &mut queue);
            }
        }
    }

    let mut seen: FxHashSet<SymbolId> = FxHashSet::default();
    while let Some(symbol_id) = queue.pop() {
        if !seen.insert(symbol_id) {
            continue;
        }
        let reference_node_ids: Vec<NodeId> = scoping
            .get_resolved_references(symbol_id)
            .map(oxc_semantic::Reference::node_id)
            .collect();
        for node_id in reference_node_ids {
            process_callable_occurrence(ctx, node_id, &mut calls, &mut queue);
        }
    }

    calls
}

/// One occurrence of a tracked callable: record direct calls and follow
/// `const alias = callable;` declarations.
fn process_callable_occurrence(
    ctx: &LintContext,
    node_id: NodeId,
    calls: &mut FxHashSet<NodeId>,
    queue: &mut Vec<SymbolId>,
) {
    let nodes = ctx.nodes();
    let span = nodes.kind(node_id).span();
    let Some((parent_id, parent_kind)) = estree_parent_kind(nodes, node_id) else {
        return;
    };
    match parent_kind {
        AstKind::CallExpression(call) if call.callee.get_inner_expression().span() == span => {
            calls.insert(parent_id);
        }
        AstKind::VariableDeclarator(declarator) => {
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && declarator
                    .init
                    .as_ref()
                    .is_some_and(|init| init.get_inner_expression().span() == span)
            {
                queue.push(binding.symbol_id());
            }
        }
        _ => {}
    }
}

/// All references to reactive variables: references to variables declared in
/// the top-level scope, plus `$store` auto-subscription references (which the
/// svelte parser would declare at the top level). Identifiers that are direct
/// function calls are excluded, mirroring upstream's
/// `getReactiveVariableReferences`.
fn collect_reactive_variable_references(ctx: &LintContext) -> FxHashSet<NodeId> {
    let scoping = ctx.scoping();
    let nodes = ctx.nodes();
    let mut reactive_refs: FxHashSet<NodeId> = FxHashSet::default();

    let is_reactive_ref = |node_id: NodeId| {
        let AstKind::IdentifierReference(ident) = nodes.kind(node_id) else {
            return false;
        };
        !is_function_call(nodes, node_id, ident)
    };

    for &symbol_id in scoping.get_bindings(scoping.root_scope_id()).values() {
        for reference in scoping.get_resolved_references(symbol_id) {
            if is_reactive_ref(reference.node_id()) {
                reactive_refs.insert(reference.node_id());
            }
        }
    }

    for (name, reference_ids) in scoping.root_unresolved_references() {
        if name.starts_with('$') {
            for &reference_id in reference_ids {
                let node_id = scoping.get_reference(reference_id).node_id();
                if is_reactive_ref(node_id) {
                    reactive_refs.insert(node_id);
                }
            }
        }
    }

    reactive_refs
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        (
            "<script>
	let a = 0;

	$: (async () => {
		a = a + 1;
		a += 1;
		await new Promise((resolve) => {});
	})();

	$: (async () => {
		let a = 0;
		await new Promise((resolve) => {
			setTimeout(() => {
				a = a + 1;
				a += 1;
			}, 100);
		});
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	const func = () => {
		setTimeout(() => {
			a = a + 1;
		}, 100);
	};
	$: func();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: (async () => {
		await doSomething((a += 1));
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let list = [0];

	$: (async () => {
		await doSomething();
		list.push(list.length);
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { store } from './store.js';

	const doSomething = () => {
		$store += 1;
	};

	$: (async () => {
		await doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let obj = { a: 0 };

	const doSomething = async () => {
		obj.a += 1;
		await fetch();
	};

	$: (async () => {
		obj.a += 1;
		await doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let obj = { a: 0 };

	const doSomething = async () => {
		obj.a += 1;
		await fetch();
	};

	$: (async () => {
		const doSomething = async () => {
			await fetch();
		};
		obj.a += 1;
		await doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	const func = () => {
		return {
			then: (fun) => fun(),
			catch: (fun) => fun()
		};
	};

	$: (() => {
		// func().then / func().catch are not verified here, like upstream
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Recursive function references must not loop forever.
        (
            "<script>
	$: {
		const foo = (recurse) => (recurse ? foo(false) : undefined);
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Plain synchronous updates stay in the same microtask.
        (
            "<script>
	let a = 0;
	$: a = a + 1;
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Updated variable is not a dependency of this reactive statement.
        (
            "<script>
	let a = 0;
	let c = 0;

	const update = () => {
		setTimeout(() => {
			c += 1;
		}, 100);
	};

	$: update(a);
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        // await
        (
            "<script>
	let a = 0;

	$: (async () => {
		a = a + 1;
		await doSomething();
		a = a + 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        // function calls
        (
            "<script>
	const fetch = () => new Promise((resolve) => setTimeout(resolve, 100));
	let a = 0;

	$: fetch().then(() => {
		a = a + 1;
	});
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;
	const fetch = async () => {
		await new Promise((resolve) => setTimeout(resolve, 100));
	};
	const doSomething = () => {
		fetch().then(() => {
			a += 1;
		});
	};

	$: (async () => {
		console.log(a);
		await doSomething();
	})();

	$: (async () => {
		// should not report here
		await doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;
	const fetch = async () => {
		await new Promise((resolve) => setTimeout(resolve, 100));
	};
	const doSomething = async () => {
		await fetch();
		a += 1;
	};

	$: (async () => {
		console.log(a);
		await doSomething();
	})();

	$: (async () => {
		// should not report here
		await doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { count } from './store.js';
	let a = 0;
	const doSomething = async () => {
		await fetchFromServer();
		a += 1;
		$count += 1;
	};

	const doSomething2 = () => {
		a += 1;
		$count += 1;
	};

	$: (async () => {
		console.log(a);
		await doSomething();
		doSomething2();
	})();

	$: (async () => {
		// should not report here
		await doSomething();
		doSomething2();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;
	let obj = { a: 0 };
	const doSomething = async () => {
		await fetchFromServer();
		a += 1;
		obj.a += 1;
	};

	$: (async () => {
		console.log(a);
		await doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let obj = { a: 0 };
	const doSomething = async () => {
		await fetchFromServer();
		obj.a += 1;
	};

	$: (async () => {
		await doSomething();
		obj.a += 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { store } from './store.js';

	const doSomething = () => {
		$store += 1;
	};

	$: (async () => {
		console.log($store);
		await fetch();
		doSomething();
	})();

	$: (async () => {
		// should not report here
		await fetch();
		doSomething();
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let foo = { obj: 1 };

	const doSomething = async () => {
		await fetchFromServer();
		foo.obj += 1;
	};

	$: (async () => {
		const obj = { a: 0 };
		console.log(obj);
		await doSomething();
		foo.obj += 1;
		obj.a = 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let obj = { a: 0 };

	const doSomething = async () => {
		await new Promise((resolve) => setTimeout(resolve, 100));
		return 1;
	};

	$: (async () => {
		obj.a += await doSomething((obj.a += 1));
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let obj = { a: 0 };

	const doSomething = (a, b) => {
		console.log({ a, b });
	};

	$: (async () => {
		doSomething((await 'a', (obj.a += 1)));
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: {
		a += 1;
		void (async () => {
			await new Promise((resolve) => setTimeout(resolve, 100));
			a += 1;
		})();
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // promises
        (
            "<script>
	let a = 0;

	$: (() => {
		Promise.resolve().then(() => {
			a = a + 1;
			a += 1;
		});

		Promise.resolve().catch(() => {
			a = a + 1;
		});
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: {
		a = a + 1;
		new Promise((resolve, reject) => {
			/** do something */
		})
			.then(() => {
				a = a + 1;
			})
			.catch(() => {
				a = a + 1;
			});
		a = a + 1;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: (() => {
		a = a + 1;
		Promise.all([])
			.then(() => {
				a = a + 1;
			})
			.catch(() => {
				a = a + 1;
			});
		a = a + 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: (async () => {
		a = a + 1;
		await Promise.resolve();
		a = a + 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: (() => {
		Promise.resolve()
			.catch(() => {
				a = a + 1;
			})
			.catch(() => {
				a = a + 1;
			});
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let a = 0;

	$: (() => {
		Promise.resolve()
			.catch(() => {
				a = a + 1;
			})
			.then(() => {
				a = a + 1;
			});
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        // queueMicrotask
        (
            "<script>
	const queueMicrotask2 = queueMicrotask;
	let a = 0;

	$: {
		queueMicrotask(() => {
			a = a + 1;
		});
	}

	$: {
		queueMicrotask2(() => {
			a = a + 1;
		});
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // setInterval
        (
            "<script>
	const setInterval2 = setInterval;
	let a = 0;

	$: {
		setInterval(() => {
			a = a + 1;
		});
	}

	$: {
		setInterval2(() => {
			a = a + 1;
		});
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // setTimeout
        (
            "<script>
	const setTimeout2 = setTimeout;
	let a = 0;

	function doSomething(fn) {
		fn();
	}

	$: {
		setTimeout(() => {
			a = a + 1;
		});
	}

	$: {
		setTimeout2(() => {
			a = a + 1;
		});

		doSomething(() => {
			a = a + 1;
		});
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { store } from './store.js';

	$: setTimeout(() => {
		$store += 1;
	}, 100);
</script>",
            None,
            None,
            svelte_path(),
        ),
        // tick
        (
            "<script>
	import { tick } from 'svelte';
	const tick2 = tick;
	let a = 0;

	$: {
		tick(() => {
			a = a + 1;
		});
	}

	$: {
		tick2(() => {
			a = a + 1;
		});
	}

	$: (async () => {
		await tick();
		a = a + 1;
	})();

	$: (async () => {
		await tick2();
		a = a + 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { tick as tick2 } from 'svelte';
	let a = 0;

	function tick(fn) {
		fn();
	}

	$: {
		tick(() => {
			a = a + 1;
		});
	}

	$: {
		tick2(() => {
			a = a + 1;
		});
	}

	$: (async () => {
		await tick();
		a = a + 1;
	})();

	$: (async () => {
		await tick2();
		a = a + 1;
	})();
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(InfiniteReactiveLoop::NAME, InfiniteReactiveLoop::PLUGIN, pass, fail)
        .test_and_snapshot();
}
