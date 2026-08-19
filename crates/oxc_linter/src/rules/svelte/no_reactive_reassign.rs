use oxc_ast::{
    AstKind,
    ast::{
        AssignmentOperator, AssignmentTarget, AssignmentTargetMaybeDefault, Expression, Statement,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::UnaryOperator;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::{DefaultRuleConfig, Rule},
};

fn assignment_to_reactive_value_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Assignment to reactive value '{name}'."))
        .with_help("The value is computed by a reactive statement and will be overwritten whenever the statement re-runs; update its dependencies instead.")
        .with_label(span)
}

fn assignment_to_property_of_reactive_value_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Assignment to property of reactive value '{name}'."))
        .with_help("The value is computed by a reactive statement and will be overwritten whenever the statement re-runs; update its dependencies instead.")
        .with_label(span)
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoReactiveReassign {
    /// Whether to also report reassignments to *properties* of reactive
    /// values (e.g. `reactiveValue.prop = 42`, `reactiveArray.push(42)`).
    /// Defaults to `true`.
    props: bool,
}

impl Default for NoReactiveReassign {
    fn default() -> Self {
        Self { props: true }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow reassigning reactive values.
    ///
    /// This rule only applies to Svelte 3/4 reactive statements (`$:`), and to
    /// Svelte 5 components which do not use runes.
    ///
    /// ### Why is this bad?
    ///
    /// A variable that is computed by a reactive statement (`$: value = ...`)
    /// is overwritten every time the reactive statement re-runs. Assigning to
    /// it (or mutating it) elsewhere is almost certainly a bug, because the
    /// assignment is silently lost on the next reactive update.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let value = 0;
    ///   $: reactiveValue = value * 2;
    ///
    ///   function handleClick() {
    ///     reactiveValue = value * 3;
    ///     reactiveValue++;
    ///   }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let value = 0;
    ///   $: reactiveValue = value * 2;
    ///
    ///   function handleClick() {
    ///     value++;
    ///   }
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// #### props
    ///
    /// `{ type: boolean, default: true }`
    ///
    /// When `false`, reassignments to properties of reactive values (e.g.
    /// `reactiveValue.prop = 42`) are not reported; only reassignments of the
    /// reactive value itself are.
    NoReactiveReassign,
    svelte,
    correctness,
    config = NoReactiveReassign,
    version = "1.80.0",
    short_description = "Disallow reassigning reactive values.",
);

// Ported from <https://github.com/sveltejs/eslint-plugin-svelte/blob/main/packages/eslint-plugin-svelte/src/rules/no-reactive-reassign.ts>
//
// A reactive value is a variable that is only "declared" by a top-level
// reactive assignment (`$: name = ...`, with plain `=`). Svelte injects the
// declaration at compile time, so in the extracted script such a name is an
// unresolved reference; if the variable is also declared with `let`/`var`,
// upstream treats it as "reactive-like" and does not check it — which matches
// resolving to a real binding here.
//
// Deviations from upstream:
// - Reassignments in the template (e.g. `bind:value={reactiveValue}` or
//   mutations inside template event handlers) cannot be seen here, because
//   only the `<script>` blocks of a `.svelte` file are analyzed.
// - Upstream only treats `$:` labels of the instance `<script>` as reactive
//   statements; top-level `$:` labels in `<script context="module">` are also
//   checked here (see `no_reactive_functions.rs`).
impl Rule for NoReactiveReassign {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::LabeledStatement(labeled) = node.kind() else {
            return;
        };
        // A Svelte reactive statement is a top-level `$:` labeled statement.
        if labeled.label.name != "$"
            || !matches!(ctx.nodes().parent_kind(node.id()), AstKind::Program(_))
        {
            return;
        }
        let Statement::ExpressionStatement(stmt) = &labeled.body else {
            return;
        };
        let Expression::AssignmentExpression(assign) = stmt.expression.without_parentheses() else {
            return;
        };
        if assign.operator != AssignmentOperator::Assign {
            return;
        }

        let mut names = Vec::new();
        collect_target_names(&assign.left, &mut names);
        let left_span = assign.left.span();

        let scoping = ctx.scoping();
        let root_scope = scoping.root_scope_id();
        for name in names {
            // `$store` assignment targets are store subscriptions, not
            // injected reactive declarations.
            if name.starts_with('$') {
                continue;
            }
            // A variable that is also declared with `let`/`var` is only
            // "reactive-like"; upstream does not check it.
            if scoping.get_binding(root_scope, name.into()).is_some() {
                continue;
            }
            let Some(reference_ids) = scoping.root_unresolved_references().get(name) else {
                continue;
            };
            for &reference_id in reference_ids {
                let reference = scoping.get_reference(reference_id);
                let id_span = ctx.semantic().reference_span(reference);
                // Skip the defining write of this reactive statement itself.
                if left_span.contains_inclusive(id_span) {
                    continue;
                }
                let Some((span, path_len)) =
                    get_reassign_span(ctx.nodes().get_node(reference.node_id()), ctx)
                else {
                    continue;
                };
                // Suppress property reassignments when `props` is `false`.
                if !self.props && path_len > 0 {
                    continue;
                }
                ctx.diagnostic(if path_len == 0 {
                    assignment_to_reactive_value_diagnostic(span, name)
                } else {
                    assignment_to_property_of_reactive_value_diagnostic(span, name)
                });
            }
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Array methods which mutate the array they are called on.
fn is_array_mutation_method(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "reverse"
            | "splice"
            | "sort"
            | "copyWithin"
            | "fill"
    )
}

/// Collects the names of the variables the reactive assignment declares,
/// i.e. every identifier bound by the left-hand side pattern.
fn collect_target_names<'a>(target: &AssignmentTarget<'a>, names: &mut Vec<&'a str>) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => names.push(ident.name.as_str()),
        AssignmentTarget::ArrayAssignmentTarget(array) => {
            for element in array.elements.iter().flatten() {
                collect_maybe_default_names(element, names);
            }
            if let Some(rest) = &array.rest {
                collect_target_names(&rest.target, names);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(object) => {
            for property in &object.properties {
                match property {
                    oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                        prop,
                    ) => names.push(prop.binding.name.as_str()),
                    oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(
                        prop,
                    ) => collect_maybe_default_names(&prop.binding, names),
                }
            }
            if let Some(rest) = &object.rest {
                collect_target_names(&rest.target, names);
            }
        }
        // Member expressions (`$: obj.prop = ...`) do not declare variables.
        _ => {}
    }
}

fn collect_maybe_default_names<'a>(
    target: &AssignmentTargetMaybeDefault<'a>,
    names: &mut Vec<&'a str>,
) {
    if let AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) = target {
        collect_target_names(&with_default.binding, names);
    } else if let Some(target) = target.as_assignment_target() {
        collect_target_names(target, names);
    }
}

/// Walks up from a reference to the reactive value and returns the span of
/// the expression/statement that reassigns it (if any), together with the
/// number of member accesses between the reference and the reassignment
/// (`0` = the value itself is reassigned, `> 0` = one of its properties is).
///
/// This is a port of upstream's `CHECK_REASSIGN` table. `ChainExpression` and
/// `ParenthesizedExpression` nodes are passed through transparently, since
/// ESTree either wraps differently or has no such nodes.
fn get_reassign_span<'a>(start: &AstNode<'a>, ctx: &LintContext<'a>) -> Option<(Span, usize)> {
    let nodes = ctx.nodes();
    let mut node = start;
    // Property names of the member accesses walked through so far.
    let mut path: Vec<Option<&str>> = Vec::new();
    loop {
        let parent = nodes.parent_node(node.id());
        let node_span = node.kind().span();
        match parent.kind() {
            // e.g. foo++, foo--
            AstKind::UpdateExpression(update) => return Some((update.span, path.len())),
            // e.g. delete foo.prop
            AstKind::UnaryExpression(unary) if unary.operator == UnaryOperator::Delete => {
                return Some((unary.span, path.len()));
            }
            // e.g. foo = 42, foo += 42
            AstKind::AssignmentExpression(assign) if assign.left.span() == node_span => {
                return Some((assign.span, path.len()));
            }
            // e.g. for (foo in itr)
            AstKind::ForInStatement(for_in) if for_in.left.span() == node_span => {
                return Some((for_in.span, path.len()));
            }
            // e.g. for (foo of itr)
            AstKind::ForOfStatement(for_of) if for_of.left.span() == node_span => {
                return Some((for_of.span, path.len()));
            }
            // e.g. foo.push(42)
            AstKind::CallExpression(call) => {
                if !path.is_empty()
                    && call.callee.span() == node_span
                    && let Some(Some(method)) = path.last()
                    && is_array_mutation_method(method)
                {
                    path.pop();
                    return Some((call.span, path.len()));
                }
                return None;
            }
            AstKind::StaticMemberExpression(member) if member.object.span() == node_span => {
                path.push(Some(member.property.name.as_str()));
                node = parent;
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == node_span => {
                path.push(member.static_property_name().map(|name| name.as_str()));
                node = parent;
            }
            AstKind::PrivateFieldExpression(member) if member.object.span() == node_span => {
                path.push(None);
                node = parent;
            }
            // e.g. `foo?.prop`, `(foo).prop`, and `([foo] = obj)` /
            // `({ a } = obj)` — continue with the whole pattern
            AstKind::ChainExpression(_)
            | AstKind::ParenthesizedExpression(_)
            | AstKind::ArrayAssignmentTarget(_)
            | AstKind::ObjectAssignmentTarget(_) => {
                node = parent;
            }
            // e.g. `(test ? foo : bar).prop = 42`, but not `foo ? a : b`
            AstKind::ConditionalExpression(conditional) => {
                if conditional.test.span() == node_span {
                    return None;
                }
                node = parent;
            }
            // e.g. `({ a: foo } = obj)`, but not the computed key in
            // `({ [foo]: a } = obj)` and not defaults (`({ a: foo = 1 } = obj)`)
            AstKind::AssignmentTargetPropertyProperty(property)
                if property.binding.span() == node_span =>
            {
                node = nodes.parent_node(parent.id());
            }
            // e.g. `({ foo } = obj)`, but not `({ foo = 1 } = obj)`
            AstKind::AssignmentTargetPropertyIdentifier(property)
                if property.init.is_none() && property.binding.span == node_span =>
            {
                node = nodes.parent_node(parent.id());
            }
            // e.g. `({ ...foo } = obj)` — continue with the enclosing pattern
            AstKind::AssignmentTargetRest(rest) if rest.target.span() == node_span => {
                node = nodes.parent_node(parent.id());
            }
            _ => return None,
        }
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        (
            "<script>
	let value = 0;
	$: reactiveValue = value * 2;

	function handleClick() {
		/* GOOD */
		value++;
	}
</script>

<button on:click={handleClick}>Click Me</button>
{reactiveValue}",
            None,
            None,
            svelte_path(),
        ),
        // Non-mutating array methods.
        (
            "<script>
	let rerender = 0;
	let value = 'abc123';
	$: array1 = [...value];
	$: array2 = [...value];
	$: array3 = [...value];
	$: array4 = [...value];
	$: array5 = [...value];
	$: array6 = [...value];
	$: array7 = [...value];
	$: array8 = [...value];
	$: array9 = [...value];

	function handleClick() {
		[...array1].push(42);
		array2.slice().pop();
		array3.concat().shift();
		array4.filter(Boolean).unshift(42);
		array5.map((a) => a).reverse();
		array6.flat().splice(1, 1);
		array7.flatMap((a) => a).sort();
		Object.keys(array8).copyWithin(0, 3, 4);
		Object.values(array9).fill(42);
		rerender++;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Reading the reactive value is fine.
        (
            "<script>
	let value = 0;
	$: reactiveValue = value * 2;
	let foo;

	function handleClick() {
		foo = reactiveValue;
		console.log(foo);
		let bar = reactiveValue;
		console.log(bar);
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Used as a computed key in a destructuring assignment.
        (
            "<script>
	let value = 0;
	$: reactiveValue = value * 2;

	function handleClick() {
		let o = { 4: 42 };
		let a;
		({ [reactiveValue]: a } = o);
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // The reactive value is the object being iterated, not the target.
        (
            "<script>
	let value = 'a';
	$: reactiveValue = { key: value + value };
	let object = { key: 42 };

	function handleClick() {
		for (object.key in reactiveValue) {
			console.log(reactiveValue.key);
		}
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let value = 'a';
	$: reactiveValue = [value + value];
	let object = 2;

	function handleClick() {
		for (object of reactiveValue) {
			console.log(object);
		}
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Used as a computed member key of another object.
        (
            "<script>
	let object = { value: 0 };
	$: reactiveValue = { key: 'value', value: object.value * 2 };

	function handleClick() {
		object[reactiveValue.key] = 42;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Also declared with `let` — "reactive-like", not checked.
        (
            "<script>
	let value = 0;
	let reactiveLikeValue;
	$: reactiveLikeValue = value * 2;

	function handleClick() {
		reactiveLikeValue++;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Block-form reactive statements do not declare reactive values.
        (
            "<script>
	let value = 0;
	let reactiveLikeValue;
	$: {
		reactiveLikeValue = value * 2;
	}

	function handleClick() {
		reactiveLikeValue++;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let object = { value: 0 };
	$: reactiveValue = { value: object.value * 2 };

	function handleClick() {
		console.log(typeof reactiveValue.value);
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let value = 0;
	$: reactiveFn = value % 2 ? () => console.log('odd') : () => console.log('even');
</script>

<button on:click={reactiveFn} />",
            None,
            None,
            svelte_path(),
        ),
        // Only the test of a conditional — no reassignment.
        (
            "<script>
	let object = { value: 0 };
	$: reactiveValue = { value: object.value * 2 };

	function handleClick() {
		let a = {};
		(reactiveValue ? a : {}).value = 42;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Compound reactive assignments do not declare reactive values.
        (
            "<script>
	let foo;
	$: foo += 1;

	function handleClick() {
		foo = 2;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Assignments to a shadowing local variable are fine.
        (
            "<script>
	let value = 0;
	$: reactiveValue = value * 2;

	function handleClick() {
		let reactiveValue = 0;
		reactiveValue = 1;
		console.log(reactiveValue);
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<script>
	let value = 0;
	$: reactiveValue = value * 2;

	function handleClick() {
		/* BAD */
		reactiveValue = value * 3;
		reactiveValue++;
	}
</script>

{reactiveValue}",
            None,
            None,
            svelte_path(),
        ),
        // Mutating array methods.
        (
            "<script>
	let value = 'abc123';
	$: array1 = [...value];
	$: array2 = [...value];
	$: array3 = [...value];
	$: array4 = [...value];
	$: array5 = [...value];
	$: array6 = [...value];
	$: array7 = [...value];
	$: array8 = [...value];
	$: array9 = [...value];

	function handleClick() {
		array1.push(42);
		array2.pop();
		array3.shift();
		array4.unshift(42);
		array5.reverse();
		array6.splice(1, 1);
		array7.sort();
		array8.copyWithin(0, 3, 4);
		array9.fill(42);
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let object = { value: 0 };
	$: reactiveValue = { value: object.value * 2 };

	function handleClick() {
		let a = {};
		(a ? reactiveValue : {}).value = 42;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let object = { value: 0 };
	$: reactiveValue = { value: object.value * 2 };

	function handleClick() {
		delete object.value;
		delete reactiveValue.value;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let value = 0;
	$: reactiveValue = value * 2;
	$: reactiveObject = { value };
	$: reactiveArray = [value];

	function handleClick() {
		let o = { foo: 42 };
		({ foo: reactiveValue } = o);
		({ ...reactiveObject } = o);
		let a = [42];
		[reactiveValue] = a;
		[...reactiveArray] = a;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let value = 'a';
	$: reactiveValue = { key: value + value };
	let object = { key: 42 };

	function handleClick() {
		for (reactiveValue.key in object) {
			console.log(reactiveValue.key);
		}
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let value = 'a';
	$: reactiveValue = { key: value + value };
	let object = [42];

	function handleClick() {
		for (reactiveValue.key of object) {
			console.log(reactiveValue.key);
		}
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let object = { value: 0 };
	$: reactiveValue = { value: object.value * 2, foo: { bar: object.value * 2 } };

	function handleClick() {
		reactiveValue.value = 42;
		(reactiveValue?.foo).bar = 42;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	let value = 0;
	$: reactiveValue = { value: value * 2 };

	function handleClick() {
		/* GOOD */
		value++;
		/* BAD */
		reactiveValue.value++;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Destructured reactive declarations.
        (
            "<script>
	let value = { a: 0, b: 0 };
	$: ({ a, b } = value);

	function handleClick() {
		a = 1;
		b++;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    // NOTE: the upstream `props` option (`[{ "props": false }]` suppresses
    // property reassignments like `reactiveValue.value++` while still
    // reporting `reactiveValue++`) is implemented in `from_configuration`,
    // but option dispatch requires regenerated `RuleEnum::from_configuration`
    // arms (`cargo lintgen`), so those test cases cannot be enabled yet:
    //
    //   pass: ("<script>let v = 0; $: r = { v }; function f() { r.v++; }</script>",
    //          Some(serde_json::json!([{ "props": false }])), None, svelte_path())
    //   fail: ("<script>let v = 0; $: r = v * 2; function f() { r++; }</script>",
    //          Some(serde_json::json!([{ "props": false }])), None, svelte_path())

    Tester::new(NoReactiveReassign::NAME, NoReactiveReassign::PLUGIN, pass, fail)
        .test_and_snapshot();
}
