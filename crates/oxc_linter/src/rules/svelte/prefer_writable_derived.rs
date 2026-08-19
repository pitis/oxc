use oxc_ast::{
    AstKind,
    ast::{
        ArrowFunctionBody, AssignmentOperator, AssignmentTarget, CallExpression, Expression,
        Statement,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn prefer_writable_derived_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Prefer using writable $derived instead of $state and $effect")
        .with_help(
            "Rewrite the `$state` declaration as a writable `$derived` and remove the `$effect`.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferWritableDerived;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers a writable `$derived` over a `$state` variable that is kept in
    /// sync by an `$effect` (or `$effect.pre`) whose only statement reassigns
    /// it.
    ///
    /// ### Why is this bad?
    ///
    /// Since Svelte 5.25 `$derived` values are writable, so a `$state` +
    /// `$effect` pair that only mirrors another expression is redundant: it
    /// runs an extra effect on every change, is harder to read, and can cause
    /// waterfalls of updates. A writable `$derived` expresses the same intent
    /// declaratively.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   const { albumName } = $props();
    ///
    ///   let newAlbumName = $state(albumName);
    ///   $effect(() => {
    ///     newAlbumName = albumName;
    ///   });
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   const { albumName } = $props();
    ///
    ///   let newAlbumName = $derived(albumName);
    /// </script>
    /// ```
    PreferWritableDerived,
    svelte,
    suspicious,
    suggestion,
    version = "1.80.0",
    short_description = "Prefer `$derived` over `$state` synchronized by `$effect`.",
);

impl Rule for PreferWritableDerived {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_effect_or_effect_pre(call) || call.arguments.len() != 1 {
            return;
        }

        // The argument must be a zero-parameter function with a block body
        // containing exactly one statement.
        let Some(body_statements) = call.arguments[0].as_expression().and_then(get_function_body)
        else {
            return;
        };
        if body_statements.len() != 1 {
            return;
        }

        // That single statement must be `identifier = expression;`.
        let Statement::ExpressionStatement(stmt) = &body_statements[0] else {
            return;
        };
        let Expression::AssignmentExpression(assignment) = &stmt.expression else {
            return;
        };
        if assignment.operator != AssignmentOperator::Assign {
            return;
        }
        let AssignmentTarget::AssignmentTargetIdentifier(left) = &assignment.left else {
            return;
        };

        // The assigned variable must be declared as `let x = $state(...)`.
        let Some(symbol_id) = ctx.scoping().get_reference(left.reference_id()).symbol_id() else {
            return;
        };
        let declaration_node = ctx.nodes().get_node(ctx.scoping().symbol_declaration(symbol_id));
        let AstKind::VariableDeclarator(declarator) = declaration_node.kind() else {
            return;
        };
        let Some(init) = &declarator.init else {
            return;
        };
        let Expression::CallExpression(init_call) = init else {
            return;
        };
        if !matches!(&init_call.callee, Expression::Identifier(ident) if ident.name == "$state") {
            return;
        }

        let right_span = assignment.right.span();
        let init_span = init.span();
        let effect_span = call.span;
        ctx.diagnostic_with_suggestion(
            prefer_writable_derived_diagnostic(declarator.span),
            |fixer| {
                let fixer = fixer.for_multifix();
                let derived = format!("$derived({})", fixer.source_range(right_span));
                fixer
                    .new_fix_with_capacity(2)
                    .extend(fixer.replace(init_span, derived))
                    .extend(fixer.delete_range(effect_span))
                    .with_message("Rewrite $state and $effect to $derived")
            },
        );
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Is this a call to `$effect` or `$effect.pre`?
fn is_effect_or_effect_pre(call: &CallExpression) -> bool {
    match &call.callee {
        Expression::Identifier(ident) => ident.name == "$effect",
        Expression::StaticMemberExpression(member) => {
            member.property.name == "pre"
                && matches!(&member.object, Expression::Identifier(ident) if ident.name == "$effect")
        }
        _ => false,
    }
}

/// Returns the statements of a zero-parameter function or arrow function with
/// a block body, or `None` for anything else.
fn get_function_body<'a, 'b>(expr: &'b Expression<'a>) -> Option<&'b [Statement<'a>]> {
    match expr {
        Expression::FunctionExpression(func) => {
            if !func.params.items.is_empty() || func.params.rest.is_some() {
                return None;
            }
            func.body.as_ref().map(|body| body.statements.as_slice())
        }
        Expression::ArrowFunctionExpression(arrow) => {
            if !arrow.params.items.is_empty() || arrow.params.rest.is_some() {
                return None;
            }
            match &arrow.body {
                ArrowFunctionBody::FunctionBody(body) => Some(body.statements.as_slice()),
                // Concise expression bodies are not matched, like upstream.
                _ => None,
            }
        }
        _ => None,
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        // The effect body has more than one statement.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    if (albumName === '') {
                        return;
                    }
                    newAlbumName = albumName;
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        // The single statement is not a plain assignment.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);

                $effect(() => {
                    if (albumName === '') {
                        newAlbumName = albumName + albumName;
                    } else {
                        newAlbumName = albumName;
                    }
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        // Compound assignment operators are not convertible.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    newAlbumName += albumName;
                });
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // The assigned variable is not `$state`.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = albumName;
                $effect(() => {
                    newAlbumName = albumName;
                });
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // `$state.raw` is not plain `$state`; upstream only matches a
        // `$state(...)` call as the initializer.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state.raw(albumName);
                $effect(() => {
                    newAlbumName = albumName;
                });
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // The effect callback takes parameters.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect((x) => {
                    newAlbumName = albumName;
                });
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Expression-bodied arrow functions are not matched, like upstream.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => (newAlbumName = albumName));
            </script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    newAlbumName = albumName;
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    newAlbumName = albumName + albumName;
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect.pre(() => {
                    newAlbumName = albumName;
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect.pre(() => {
                    newAlbumName = albumName + albumName;
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        // Reassignments elsewhere do not matter; the $state/$effect pair is
        // still reported.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    newAlbumName = albumName;
                });

                setInterval(() => {
                    newAlbumName = albumName + albumName;
                }, 1000);
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        // Two matching effects produce two reports on the same declarator.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    newAlbumName = albumName;
                });

                $effect(() => {
                    newAlbumName = albumName;
                });
            </script>

            <input bind:value={newAlbumName} />",
            None,
            None,
            svelte_path(),
        ),
        // Function expression callbacks are matched too.
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(function () {
                    newAlbumName = albumName;
                });
            </script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fix = vec![
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect(() => {
                    newAlbumName = albumName;
                });
            </script>",
            "<script>
                const { albumName } = $props();

                let newAlbumName = $derived(albumName);
                ;
            </script>",
            None,
            Some(PathBuf::from("test.svelte")),
        ),
        (
            "<script>
                const { albumName } = $props();

                let newAlbumName = $state(albumName);
                $effect.pre(() => {
                    newAlbumName = albumName + albumName;
                });
            </script>",
            "<script>
                const { albumName } = $props();

                let newAlbumName = $derived(albumName + albumName);
                ;
            </script>",
            None,
            Some(PathBuf::from("test.svelte")),
        ),
    ];

    Tester::new(PreferWritableDerived::NAME, PreferWritableDerived::PLUGIN, pass, fail)
        .expect_fix(fix)
        .test_and_snapshot();
}
