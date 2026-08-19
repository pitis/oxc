use rustc_hash::FxHashMap;

use oxc_ast::{
    AstKind,
    ast::{AssignmentOperator, Expression, Statement},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::SymbolId;
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::UnaryOperator;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_immutable_reactive_statements_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "This statement is not reactive because all variables referenced in the reactive statement are immutable.",
    )
    .with_help("If none of the referenced values ever change, use a regular statement instead of a reactive one.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoImmutableReactiveStatements;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow reactive statements that don't reference reactive values.
    ///
    /// This rule only applies to Svelte 3/4 reactive statements (`$:`), and to
    /// Svelte 5 components which do not use runes.
    ///
    /// ### Why is this bad?
    ///
    /// A reactive statement whose referenced variables can never change only
    /// runs once and is therefore not actually reactive. This is usually a
    /// mistake — either the statement should reference a mutable value, or it
    /// should be written as a regular statement.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   const immutable = 'hello';
    ///   $: computed = `${immutable} world`;
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let mutable = 'hello';
    ///   $: computed = `${mutable} world`;
    ///
    ///   function update() {
    ///     mutable = 'hi';
    ///   }
    /// </script>
    /// ```
    NoImmutableReactiveStatements,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow reactive statements that depend on no reactive values.",
);

// Ported from <https://github.com/sveltejs/eslint-plugin-svelte/blob/main/packages/eslint-plugin-svelte/src/rules/no-immutable-reactive-statements.ts>
//
// A variable is considered mutable when it is:
// - a store subscription or builtin Svelte variable (`$store`, `$$props`, ...),
// - a prop (`export let ...`),
// - a `let`/`var` (or a `const` without a literal/function initializer) that
//   is written to somewhere in the script, directly or through one of its
//   members (`x = 1`, `x.y = 1`, `x++`, `delete x.y`, ...).
// Imports, functions, classes, and `const`s initialized with a literal or a
// function expression are immutable. The statement is reported only when every
// referenced top-level variable is immutable; unknown (unresolved, non-global)
// references suppress the report.
//
// Deviations from upstream:
// - Mutations that only happen in the template (e.g. `bind:value={x}`,
//   `{#each ...}` bindings, or assignments inside template event handlers)
//   cannot be seen here, because only the `<script>` blocks of a `.svelte`
//   file are analyzed. Variables mutated exclusively from the template may
//   therefore be reported as immutable.
// - Whether an unresolved reference is a known global depends on the
//   configured `env`/`globals` (upstream depends on ESLint's configured
//   globals in the same way; e.g. `console` requires the `browser` or `node`
//   env). Unknown references suppress the report, matching upstream.
// - Upstream only treats `$:` labels of the instance `<script>` as reactive
//   statements; top-level `$:` labels in `<script context="module">` are also
//   checked here (see `no_reactive_functions.rs`).
impl Rule for NoImmutableReactiveStatements {
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

        let scoping = ctx.scoping();
        let root_scope = scoping.root_scope_id();
        let mut mutable_cache: FxHashMap<SymbolId, bool> = FxHashMap::default();

        for ref_node in ctx.nodes().iter() {
            let AstKind::IdentifierReference(ident) = ref_node.kind() else {
                continue;
            };
            if !labeled.span.contains_inclusive(ident.span) {
                continue;
            }
            let reference = scoping.get_reference(ident.reference_id());
            // Skip write-only references — most notably the variable the
            // reactive statement itself assigns to.
            if reference.is_write() && !reference.is_read() {
                continue;
            }
            if let Some(symbol_id) = reference.symbol_id() {
                // Upstream only considers top-level variables; references
                // to variables of nested scopes are ignored.
                if scoping.symbol_scope_id(symbol_id) != root_scope {
                    continue;
                }
                if is_mutable_symbol(symbol_id, ctx, &mut mutable_cache) {
                    // The statement references a mutable value: it is
                    // genuinely reactive.
                    return;
                }
            } else {
                let name = ident.name.as_str();
                // `$store` subscriptions are reactive; `$$props`,
                // `$$restProps` and `$$slots` are builtin Svelte variables.
                if name.starts_with('$') {
                    return;
                }
                // Known globals are immutable; keep checking.
                if ctx.is_global_defined(name) {
                    continue;
                }
                // Do not report if there are unknown references.
                return;
            }
        }

        // For `$: x = ...` report the assigned expression, otherwise the body.
        let span = if let Statement::ExpressionStatement(stmt) = &labeled.body
            && let Expression::AssignmentExpression(assign) = stmt.expression.without_parentheses()
            && assign.operator == AssignmentOperator::Assign
        {
            assign.right.span()
        } else {
            labeled.body.span()
        };
        ctx.diagnostic(no_immutable_reactive_statements_diagnostic(span));
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Whether the given top-level variable can change after initialization.
fn is_mutable_symbol(
    symbol_id: SymbolId,
    ctx: &LintContext<'_>,
    cache: &mut FxHashMap<SymbolId, bool>,
) -> bool {
    if let Some(&cached) = cache.get(&symbol_id) {
        return cached;
    }
    let mutable = compute_is_mutable_symbol(symbol_id, ctx);
    cache.insert(symbol_id, mutable);
    mutable
}

fn compute_is_mutable_symbol(symbol_id: SymbolId, ctx: &LintContext<'_>) -> bool {
    let scoping = ctx.scoping();
    if scoping.symbol_flags(symbol_id).is_import() {
        return false;
    }
    let declaration = ctx.semantic().symbol_declaration(symbol_id);
    let AstKind::VariableDeclarator(declarator) = declaration.kind() else {
        // Functions, classes, TS declarations, ... are immutable.
        return false;
    };
    let declaration_parent = ctx.nodes().parent_node(declaration.id());
    let AstKind::VariableDeclaration(var_decl) = declaration_parent.kind() else {
        return false;
    };
    if var_decl.kind.is_const() {
        // `const` initialized with a function or a literal can never change.
        let has_immutable_init = declarator.init.as_ref().is_some_and(|init| {
            let init = init.without_parentheses();
            init.is_literal()
                || matches!(
                    init,
                    Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
                )
        });
        if has_immutable_init {
            return false;
        }
    } else if matches!(
        ctx.nodes().parent_kind(declaration_parent.id()),
        AstKind::ExportDeclaration(_)
    ) {
        // `export let ...` is a prop and can be changed by the parent.
        return true;
    }
    has_write(symbol_id, ctx)
}

/// Whether the variable is ever written to, directly or through a member.
fn has_write(symbol_id: SymbolId, ctx: &LintContext<'_>) -> bool {
    let scoping = ctx.scoping();
    for reference in scoping.get_resolved_references(symbol_id) {
        if reference.is_write() {
            return true;
        }
        if has_write_member(ctx.nodes().get_node(reference.node_id()), ctx) {
            return true;
        }
    }
    false
}

/// Whether the reference is used to write to a member of the variable,
/// e.g. `x.y = 1`, `x.y.z++`, `delete x.y`.
fn has_write_member(start: &AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    let nodes = ctx.nodes();
    let mut node = start;
    loop {
        let parent = nodes.parent_node(node.id());
        let node_span = node.kind().span();
        match parent.kind() {
            AstKind::AssignmentExpression(assign) => return assign.left.span() == node_span,
            AstKind::UpdateExpression(update) => return update.argument.span() == node_span,
            AstKind::UnaryExpression(unary) => {
                return unary.operator == UnaryOperator::Delete
                    && unary.argument.span() == node_span;
            }
            AstKind::StaticMemberExpression(member) if member.object.span() == node_span => {
                node = parent;
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == node_span => {
                node = parent;
            }
            AstKind::PrivateFieldExpression(member) if member.object.span() == node_span => {
                node = parent;
            }
            // ESTree has no parenthesized expressions and wraps optional
            // chains; pass both through transparently.
            AstKind::ChainExpression(_) | AstKind::ParenthesizedExpression(_) => {
                node = parent;
            }
            _ => return false,
        }
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use serde_json::json;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));
    // `console` is only a known global in the `browser`/`node` envs.
    let browser_env = || Some(json!({ "env": { "browser": true } }));

    let pass = vec![
        // Mutable script-side values keep the statement reactive.
        // (Upstream's fixture mutates `mutableVar` via `bind:value` in the
        // template; template mutations are invisible here, so the fixture is
        // adapted to mutate it from the script.)
        (
            "<script>
	import myStore from './my-stores';
	let mutableVar = 'hello';
	export let prop;
	/* GOOD */
	$: computed1 = `${mutableVar} ${mutableVar}`;
	$: computed2 = fn1(mutableVar);
	$: console.log(mutableVar);
	$: console.log(computed1);
	$: console.log($myStore);
	$: console.log(prop);

	function fn1(v) {
		return `${v} ${v}`;
	}
	function update() {
		mutableVar = 'updated';
	}
</script>",
            None,
            browser_env(),
            svelte_path(),
        ),
        // Builtin `$$` variables.
        (
            "<script>
	$: desc = $$slots.description;
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	$: prop = $$props.prop;
	$: type = $$restProps.type;
</script>",
            None,
            None,
            svelte_path(),
        ),
        // `const` object/array with member writes is mutable.
        (
            "<script>
	const array = [1];
	const object = { b: 1 };
	$: a = array[0];
	$: b = object.b;

	function update() {
		array[0] = 2;
		object.b = 2;
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	const values = ['a', 'b'];
	$: first = values[0];

	function update() {
		values[0] = 'c';
	}
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Unknown references suppress the report.
        (
            "<script>
	$: console.log(unknown);
</script>",
            None,
            browser_env(),
            svelte_path(),
        ),
        // `let` reassigned via a compound assignment.
        (
            "<script>
	let count = 0;
	$: doubled = count * 2;

	function increment() {
		count += 1;
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
	let immutableVar = 'hello';
	$: computed1 = `${immutableVar} ${immutableVar}`;
	$: computed1 = fn1(immutableVar);
	$: console.log(immutableVar);

	function fn1(v) {
		return `${v} ${v}`;
	}
</script>",
            None,
            browser_env(),
            svelte_path(),
        ),
        (
            "<script>
	import myVar from './my-variables';
	let mutableVar = 'hello';

	const immutableVar = 'hello';
	/* BAD */
	$: computed3 = fn1(immutableVar);
	$: computed4 = fn2();
	$: console.log(immutableVar);
	$: console.log(myVar);

	function fn1(v) {
		return `${v} ${v}`;
	}
	function fn2() {
		return `${mutableVar} ${mutableVar}`;
	}
	function update() {
		mutableVar = 'updated';
	}
</script>",
            None,
            browser_env(),
            svelte_path(),
        ),
        (
            "<script>
	export const thisIs = 'readonly';

	export function greet(name) {
		console.log(`hello ${name}!`);
	}

	export class Foo {}

	const immutableVar = 'hello';

	$: message1 = greet(immutableVar);
	$: message2 = `this is${thisIs}`;
	$: instance = new Foo();
</script>",
            None,
            browser_env(),
            svelte_path(),
        ),
    ];

    Tester::new(
        NoImmutableReactiveStatements::NAME,
        NoImmutableReactiveStatements::PLUGIN,
        pass,
        fail,
    )
    .test_and_snapshot();
}
