use oxc_ast::ast::{Argument, BindingPattern, Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::{ContextHost, LintContext},
    rule::Rule,
    utils::for_each_svelte_store_call,
};

fn require_store_callbacks_use_set_param_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Store callbacks must use `set` param.")
        .with_help("Name the callback's first parameter `set`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireStoreCallbacksUseSetParam;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the start/stop notifier passed to `writable()` or
    /// `readable()` to name its first parameter `set`.
    ///
    /// ### Why is this bad?
    ///
    /// The parameter is the store's setter. Naming it anything else — or
    /// omitting it and reaching for the store variable instead — obscures
    /// what the callback does, and every example in the Svelte docs calls it
    /// `set`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// import { readable } from 'svelte/store';
    ///
    /// const time = readable(null, (update) => {});
    /// const other = readable(null, () => {});
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { readable } from 'svelte/store';
    ///
    /// const time = readable(null, (set) => {
    ///   set(new Date());
    /// });
    /// ```
    RequireStoreCallbacksUseSetParam,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Require store callbacks to name their first parameter `set`.",
);

impl Rule for RequireStoreCallbacksUseSetParam {
    fn run_once(&self, ctx: &LintContext) {
        let mut spans = Vec::new();
        for_each_svelte_store_call(ctx, &["readable", "writable"], &mut |call, _| {
            let Some(callback) = call.arguments.get(1).and_then(Argument::as_expression) else {
                return;
            };
            let (params, span) = match callback.get_inner_expression() {
                Expression::ArrowFunctionExpression(func) => (&func.params, func.span),
                Expression::FunctionExpression(func) => (&func.params, func.span),
                _ => return,
            };
            // Upstream reports when there is no first parameter at all, and
            // when there is one whose (simple) name is not `set`. A
            // destructuring pattern is left alone.
            let named_set = params.items.first().is_some_and(|param| {
                match &param.pattern {
                    BindingPattern::BindingIdentifier(id) => id.name == "set",
                    // Not a plain identifier: upstream does not rename it.
                    _ => true,
                }
            });
            if !named_set {
                spans.push(span);
            }
        });
        for span in spans {
            ctx.diagnostic(require_store_callbacks_use_set_param_diagnostic(span));
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let path = || Some(PathBuf::from("test.svelte"));
    let pass = vec![
        (
            "<script>
                import { readable } from 'svelte/store';
                const time = readable(null, (set) => { set(new Date()); });
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { writable } from 'svelte/store';
                const w = writable(0, function (set) { set(1); });
            </script>",
            None,
            None,
            path(),
        ),
        // No callback at all.
        (
            "<script>
                import { writable } from 'svelte/store';
                const w = writable(0);
            </script>",
            None,
            None,
            path(),
        ),
        // A destructuring parameter is left alone, like upstream.
        (
            "<script>
                import { readable } from 'svelte/store';
                const r = readable(null, ({ set }) => {});
            </script>",
            None,
            None,
            path(),
        ),
        // `derived` is not checked by this rule.
        (
            "<script>
                import { writable, derived } from 'svelte/store';
                const w = writable(0);
                const d = derived(w, ($w) => $w, 0);
            </script>",
            None,
            None,
            path(),
        ),
    ];
    let fail = vec![
        (
            "<script>
                import { readable } from 'svelte/store';
                const time = readable(null, (update) => {});
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { readable } from 'svelte/store';
                const time = readable(null, () => {});
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { writable } from 'svelte/store';
                const w = writable(0, function (update) {});
            </script>",
            None,
            None,
            path(),
        ),
    ];

    Tester::new(
        RequireStoreCallbacksUseSetParam::NAME,
        RequireStoreCallbacksUseSetParam::PLUGIN,
        pass,
        fail,
    )
    .test_and_snapshot();
}
