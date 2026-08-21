use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, fixer::RuleFixer, rule::Rule};

/// The `process.*` flags Nuxt exposes as `import.meta.*`.
const PROCESS_SUFFIXES: [&str; 7] =
    ["client", "browser", "server", "nitro", "dev", "test", "prerender"];

fn prefer_import_meta_diagnostic(span: Span, suffix: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Replace `process.{suffix}` with `import.meta.{suffix}`."))
        .with_help(
            "`import.meta` is statically analysable, so the bundler can drop the dead branch.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferImportMeta;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers Nuxt's `import.meta.client` / `import.meta.server` / … over the
    /// equivalent `process.*` flags.
    ///
    /// ### Why is this bad?
    ///
    /// `import.meta.*` is part of the module syntax, so the bundler can
    /// replace it at build time and tree-shake the branch it guards.
    /// `process.*` is a runtime property lookup: the dead branch survives into
    /// the client bundle, taking its imports with it, and `process` may not
    /// exist there at all.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// if (process.client) {
    ///   mount()
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// if (import.meta.client) {
    ///   mount()
    /// }
    /// ```
    PreferImportMeta,
    nuxt,
    style,
    fix,
    version = "1.80.0",
    short_description = "Prefer using `import.meta.*` over `process.*`.",
);

impl Rule for PreferImportMeta {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::StaticMemberExpression(member) = node.kind() else { return };
        let Expression::Identifier(object) = &member.object else { return };
        if object.name != "process" {
            return;
        }
        let suffix = member.property.name.as_str();
        if !PROCESS_SUFFIXES.contains(&suffix) {
            return;
        }
        ctx.diagnostic_with_fix(
            prefer_import_meta_diagnostic(member.span, suffix),
            |fixer: RuleFixer<'_, 'a>| fixer.replace(member.span, format!("import.meta.{suffix}")),
        );
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "if (import.meta.client) {}",
        "if (import.meta.server) {}",
        // Not one of the Nuxt flags.
        "if (process.env.NODE_ENV) {}",
        "if (process.platform) {}",
        // Not `process`.
        "if (ctx.client) {}",
        // A computed access is not the documented spelling.
        "if (process['client']) {}",
    ];

    let fail = vec![
        "if (process.client) {}",
        "if (process.server) {}",
        "if (process.browser) {}",
        "if (process.dev) {}",
        "const x = process.nitro ? 1 : 2",
        "if (process.test || process.prerender) {}",
    ];

    let fix = vec![
        ("if (process.client) {}", "if (import.meta.client) {}", None),
        ("const x = process.dev", "const x = import.meta.dev", None),
    ];

    Tester::new(PreferImportMeta::NAME, PreferImportMeta::PLUGIN, pass, fail)
        .expect_fix(fix)
        .test_and_snapshot();
}
