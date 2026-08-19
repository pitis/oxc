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

fn no_reactive_literals_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Do not assign literal values inside reactive statements unless absolutely necessary.",
    )
    .with_help("Move the literal out of the reactive statement into an assignment.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoReactiveLiterals;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow assigning literal values in reactive statements.
    ///
    /// This rule flags reactive statements that assign a plain literal, an
    /// empty array (`[]`), or an empty object (`{}`), since such values can
    /// never change between runs.
    ///
    /// This rule only applies to Svelte 3/4 reactive statements (`$:`), and to
    /// Svelte 5 components which do not use runes.
    ///
    /// ### Why is this bad?
    ///
    /// A reactive statement that assigns a constant literal has no reactive
    /// dependencies, so it only ever runs once. A plain `let` declaration
    /// expresses the same thing without pretending to be reactive.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   $: foo = "foo";
    ///   $: bar = [];
    ///   $: baz = {};
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let foo = "foo";
    ///   let value = "bar";
    ///   $: qux = `${value}baz`;
    /// </script>
    /// ```
    NoReactiveLiterals,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow assigning literals in reactive statements.",
);

// Ported from <https://github.com/sveltejs/eslint-plugin-svelte/blob/main/packages/eslint-plugin-svelte/src/rules/no-reactive-literals.ts>
//
// Matches upstream's selector: the assigned value must be an ESTree `Literal`
// (string, number, boolean, null, bigint, regex — NOT a template literal), an
// empty array expression, or an empty object expression.
//
// Deviations from upstream:
// - Upstream provides a suggestion fix (rewrite the statement as a `let`
//   declaration); suggestions are not supported here, so only the diagnostic
//   is reported.
// - Upstream only treats `$:` labels of the instance `<script>` as reactive
//   statements. The extracted script blocks of a `.svelte` file cannot be told
//   apart here, so top-level `$:` labels in `<script context="module">` are
//   also checked (`$:` labels are inert there, so flagged code is dead either
//   way).
impl Rule for NoReactiveLiterals {
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
        // to match upstream's `AssignmentExpression:matches(...)` selector.
        let Expression::AssignmentExpression(assign) = stmt.expression.without_parentheses() else {
            return;
        };
        let is_static = match assign.right.without_parentheses() {
            // $: foo = "foo"; $: foo = 1;
            expr if expr.is_literal() => true,
            // $: foo = [];
            Expression::ArrayExpression(array) => array.elements.is_empty(),
            // $: foo = {};
            Expression::ObjectExpression(object) => object.properties.is_empty(),
            _ => false,
        };
        if is_static {
            ctx.diagnostic(no_reactive_literals_diagnostic(labeled.span));
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
    $: foo = `${\"bar\"}baz`
    $: bar = [ \"bar\" ]
    $: baz = { qux : true }

    let qux;

    qux = 1;
    qux = [];
    qux = {};
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Template literals are not ESTree `Literal`s, even without expressions.
        (
            "<script>
    $: foo = `foo`;
</script>",
            None,
            None,
            svelte_path(),
        ),
        // `$:` labels inside functions are plain labels, not reactive statements.
        (
            "<script>
    function setup() {
        $: foo = 1;
    }
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
    let value = 0;
    $: doubled = value * 2;
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
    $: foo = \"foo\";
    $: bar = [];
    $: baz = {};
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
    $: foo = 1;
    $: bar = true;
    $: baz = null;
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script lang=\"ts\">
    $: foo = \"foo\";
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(NoReactiveLiterals::NAME, NoReactiveLiterals::PLUGIN, pass, fail)
        .test_and_snapshot();
}
