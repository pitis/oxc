use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    context::{ContextHost, LintContext},
    rule::Rule,
    utils::is_svelte_path,
};

fn no_top_level_browser_globals_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Unexpected top-level browser global variable \"{name}\"."))
        .with_help("Move the access into `onMount`, an `$effect`, or a `typeof window !== 'undefined'` guard.")
        .with_label(span)
}

/// The browser globals upstream's `getBrowserGlobals()` returns, minus the
/// ones that also exist on the server.
const BROWSER_GLOBALS: [&str; 16] = [
    "alert",
    "confirm",
    "document",
    "history",
    "indexedDB",
    "localStorage",
    "location",
    "matchMedia",
    "navigator",
    "prompt",
    "requestAnimationFrame",
    "screen",
    "scrollBy",
    "scrollTo",
    "sessionStorage",
    "window",
];

#[derive(Debug, Default, Clone)]
pub struct NoTopLevelBrowserGlobals;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows reading a browser global at the top level of a component or
    /// of a `.svelte.js` / `.svelte.ts` module.
    ///
    /// ### Why is this bad?
    ///
    /// Top-level code runs during server-side rendering, where `window`,
    /// `document` and friends do not exist, so the render crashes. Deferring
    /// the access to `onMount` or an `$effect` — or guarding it — keeps the
    /// component renderable on the server.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   const width = window.innerWidth;
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { onMount } from 'svelte';
    ///
    ///   let width = 0;
    ///   onMount(() => {
    ///     width = window.innerWidth;
    ///   });
    /// </script>
    /// ```
    ///
    /// ```svelte
    /// <script>
    ///   import { browser } from '$app/environment';
    ///
    ///   if (browser) {
    ///     console.log(window.innerWidth);
    ///   }
    /// </script>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// The guards recognised are the common ones: an enclosing
    /// `if (typeof window !== 'undefined')`, `if (browser)` (from
    /// `$app/environment`), or `if (!import.meta.env.SSR)`, and the same
    /// tests written as a `&&` / ternary. Upstream additionally tracks a
    /// guard's condition through intermediate variables and negated forms it
    /// can prove equivalent, so an unusual guard may be reported here.
    NoTopLevelBrowserGlobals,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow browser globals at the top level of a component.",
);

impl Rule for NoTopLevelBrowserGlobals {
    fn run_once(&self, ctx: &LintContext) {
        let scoping = ctx.scoping();
        for reference_ids in scoping.root_unresolved_references_ids() {
            for reference_id in reference_ids {
                let reference = scoping.get_reference(reference_id);
                if reference.is_type() || !reference.is_read() {
                    continue;
                }
                let name = ctx.semantic().reference_name(reference);
                if BROWSER_GLOBALS.binary_search(&name).is_err() {
                    continue;
                }
                let node = ctx.nodes().get_node(reference.node_id());
                // `typeof window` is safe even where `window` does not
                // exist — it is how the guard itself is written.
                if is_typeof_operand(node, ctx)
                    || is_inside_function(node, ctx)
                    || is_guarded(node, ctx)
                {
                    continue;
                }
                ctx.diagnostic(no_top_level_browser_globals_diagnostic(name, node.kind().span()));
            }
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        is_svelte_path(ctx.file_path())
    }
}

/// Whether the reference is the operand of a `typeof`.
fn is_typeof_operand(node: &crate::AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    matches!(
        ctx.nodes().parent_kind(node.id()),
        AstKind::UnaryExpression(unary)
            if unary.operator == oxc_syntax::operator::UnaryOperator::Typeof
    )
}

/// Whether the reference sits inside a function, where it only runs when the
/// function is called.
fn is_inside_function(node: &crate::AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    ctx.nodes().ancestors(node.id()).any(|ancestor| {
        matches!(
            ancestor.kind(),
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) | AstKind::StaticBlock(_)
        )
    })
}

/// Whether an enclosing `if`, `&&` or ternary tests for a browser
/// environment.
fn is_guarded(node: &crate::AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    let mut child_span = node.kind().span();
    for ancestor in ctx.nodes().ancestors(node.id()) {
        match ancestor.kind() {
            AstKind::IfStatement(statement) => {
                // Only the consequent is guarded; the `else` runs on the
                // server.
                if statement.consequent.span().contains_inclusive(child_span)
                    && is_browser_test(&statement.test)
                {
                    return true;
                }
            }
            AstKind::ConditionalExpression(conditional) => {
                if conditional.consequent.span().contains_inclusive(child_span)
                    && is_browser_test(&conditional.test)
                {
                    return true;
                }
            }
            AstKind::LogicalExpression(logical)
                if logical.operator == oxc_syntax::operator::LogicalOperator::And
                    && logical.right.span().contains_inclusive(child_span)
                    && is_browser_test(&logical.left) =>
            {
                return true;
            }
            _ => {}
        }
        child_span = ancestor.kind().span();
    }
    false
}

/// Whether the expression tests that the code is running in a browser.
fn is_browser_test(test: &Expression<'_>) -> bool {
    match test.get_inner_expression() {
        // `browser` from `$app/environment`.
        Expression::Identifier(identifier) => identifier.name == "browser",
        // `typeof window !== 'undefined'`
        Expression::BinaryExpression(binary) => {
            let typeof_operand = match binary.left.get_inner_expression() {
                Expression::UnaryExpression(unary)
                    if unary.operator == oxc_syntax::operator::UnaryOperator::Typeof =>
                {
                    unary.argument.get_inner_expression()
                }
                _ => return false,
            };
            let is_browser_global = matches!(
                typeof_operand,
                Expression::Identifier(identifier)
                    if BROWSER_GLOBALS.binary_search(&identifier.name.as_str()).is_ok()
            );
            let is_undefined = matches!(
                binary.right.get_inner_expression(),
                Expression::StringLiteral(literal) if literal.value == "undefined"
            );
            let inequality = matches!(
                binary.operator,
                oxc_syntax::operator::BinaryOperator::StrictInequality
                    | oxc_syntax::operator::BinaryOperator::Inequality
            );
            is_browser_global && is_undefined && inequality
        }
        // `!import.meta.env.SSR`
        Expression::UnaryExpression(unary)
            if unary.operator == oxc_syntax::operator::UnaryOperator::LogicalNot =>
        {
            is_ssr_flag(&unary.argument)
        }
        _ => false,
    }
}

/// Whether the expression is `import.meta.env.SSR`.
fn is_ssr_flag(expression: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(ssr) = expression.get_inner_expression() else {
        return false;
    };
    if ssr.property.name != "SSR" {
        return false;
    }
    let Expression::StaticMemberExpression(env) = ssr.object.get_inner_expression() else {
        return false;
    };
    env.property.name == "env"
        && matches!(env.object.get_inner_expression(), Expression::ImportMeta(_))
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let path = || Some(PathBuf::from("test.svelte"));
    let pass = vec![
        // Deferred into a function.
        (
            "<script>
                import { onMount } from 'svelte';
                let width = 0;
                onMount(() => { width = window.innerWidth; });
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                function measure() { return document.body.clientWidth; }
            </script>",
            None,
            None,
            path(),
        ),
        // Guarded.
        (
            "<script>
                import { browser } from '$app/environment';
                if (browser) { console.log(window.innerWidth); }
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                if (typeof window !== 'undefined') { console.log(window.innerWidth); }
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                const width = typeof window !== 'undefined' ? window.innerWidth : 0;
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                const ok = typeof document !== 'undefined' && document.title;
            </script>",
            None,
            None,
            path(),
        ),
        (
            "<script>
                if (!import.meta.env.SSR) { console.log(window.innerWidth); }
            </script>",
            None,
            None,
            path(),
        ),
        // A local binding of the same name is not the global.
        (
            "<script>\n\tconst location = 'here';\n\tconsole.log(location);\n</script>",
            None,
            None,
            path(),
        ),
    ];
    let fail = vec![
        ("<script>\n\tconst width = window.innerWidth;\n</script>", None, None, path()),
        ("<script>\n\tconst title = document.title;\n</script>", None, None, path()),
        ("<script>\n\tconst saved = localStorage.getItem('x');\n</script>", None, None, path()),
        // The `else` branch of a browser guard runs on the server.
        (
            "<script>
                if (typeof window !== 'undefined') { console.log(1); } else { console.log(document.title); }
            </script>",
            None,
            None,
            path(),
        ),
    ];

    Tester::new(NoTopLevelBrowserGlobals::NAME, NoTopLevelBrowserGlobals::PLUGIN, pass, fail)
        .test_and_snapshot();
}
