use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_svelte_internal_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Using svelte/internal is prohibited. This will be removed in Svelte 6.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoSvelteInternal;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows importing from `svelte/internal` (including any
    /// `svelte/internal/*` subpath), whether through static imports,
    /// re-exports, or dynamic `import()` calls.
    ///
    /// ### Why is this bad?
    ///
    /// The `svelte/internal` module exposes private compiler internals that
    /// are not part of Svelte's public API. They can change or disappear in
    /// any release, and the whole module will be removed in Svelte 6, so any
    /// code relying on it will break.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { get_current_component } from 'svelte/internal';
    ///   import { inspect } from 'svelte/internal/client';
    ///   export * from 'svelte/internal';
    ///   import('svelte/internal').then(module => {});
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { mount } from 'svelte';
    ///   import { writable } from 'svelte/store';
    /// </script>
    /// ```
    NoSvelteInternal,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow importing from `svelte/internal`.",
);

fn is_svelte_internal(value: &str) -> bool {
    value == "svelte/internal" || value.starts_with("svelte/internal/")
}

impl Rule for NoSvelteInternal {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ImportDeclaration(decl) => {
                if is_svelte_internal(decl.source.value.as_str()) {
                    ctx.diagnostic(no_svelte_internal_diagnostic(decl.span));
                }
            }
            AstKind::ImportExpression(expr) => {
                if let Expression::StringLiteral(source) = &expr.source
                    && is_svelte_internal(source.value.as_str())
                {
                    ctx.diagnostic(no_svelte_internal_diagnostic(expr.span));
                }
            }
            AstKind::ExportFromDeclaration(decl) => {
                if is_svelte_internal(decl.source.value.as_str()) {
                    ctx.diagnostic(no_svelte_internal_diagnostic(decl.span));
                }
            }
            AstKind::ExportAllDeclaration(decl)
                if is_svelte_internal(decl.source.value.as_str()) =>
            {
                ctx.diagnostic(no_svelte_internal_diagnostic(decl.span));
            }
            _ => {}
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
            "<script>
	import { mount } from 'svelte';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { writable } from 'svelte/store';
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Only exact `svelte/internal` or `svelte/internal/*` match.
        (
            "<script>
	import { foo } from 'svelte/internals';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import('svelte').then(module => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
        // Non-literal dynamic import source is not checked.
        (
            "<script>
	const path = 'svelte/internal';
	import(path);
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	export * from 'svelte/store';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	const internal = 'svelte/internal';
	export { internal };
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<script>
	import { get_current_component } from 'svelte/internal';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import { inspect } from 'svelte/internal/client';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import * as svelteInternal from 'svelte/internal';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	export * from 'svelte/internal';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	export * from 'svelte/internal/client';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	export { inspect } from 'svelte/internal/client';
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import('svelte/internal');
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import('svelte/internal/client').then(module => {});
</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
	import defaultExport from 'svelte/internal';
</script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(NoSvelteInternal::NAME, NoSvelteInternal::PLUGIN, pass, fail).test_and_snapshot();
}
