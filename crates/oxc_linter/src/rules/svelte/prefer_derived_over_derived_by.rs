use oxc_ast::{
    AstKind,
    ast::{Argument, ArrowFunctionBody, Expression, Statement},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn prefer_derived_over_derived_by_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unnecessary `$derived.by(...)`. Use `$derived(...)` instead.")
        .with_help(
            "The callback is a single expression, so `$derived(expression)` says the same thing.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferDerivedOverDerivedBy;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers `$derived(expression)` over `$derived.by(() => expression)`.
    ///
    /// ### Why is this bad?
    ///
    /// `$derived.by` exists for derivations that need statements — a loop, a
    /// temporary, an early return. When the callback is just one expression,
    /// the wrapping function adds nothing.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// let doubled = $derived.by(() => count * 2);
    /// let tripled = $derived.by(() => {
    ///   return count * 3;
    /// });
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// let doubled = $derived(count * 2);
    /// let total = $derived.by(() => {
    ///   let sum = 0;
    ///   for (const n of numbers) sum += n;
    ///   return sum;
    /// });
    /// ```
    PreferDerivedOverDerivedBy,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Prefer `$derived` over a single-expression `$derived.by`.",
);

impl Rule for PreferDerivedOverDerivedBy {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        // `$derived.by(...)`, not computed, exactly one argument.
        let Expression::StaticMemberExpression(member) = call.callee.get_inner_expression() else {
            return;
        };
        if member.property.name != "by"
            || !matches!(member.object.get_inner_expression(), Expression::Identifier(object) if object.name == "$derived")
            || call.arguments.len() != 1
        {
            return;
        }
        let Some(argument) = call.arguments.first().and_then(Argument::as_expression) else {
            return;
        };

        // A plain, parameterless, synchronous, non-generator function whose
        // body is one expression (or one `return expression`).
        let single_expression = match argument.get_inner_expression() {
            Expression::ArrowFunctionExpression(func) => {
                if !func.params.items.is_empty() || func.r#async {
                    return;
                }
                match &func.body {
                    // `() => expression`
                    ArrowFunctionBody::FunctionBody(body) => is_single_return(&body.statements),
                    // Any other variant is an expression body.
                    _ => true,
                }
            }
            Expression::FunctionExpression(func) => {
                if !func.params.items.is_empty() || func.r#async || func.generator {
                    return;
                }
                func.body.as_ref().is_some_and(|body| is_single_return(&body.statements))
            }
            _ => return,
        };
        if single_expression {
            ctx.diagnostic(prefer_derived_over_derived_by_diagnostic(call.span));
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Whether the body is exactly `return <expression>;`.
fn is_single_return(statements: &[Statement<'_>]) -> bool {
    matches!(statements, [Statement::ReturnStatement(ret)] if ret.argument.is_some())
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let path = || Some(PathBuf::from("test.svelte"));
    let pass = vec![
        ("<script>\n\tlet doubled = $derived(count * 2);\n</script>", None, None, path()),
        // Real statements need `.by`.
        (
            "<script>
                let total = $derived.by(() => {
                    let sum = 0;
                    for (const n of numbers) sum += n;
                    return sum;
                });
            </script>",
            None,
            None,
            path(),
        ),
        // A bare `return;` has no expression to inline.
        ("<script>\n\tlet x = $derived.by(() => { return; });\n</script>", None, None, path()),
        // Parameters, async, and generators are left alone.
        ("<script>\n\tlet x = $derived.by((a) => a);\n</script>", None, None, path()),
        ("<script>\n\tlet x = $derived.by(async () => value);\n</script>", None, None, path()),
        // Not the rune.
        ("<script>\n\tlet x = other.by(() => value);\n</script>", None, None, path()),
    ];
    let fail = vec![
        ("<script>\n\tlet doubled = $derived.by(() => count * 2);\n</script>", None, None, path()),
        (
            "<script>\n\tlet tripled = $derived.by(() => {\n\t\treturn count * 3;\n\t});\n</script>",
            None,
            None,
            path(),
        ),
        (
            "<script>\n\tlet x = $derived.by(function () { return count; });\n</script>",
            None,
            None,
            path(),
        ),
    ];

    Tester::new(PreferDerivedOverDerivedBy::NAME, PreferDerivedOverDerivedBy::PLUGIN, pass, fail)
        .test_and_snapshot();
}
