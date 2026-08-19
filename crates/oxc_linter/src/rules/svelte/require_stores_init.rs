use oxc_ast::ast::Argument;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::{ContextHost, LintContext},
    rule::Rule,
    utils::{SVELTE_STORE_FACTORIES, for_each_svelte_store_call},
};

fn require_stores_init_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Store should have an initial value.")
        .with_help("Pass the store's initial value as the first argument.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireStoresInit;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires store initialization: `writable()` and `readable()` must be
    /// given an initial value, and `derived()` its stores, callback, and
    /// initial value.
    ///
    /// ### Why is this bad?
    ///
    /// A store created without an initial value starts as `undefined`, which
    /// every subscriber then has to handle — usually by accident rather than
    /// by design. Being explicit about the starting value keeps the store's
    /// type honest.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// import { writable, readable, derived } from 'svelte/store';
    ///
    /// const w = writable();
    /// const r = readable();
    /// const d = derived(w, ($w) => $w * 2);
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { writable, readable, derived } from 'svelte/store';
    ///
    /// const w = writable(0);
    /// const r = readable(0);
    /// const d = derived(w, ($w) => $w * 2, 0);
    /// ```
    RequireStoresInit,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Require initial values for `svelte/store` stores.",
);

impl Rule for RequireStoresInit {
    fn run_once(&self, ctx: &LintContext) {
        let mut spans = Vec::new();
        for_each_svelte_store_call(ctx, &SVELTE_STORE_FACTORIES, &mut |call, factory| {
            // `derived` additionally takes the stores and the callback, so
            // its initial value is the third argument.
            let required = if factory == "derived" { 3 } else { 1 };
            if call.arguments.len() >= required
                // A spread could supply the missing arguments.
                || call.arguments.iter().any(|argument| matches!(argument, Argument::SpreadElement(_)))
            {
                return;
            }
            spans.push(call.span);
        });
        for span in spans {
            ctx.diagnostic(require_stores_init_diagnostic(span));
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
                import { writable, readable, derived } from 'svelte/store';
                const w = writable(0);
                const r = readable(0);
                const d = derived(w, ($w) => $w * 2, 0);
            </script>",
            None,
            None,
            path(),
        ),
        // A spread may supply the arguments.
        (
            "<script>
                import { writable } from 'svelte/store';
                const w = writable(...args);
            </script>",
            None,
            None,
            path(),
        ),
        // Not the `svelte/store` factories.
        (
            "<script>
                import { writable } from './my-store';
                const w = writable();
            </script>",
            None,
            None,
            path(),
        ),
    ];
    let fail = vec![
        (
            "<script>
                import { writable } from 'svelte/store';
                const w = writable();
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { readable } from 'svelte/store';
                const r = readable();
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { writable, derived } from 'svelte/store';
                const w = writable(0);
                const d = derived(w, ($w) => $w * 2);
            </script>",
            None,
            None,
            path(),
        ),
        // Namespace import and local alias are tracked too.
        (
            "<script>
                import * as store from 'svelte/store';
                const w = store.writable();
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { writable } from 'svelte/store';
                const alias = writable;
                const w = alias();
            </script>",
            None,
            None,
            path(),
        ),
    ];

    Tester::new(RequireStoresInit::NAME, RequireStoresInit::PLUGIN, pass, fail).test_and_snapshot();
}
