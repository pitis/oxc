use oxc_ast::{
    AstKind,
    ast::{Expression, Statement},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_reactive_functions_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Do not create functions inside reactive statements unless absolutely necessary.",
    )
    .with_help("Move the function out of the reactive statement.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoReactiveFunctions;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow defining functions in reactive statements.
    ///
    /// This rule only applies to Svelte 3/4 reactive statements (`$:`), and to
    /// Svelte 5 components which do not use runes.
    ///
    /// ### Why is this bad?
    ///
    /// A function defined in a reactive statement is re-created on every run of
    /// the reactive statement, even though the function itself never changes.
    /// Declaring the function with `const` outside of a reactive statement
    /// behaves identically and avoids the churn.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   $: arrow = () => {};
    ///   $: func = function () {};
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   const arrow = () => {};
    ///   const func = function () {};
    /// </script>
    /// ```
    NoReactiveFunctions,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow defining functions in reactive statements.",
);

// Ported from <https://github.com/sveltejs/eslint-plugin-svelte/blob/main/packages/eslint-plugin-svelte/src/rules/no-reactive-functions.ts>
//
// Deviations from upstream:
// - Upstream provides a suggestion fix (replace the `$:` label with `const`);
//   suggestions are not supported here, so only the diagnostic is reported.
// - Upstream only treats `$:` labels of the instance `<script>` as reactive
//   statements. The extracted script blocks of a `.svelte` file cannot be told
//   apart here, so top-level `$:` labels in `<script context="module">` are
//   also checked (`$:` labels are inert there, so flagged code is dead either
//   way).
impl Rule for NoReactiveFunctions {
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
        // ESTree has no parenthesized expression nodes, so unwrap parentheses
        // to match upstream's `AssignmentExpression > :function` selector.
        let Expression::AssignmentExpression(assign) = stmt.expression.without_parentheses() else {
            return;
        };
        if matches!(
            assign.right.without_parentheses(),
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        ) {
            ctx.diagnostic(no_reactive_functions_diagnostic(labeled.span));
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

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        (
            "<!-- prettier-ignore -->
<script>
    const arrow = () => {}
    const fn = function() { }
</script>",
            None,
            None,
            svelte_path(),
        ),
        // `$:` labels inside functions are plain labels, not reactive statements.
        (
            "<script>
    function setup() {
        $: fn = () => {}
    }
</script>",
            None,
            None,
            svelte_path(),
        ),
        // The function is not directly assigned in the reactive statement.
        (
            "<script>
    let value = 0;
    $: fn = value % 2 ? () => 'odd' : () => 'even';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
    let value = 0;
    $: result = (() => value * 2)();
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Reactive statements without assignments.
        (
            "<script>
    let value = 0;
    $: console.log(value);
    $: { value; }
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<!-- prettier-ignore -->
<script>
    $: arrow = () => {}
    $: fn = function() {}
    $:nospace = () => {}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
    let obj = {};
    $: obj.fn = () => {}
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
    $: fn = (() => {})
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script lang=\"ts\">
    $: arrow = (): number => 1
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(NoReactiveFunctions::NAME, NoReactiveFunctions::PLUGIN, pass, fail)
        .test_and_snapshot();
}
