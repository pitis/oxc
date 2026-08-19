use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_add_event_listener_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Do not use `addEventListener`. Use the `on` function from `svelte/events` instead.",
    )
    .with_help("`import { on } from 'svelte/events'`, then `on(target, type, handler)`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAddEventListener;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `addEventListener`, preferring the `on` function from
    /// `svelte/events`.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte 5's `on` preserves the ordering guarantees the framework relies
    /// on: handlers it attaches itself always run before ones attached later
    /// by user code, which plain `addEventListener` can violate. `on` also
    /// returns a cleanup function, which fits `$effect` and `onMount`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// node.addEventListener('click', handler);
    /// addEventListener('resize', handler);
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { on } from 'svelte/events';
    ///
    /// on(node, 'click', handler);
    /// on(window, 'resize', handler);
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream only runs this rule on Svelte 5 projects (`on` does not exist
    /// before it). oxlint does not resolve the installed Svelte version, so
    /// the rule applies to every `.svelte` file; it is off by default.
    NoAddEventListener,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Disallow `addEventListener` in favour of `svelte/events`' `on`.",
);

impl Rule for NoAddEventListener {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let is_add_event_listener = match call.callee.get_inner_expression() {
            Expression::StaticMemberExpression(member) => {
                member.property.name == "addEventListener"
            }
            // A bare `addEventListener(…)` is `window.addEventListener`.
            Expression::Identifier(identifier) => identifier.name == "addEventListener",
            _ => false,
        };
        if is_add_event_listener {
            ctx.diagnostic(no_add_event_listener_diagnostic(call.span));
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
                import { on } from 'svelte/events';
                on(node, 'click', handler);
            </script>",
            None,
            None,
            path(),
        ),
        ("<script>\n\tnode.removeEventListener('click', handler);\n</script>", None, None, path()),
        // A property merely named like it, not called.
        ("<script>\n\tconst f = node.addEventListener;\n</script>", None, None, path()),
    ];
    let fail = vec![
        ("<script>\n\tnode.addEventListener('click', handler);\n</script>", None, None, path()),
        ("<script>\n\taddEventListener('resize', handler);\n</script>", None, None, path()),
        ("<script>\n\twindow.addEventListener('resize', handler);\n</script>", None, None, path()),
        ("<script>\n\tnode?.addEventListener('click', handler);\n</script>", None, None, path()),
    ];

    Tester::new(NoAddEventListener::NAME, NoAddEventListener::PLUGIN, pass, fail)
        .test_and_snapshot();
}
