use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_ignored_unsubscribe_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Ignoring returned value of the subscribe method is forbidden.")
        .with_help("Keep the returned unsubscribe function and call it on destroy, or use the `$store` auto-subscription.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoIgnoredUnsubscribe;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows discarding the unsubscribe function returned by a store's
    /// `subscribe()`.
    ///
    /// ### Why is this bad?
    ///
    /// A subscription that is never cancelled keeps the subscriber — and
    /// everything it closes over — alive for as long as the store lives. In a
    /// component that is created and destroyed repeatedly this leaks memory
    /// and keeps running stale callbacks.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// store.subscribe((value) => console.log(value));
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// const unsubscribe = store.subscribe((value) => console.log(value));
    /// onDestroy(unsubscribe);
    /// ```
    ///
    /// Or let Svelte manage it with the `$` prefix:
    /// ```svelte
    /// <p>{$store}</p>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Like upstream, any `x.subscribe(…)` whose result is discarded is
    /// reported — there is no check that `x` really is a store, so an
    /// unrelated `subscribe` method (an RxJS observable, an event emitter)
    /// is reported too. Also like upstream, an optional call
    /// (`store?.subscribe(…)`) is not matched.
    NoIgnoredUnsubscribe,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow ignoring the unsubscribe returned by `subscribe()`.",
);

impl Rule for NoIgnoredUnsubscribe {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // Upstream matches `ExpressionStatement > CallExpression >
        // MemberExpression.callee[property.name='subscribe']`: a `subscribe`
        // call whose value is thrown away because it is the whole statement.
        let AstKind::ExpressionStatement(statement) = node.kind() else {
            return;
        };
        let Expression::CallExpression(call) = statement.expression.without_parentheses() else {
            return;
        };
        let Expression::StaticMemberExpression(member) = call.callee.get_inner_expression() else {
            return;
        };
        if member.property.name == "subscribe" {
            ctx.diagnostic(no_ignored_unsubscribe_diagnostic(member.property.span));
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
                const unsubscribe = store.subscribe((value) => value);
                onDestroy(unsubscribe);
            </script>",
            None,
            None,
            path(),
        ),
        // The result is used, not discarded.
        (
            "<script>
                onDestroy(store.subscribe((value) => value));
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                subscriptions.push(store.subscribe((value) => value));
            </script>",
            None,
            None,
            path(),
        ),
        // Some other method.
        ("<script>\n\tstore.set(1);\n</script>", None, None, path()),
        // Optional chaining wraps the call in a `ChainExpression`, which
        // upstream's `ExpressionStatement > CallExpression` selector does not
        // match either.
        ("<script>\n\tstore?.subscribe(fn);\n</script>", None, None, path()),
    ];
    let fail = vec![
        ("<script>\n\tstore.subscribe((value) => value);\n</script>", None, None, path()),
        (
            "<script>
                function f() {
                    store.subscribe((value) => value);
                }
            </script>",
            None,
            None,
            path(),
        ),
    ];

    Tester::new(NoIgnoredUnsubscribe::NAME, NoIgnoredUnsubscribe::PLUGIN, pass, fail)
        .test_and_snapshot();
}
