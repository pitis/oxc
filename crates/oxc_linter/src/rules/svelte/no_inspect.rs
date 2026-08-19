use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_inspect_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Do not use $inspect directive").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoInspect;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Warns against the use of the `$inspect` rune.
    ///
    /// ### Why is this bad?
    ///
    /// `$inspect` is a debugging tool: it logs its arguments whenever they
    /// change, and only works in development mode. Leaving `$inspect` calls
    /// in the code is a sign of forgotten debugging statements that should be
    /// removed before shipping to production.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let count = $state(0);
    ///   $inspect(count);
    ///   $inspect(count).with(console.trace);
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let count = $state(0);
    /// </script>
    /// ```
    NoInspect,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Warn against `$inspect` in production code.",
);

impl Rule for NoInspect {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // Upstream reports every identifier named `$inspect`, wherever it
        // appears (reference, binding, or member property).
        let span = match node.kind() {
            AstKind::IdentifierReference(ident) if ident.name == "$inspect" => ident.span,
            AstKind::BindingIdentifier(ident) if ident.name == "$inspect" => ident.span,
            AstKind::IdentifierName(ident) if ident.name == "$inspect" => ident.span,
            _ => return,
        };
        ctx.diagnostic(no_inspect_diagnostic(span));
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
            "<script>
  const _ = $state(1);
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
  let count = $state(0);
  const doubled = $derived(count * 2);
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Similar names are not reported.
        (
            "<script>
  inspect(1);
  const inspector = { inspect: () => {} };
  inspector.inspect(1);
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<script>
  $inspect(1);
  $state(0);

  const a = $inspect(1);

  const _ = () => {
    $inspect(1);
  }
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
  let count = $state(0);
  $inspect(count).with(console.trace);
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
  $inspect.trace();
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(NoInspect::NAME, NoInspect::PLUGIN, pass, fail).test_and_snapshot();
}
