use oxc_ast::ast::{Argument, CallExpression, Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::{ContextHost, LintContext},
    rule::Rule,
    utils::{SVELTE_STORE_FACTORIES, for_each_svelte_store_call},
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

impl Rule for NoStoreAsync {
    fn run_once(&self, ctx: &LintContext) {
        let mut spans = Vec::new();
        for_each_svelte_store_call(ctx, &SVELTE_STORE_FACTORIES, &mut |call, _| {
            if let Some(span) = async_callback_span(call) {
                spans.push(span);
            }
        });
        for span in spans {
            ctx.diagnostic(no_store_async_diagnostic(span));
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// The `async` keyword of the call's second argument (the start/stop notifier
/// or the derive callback), when that argument is an async function.
fn async_callback_span(call: &CallExpression<'_>) -> Option<Span> {
    let argument = call.arguments.get(1).and_then(Argument::as_expression)?;
    let function_span = match argument.get_inner_expression() {
        Expression::ArrowFunctionExpression(func) if func.r#async => func.span,
        Expression::FunctionExpression(func) if func.r#async => func.span,
        _ => return None,
    };
    // Like upstream, only the `async` keyword is highlighted.
    Some(Span::sized(function_span.start, 5))
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
