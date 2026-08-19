use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule};

fn no_named_export_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Named exports are not allowed.")
        .with_help("Replace named exports with a single export default to ensure a consistent module entry point.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoNamedExport;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prohibit named exports.
    ///
    /// ### Why is this bad?
    ///
    /// Named exports require strict identifier matching and can lead to fragile imports,
    /// while default exports enforce a single, consistent module entry point.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// export const foo = 'foo';
    ///
    /// const bar = 'bar';
    /// export { bar }
    ///
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// export default 'bar';
    ///
    /// const foo = 'foo';
    /// export { foo as default }
    /// ```
    NoNamedExport,
    import,
    style,
    version = "1.19.0",
    short_description = "Prohibit named exports.",
);

impl Rule for NoNamedExport {
    fn run<'a>(&self, node: &oxc_semantic::AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ExportAllDeclaration(all_decl) => {
                ctx.diagnostic(no_named_export_diagnostic(all_decl.span));
            }
            AstKind::ExportDeclaration(export_decl) => {
                ctx.diagnostic(no_named_export_diagnostic(export_decl.span));
            }
            AstKind::ExportNamedDeclaration(named_decl) => {
                let specifiers = &named_decl.specifiers;
                if specifiers.is_empty() {
                    ctx.diagnostic(no_named_export_diagnostic(named_decl.span));
                }
                if specifiers.iter().any(|specifier| specifier.exported.name() != "default") {
                    ctx.diagnostic(no_named_export_diagnostic(named_decl.span));
                }
            }
            AstKind::ExportFromDeclaration(from_decl)
                if from_decl.specifiers.is_empty()
                    || from_decl
                        .specifiers
                        .iter()
                        .any(|specifier| specifier.exported.name() != "default") =>
            {
                ctx.diagnostic(no_named_export_diagnostic(from_decl.span));
            }
            _ => {}
        }
    }

    fn should_run(&self, ctx: &crate::context::ContextHost) -> bool {
        // `export let prop` / `export const x` in a Svelte `<script>` declares
        // a component prop, not a module export: the component's real export
        // is the compiler-generated default. Reasoning about the script's
        // export shape therefore does not apply.
        !crate::utils::is_svelte_path(ctx.file_path())
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "module.export.foo = function () {}",
        "module.export.foo = function () {}",
        "export default function bar() {};",
        "let foo; export { foo as default }",
        "import * as foo from './foo';",
        "import foo from './foo';",
        "import {default as foo} from './foo';",
        "let foo; export { foo as \"default\" }",
    ];

    let fail = vec![
        "export const foo = 'foo';",
        "
            export const foo = 'foo';
            export default bar;
        ",
        "
            export const foo = 'foo';
            export function bar() {};
        ",
        "export const foo = 'foo';",
        "
            const foo = 'foo';
            export { foo };
        ",
        "let foo, bar; export { foo, bar }",
        "export const { foo, bar } = item;",
        "export const { foo, bar: baz } = item;",
        "export const { foo: { bar, baz } } = item;",
        "
            let item;
            export const foo = item;
            export { item };
        ",
        "export * from './foo';",
        "export const { foo } = { foo: 'bar' };",
        "export const { foo: { bar } } = { foo: { bar: 'baz' } };",
        "export { a, b } from 'foo.js'",
        "export type UserId = number;",
        "export foo from 'foo.js'",
        "export Memory, { MemoryValue } from './Memory'",
    ];

    Tester::new(NoNamedExport::NAME, NoNamedExport::PLUGIN, pass, fail).test_and_snapshot();
}

#[test]
fn test_svelte() {
    use crate::tester::Tester;

    // A Svelte component's export is the compiler-generated default; the
    // script's named exports are props.
    let pass = vec!["<script>\n\texport let label;\n</script>"];

    Tester::new(NoNamedExport::NAME, NoNamedExport::PLUGIN, pass, vec![])
        .change_rule_path("Component.svelte")
        .test();
}
