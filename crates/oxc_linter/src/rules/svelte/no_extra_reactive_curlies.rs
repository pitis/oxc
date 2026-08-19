use oxc_ast::{AstKind, ast::Statement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_extra_reactive_curlies_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Do not wrap a single statement in curly braces.")
        .with_help("Write `$: statement;` without the braces.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoExtraReactiveCurlies;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows wrapping a single-statement reactive block in curly braces.
    ///
    /// ### Why is this bad?
    ///
    /// `$: { x = 1 }` and `$: x = 1` do the same thing; the braces are noise.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   $: {
    ///     doubled = count * 2;
    ///   }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   $: doubled = count * 2;
    ///
    ///   $: {
    ///     doubled = count * 2;
    ///     logged = true;
    ///   }
    /// </script>
    /// ```
    NoExtraReactiveCurlies,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Disallow braces around a single-statement reactive block.",
);

impl Rule for NoExtraReactiveCurlies {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::LabeledStatement(labeled) = node.kind() else {
            return;
        };
        // Only a top-level `$:` is a Svelte reactive statement.
        if labeled.label.name != "$"
            || !matches!(ctx.nodes().parent_kind(node.id()), AstKind::Program(_))
        {
            return;
        }
        let Statement::BlockStatement(block) = &labeled.body else {
            return;
        };
        if block.body.len() == 1 {
            ctx.diagnostic(no_extra_reactive_curlies_diagnostic(block.span));
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
        ("<script>\n\t$: doubled = count * 2;\n</script>", None, None, path()),
        // More than one statement needs the block.
        (
            "<script>\n\t$: {\n\t\tdoubled = count * 2;\n\t\tlogged = true;\n\t}\n</script>",
            None,
            None,
            path(),
        ),
        // An empty block is not a single statement.
        ("<script>\n\t$: {}\n</script>", None, None, path()),
        // A `$` label inside a function is not a reactive statement.
        (
            "<script>\n\tfunction f() {\n\t\t$: {\n\t\t\tx = 1;\n\t\t}\n\t}\n</script>",
            None,
            None,
            path(),
        ),
    ];
    let fail = vec![
        ("<script>\n\t$: {\n\t\tdoubled = count * 2;\n\t}\n</script>", None, None, path()),
        ("<script>\n\t$: { console.log(count); }\n</script>", None, None, path()),
    ];

    Tester::new(NoExtraReactiveCurlies::NAME, NoExtraReactiveCurlies::PLUGIN, pass, fail)
        .test_and_snapshot();
}
