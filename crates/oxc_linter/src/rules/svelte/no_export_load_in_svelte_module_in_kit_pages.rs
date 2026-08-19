use std::{ffi::OsStr, path::Path};

use oxc_ast::{
    AstKind,
    ast::{BindingPattern, Declaration},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_export_load_diagnostic(span: Span) -> OxcDiagnostic {
    // Upstream's message text.
    OxcDiagnostic::warn(
        "disallow exporting load functions in `*.svelte` module in SvelteKit page components.",
    )
    .with_help(
        "Move the `load` function into the route's `+page.js` / `+page.server.js` (or the `+layout` / `+error` equivalent).",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoExportLoadInSvelteModuleInKitPages;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows exporting a `load` function from a `<script>` block of a
    /// SvelteKit route component (`+page.svelte`, `+layout.svelte`,
    /// `+error.svelte`).
    ///
    /// ### Why is this bad?
    ///
    /// In SvelteKit 1+, `load` functions belong in the route's `+page.js` /
    /// `+page.server.js` (or `+layout(.server).js`) file, not in the
    /// component. A `load` exported from the component's
    /// `<script context="module">` block — the Sapper / pre-1.0 SvelteKit
    /// pattern — is silently ignored, so the page renders without the data
    /// the author expected to load.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <!-- +page.svelte -->
    /// <script context="module">
    ///   export function load() {}
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <!-- +page.svelte -->
    /// <script context="module">
    ///   export function foo() {}
    /// </script>
    ///
    /// <script>
    ///   function load() {} // not exported
    /// </script>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// - Upstream resolves the SvelteKit routes directory from
    ///   `svelte.config.js` and checks that `@sveltejs/kit` is installed;
    ///   oxlint uses a path heuristic instead: the file must be named
    ///   `+page.svelte` / `+layout.svelte` / `+error.svelte` and live under a
    ///   `src/routes` directory.
    /// - Upstream only reports exports from the `<script context="module">`
    ///   (or `<script module>`) block. oxlint's Svelte script pass does not
    ///   expose which block an extracted script came from, so exported `load`
    ///   declarations are reported in *any* `<script>` block of a route
    ///   component. Exporting `load` from the instance script of a route
    ///   component (a Svelte 4 accessor) is not meaningful either, so the
    ///   widened check catches the same mistake.
    NoExportLoadInSvelteModuleInKitPages,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow exporting `load` from a module script in SvelteKit pages.",
);

/// Upstream (`svelte-context.ts`) resolves the routes directory from
/// `svelte.config.js` / plugin settings and requires `@sveltejs/kit` to be
/// installed; oxlint resolves neither, so this reproduces the file-path part
/// of `isKitPageComponent` heuristically.
fn is_kit_route_component(path: &Path) -> bool {
    if !path
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| matches!(name, "+page.svelte" | "+layout.svelte" | "+error.svelte"))
    {
        return false;
    }
    let components: Vec<&OsStr> = path.iter().collect();
    components.windows(2).any(|window| window[0] == "src" && window[1] == "routes")
}

impl Rule for NoExportLoadInSvelteModuleInKitPages {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ExportDeclaration(export) = node.kind() else {
            return;
        };
        // Upstream matches `ExportNamedDeclaration > FunctionDeclaration` and
        // `ExportNamedDeclaration > VariableDeclaration > VariableDeclarator`
        // with an `Identifier` id named `load`, reporting on the identifier.
        // Re-exports (`export { load }`) are deliberately not matched — they
        // parse as `ExportNamedDeclaration`, a separate node here.
        match &export.declaration {
            Declaration::FunctionDeclaration(function) => {
                if let Some(id) = &function.id
                    && id.name == "load"
                {
                    ctx.diagnostic(no_export_load_diagnostic(id.span));
                }
            }
            Declaration::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id
                        && id.name == "load"
                    {
                        ctx.diagnostic(no_export_load_diagnostic(id.span));
                    }
                }
            }
            _ => {}
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
            && is_kit_route_component(ctx.file_path())
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let page_path = || Some(PathBuf::from("src/routes/+page.svelte"));

    let pass = vec![
        // Not exported.
        ("<script context=\"module\">\n\tfunction load() {}\n</script>", None, None, page_path()),
        (
            "<script context=\"module\">\n\tconst load = () => {};\n</script>",
            None,
            None,
            page_path(),
        ),
        // Exported, but not named `load`.
        (
            "<script context=\"module\">\n\texport function foo() {}\n</script>",
            None,
            None,
            page_path(),
        ),
        // `load` only appears as a parameter or a reference.
        (
            "<script context=\"module\">\n\tfunction load() {}\n\texport function foo(load) {}\n</script>",
            None,
            None,
            page_path(),
        ),
        (
            "<script context=\"module\">\n\tfunction load() {}\n\texport const foo = load;\n</script>",
            None,
            None,
            page_path(),
        ),
        // Nested `load` declarations inside an exported function are fine.
        (
            "<script context=\"module\">\n\texport function fn() {\n\t\tfunction load() {}\n\t}\n</script>",
            None,
            None,
            page_path(),
        ),
        (
            "<script context=\"module\">\n\texport function fn() {\n\t\tconst load = () => {};\n\t}\n</script>",
            None,
            None,
            page_path(),
        ),
        // Upstream does not match re-exports or default exports.
        (
            "<script context=\"module\">\n\tfunction load() {}\n\texport { load };\n</script>",
            None,
            None,
            page_path(),
        ),
        (
            "<script context=\"module\">\n\texport default function load() {}\n</script>",
            None,
            None,
            page_path(),
        ),
        // Destructured exports are not `load` declarations.
        (
            "<script context=\"module\">\n\texport const { load: renamed } = whatever;\n</script>",
            None,
            None,
            page_path(),
        ),
        // Not a kit route component: wrong file name.
        (
            "<script context=\"module\">\n\texport function load() {}\n</script>",
            None,
            None,
            Some(PathBuf::from("src/routes/Component.svelte")),
        ),
        // Not a kit route component: not under `src/routes`.
        (
            "<script context=\"module\">\n\texport function load() {}\n</script>",
            None,
            None,
            Some(PathBuf::from("src/lib/+page.svelte")),
        ),
        (
            "<script context=\"module\">\n\texport const load = () => {};\n</script>",
            None,
            None,
            Some(PathBuf::from("+page.svelte")),
        ),
    ];

    let fail = vec![
        (
            "<script context=\"module\">\n\texport function load() {}\n</script>",
            None,
            None,
            page_path(),
        ),
        (
            "<script context=\"module\">\n\texport const load = () => {};\n</script>",
            None,
            None,
            page_path(),
        ),
        // Svelte 5 `<script module>` form.
        ("<script module>\n\texport function load() {}\n</script>", None, None, page_path()),
        // TypeScript module script.
        (
            "<script context=\"module\" lang=\"ts\">\n\texport const load = async () => {};\n</script>",
            None,
            None,
            page_path(),
        ),
        // `export let` / multiple declarators also declare `load`.
        (
            "<script context=\"module\">\n\texport const foo = 1, load = () => {};\n</script>",
            None,
            None,
            page_path(),
        ),
        // Layout and error route components are also checked.
        (
            "<script context=\"module\">\n\texport function load() {}\n</script>",
            None,
            None,
            Some(PathBuf::from("src/routes/blog/+layout.svelte")),
        ),
        (
            "<script context=\"module\">\n\texport function load() {}\n</script>",
            None,
            None,
            Some(PathBuf::from("app/src/routes/+error.svelte")),
        ),
        // DEVIATION: upstream only reports module scripts; oxlint cannot tell
        // which extracted script block was the module one, so exported `load`
        // in an instance script is reported too (upstream treats these as
        // valid Svelte 4 accessor exports).
        ("<script>\n\texport function load() {}\n</script>", None, None, page_path()),
        ("<script>\n\texport const load = () => {};\n</script>", None, None, page_path()),
        (
            "<script context=\"module\">\n\texport function foo() {}\n</script>\n\n<script>\n\texport function load() {}\n</script>",
            None,
            None,
            page_path(),
        ),
    ];

    Tester::new(
        NoExportLoadInSvelteModuleInKitPages::NAME,
        NoExportLoadInSvelteModuleInKitPages::PLUGIN,
        pass,
        fail,
    )
    .test_and_snapshot();
}
