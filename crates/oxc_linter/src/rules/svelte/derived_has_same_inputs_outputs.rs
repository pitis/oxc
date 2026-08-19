use oxc_ast::ast::{Argument, BindingPattern, Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::{ContextHost, LintContext},
    rule::Rule,
    utils::for_each_svelte_store_call,
};

fn derived_has_same_inputs_outputs_diagnostic(expected: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The argument name should be `{expected}`."))
        .with_help(format!("Rename the parameter to `{expected}` so it matches its store."))
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct DerivedHasSameInputsOutputs;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the callback parameter of `derived()` to be named after the
    /// store it receives, with the `$` prefix Svelte uses for a store's
    /// value.
    ///
    /// ### Why is this bad?
    ///
    /// `derived(count, (c) => c * 2)` hides which store `c` came from. The
    /// `$`-prefixed name is the same one you would write in the template to
    /// read that store, so `$count` reads consistently everywhere.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// import { derived } from 'svelte/store';
    ///
    /// const doubled = derived(count, (c) => c * 2);
    /// const sum = derived([a, b], ([x, y]) => x + y);
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { derived } from 'svelte/store';
    ///
    /// const doubled = derived(count, ($count) => $count * 2);
    /// const sum = derived([a, b], ([$a, $b]) => $a + $b);
    /// ```
    DerivedHasSameInputsOutputs,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Require `derived` callback params to match their store names.",
);

impl Rule for DerivedHasSameInputsOutputs {
    fn run_once(&self, ctx: &LintContext) {
        let mut reports: Vec<(String, Span)> = Vec::new();
        for_each_svelte_store_call(ctx, &["derived"], &mut |call, _| {
            let Some(stores) = call.arguments.first().and_then(Argument::as_expression) else {
                return;
            };
            let Some(callback) = call.arguments.get(1).and_then(Argument::as_expression) else {
                return;
            };
            let params = match callback.get_inner_expression() {
                Expression::ArrowFunctionExpression(func) => &func.params,
                Expression::FunctionExpression(func) => &func.params,
                _ => return,
            };
            let Some(first) = params.items.first() else { return };

            match (stores.get_inner_expression(), &first.pattern) {
                // `derived(count, ($count) => …)`
                (Expression::Identifier(store), BindingPattern::BindingIdentifier(param)) => {
                    let expected = format!("${}", store.name);
                    if param.name.as_str() != expected {
                        reports.push((expected, param.span));
                    }
                }
                // `derived([a, b], ([$a, $b]) => …)`
                (Expression::ArrayExpression(stores), BindingPattern::ArrayPattern(pattern)) => {
                    for (store, element) in stores.elements.iter().zip(pattern.elements.iter()) {
                        let (Some(Expression::Identifier(store)), Some(element)) =
                            (store.as_expression(), element.as_ref())
                        else {
                            continue;
                        };
                        let BindingPattern::BindingIdentifier(param) = element else { continue };
                        let expected = format!("${}", store.name);
                        if param.name.as_str() != expected {
                            reports.push((expected, param.span));
                        }
                    }
                }
                _ => {}
            }
        });
        for (expected, span) in reports {
            ctx.diagnostic(derived_has_same_inputs_outputs_diagnostic(&expected, span));
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
                import { derived } from 'svelte/store';
                const doubled = derived(count, ($count) => $count * 2, 0);
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { derived } from 'svelte/store';
                const sum = derived([a, b], ([$a, $b]) => $a + $b, 0);
            </script>",
            None,
            None,
            path(),
        ),
        // Not a plain identifier store: nothing to compare against.
        (
            "<script>
                import { derived } from 'svelte/store';
                const d = derived(stores.count, (value) => value, 0);
            </script>",
            None,
            None,
            path(),
        ),
        // A destructuring parameter over an identifier store is left alone.
        (
            "<script>
                import { derived } from 'svelte/store';
                const d = derived(count, ({ value }) => value, 0);
            </script>",
            None,
            None,
            path(),
        ),
    ];
    let fail = vec![
        (
            "<script>
                import { derived } from 'svelte/store';
                const doubled = derived(count, (c) => c * 2, 0);
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { derived } from 'svelte/store';
                const sum = derived([a, b], ([x, y]) => x + y, 0);
            </script>",
            None,
            None,
            path(),
        ),
        // Only the mismatching element of the array pattern is reported.
        (
            "<script>
                import { derived } from 'svelte/store';
                const sum = derived([a, b], ([$a, y]) => $a + y, 0);
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                import { derived } from 'svelte/store';
                const d = derived(count, function (c) { return c; }, 0);
            </script>",
            None,
            None,
            path(),
        ),
    ];

    Tester::new(DerivedHasSameInputsOutputs::NAME, DerivedHasSameInputsOutputs::PLUGIN, pass, fail)
        .test_and_snapshot();
}
