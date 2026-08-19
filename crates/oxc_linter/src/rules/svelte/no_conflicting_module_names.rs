use std::path::{Path, PathBuf};

use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use svelte_markup_parser::ast::Node;

use crate::{
    context::{ContextHost, LintContext},
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

fn conflict_on_component_diagnostic(module_name: &str, specifier: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The module `{module_name}` has the same name as this component. TypeScript resolves the import `{specifier}` to that module, not to this component."
    ))
    .with_help(format!("Rename `{module_name}`."))
    .with_label(Span::empty(0))
}

fn conflict_on_module_diagnostic(svelte_name: &str, specifier: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "This module has the same name as the component `{svelte_name}`. TypeScript resolves the import `{specifier}` to this module, not to the component."
    ))
    .with_help("Rename this file.")
    .with_label(Span::empty(0))
}

/// The extensions a Svelte "runes module" can carry, as upstream lists them.
const MODULE_EXTENSIONS: [&str; 8] = [".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"];

#[derive(Debug, Default, Clone)]
pub struct NoConflictingModuleNames;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows a `.svelte` component and a same-named runes module — say
    /// `Foo.svelte` next to `Foo.svelte.ts` — from living side by side.
    ///
    /// ### Why is this bad?
    ///
    /// `import Foo from './Foo.svelte'` resolves to `Foo.svelte.ts`, not to
    /// the component, because TypeScript appends the module extension while
    /// resolving. The component silently becomes unreachable under that
    /// specifier.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    ///
    /// A `Foo.svelte` that has a `Foo.svelte.ts` beside it — either file is
    /// reported.
    ///
    /// Examples of **correct** code for this rule:
    ///
    /// Give the module a different stem, e.g. `Foo.svelte` and
    /// `foo-state.svelte.ts`.
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// This rule reads the filesystem — it is the only way to know whether
    /// the sibling file exists — so it does one directory lookup per linted
    /// file while it is enabled. It is off by default. Upstream additionally
    /// recovers the on-disk spelling of the sibling on case-insensitive
    /// filesystems; oxlint reports the name it looked for.
    ///
    /// The component side runs in the Svelte markup pass, so a component
    /// without a `<script>` block is checked too.
    NoConflictingModuleNames,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Disallow a component and a runes module sharing a name.",
);

impl Rule for NoConflictingModuleNames {
    fn run_once(&self, ctx: &LintContext) {
        // The `.svelte` side is handled by the markup pass, which runs even
        // for a component with no `<script>` block.
        if let Some(diagnostic) = module_side_conflict(ctx.file_path()) {
            ctx.diagnostic(diagnostic);
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        let path = ctx.file_path();
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| MODULE_EXTENSIONS.iter().any(|ext| name.ends_with(ext)))
    }
}

impl SvelteTemplateRule for NoConflictingModuleNames {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        if let Some(diagnostic) = component_side_conflict(ctx.path()) {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// `Foo.svelte` with a `Foo.svelte.ts` (or `.js`, …) beside it.
fn component_side_conflict(path: &Path) -> Option<OxcDiagnostic> {
    let name = path.file_name().and_then(std::ffi::OsStr::to_str)?;
    if !name.ends_with(".svelte") {
        return None;
    }
    MODULE_EXTENSIONS.iter().find(|extension| with_suffix(path, extension).is_file()).map(
        |extension| {
            conflict_on_component_diagnostic(&format!("{name}{extension}"), &format!("./{name}"))
        },
    )
}

/// `Foo.svelte.ts` with a `Foo.svelte` beside it.
fn module_side_conflict(path: &Path) -> Option<OxcDiagnostic> {
    let name = path.file_name().and_then(std::ffi::OsStr::to_str)?;
    let extension = MODULE_EXTENSIONS.iter().find(|ext| name.ends_with(**ext))?;
    let component_name = &name[..name.len() - extension.len()];
    if !component_name.ends_with(".svelte") {
        return None;
    }
    path.with_file_name(component_name)
        .is_file()
        .then(|| conflict_on_module_diagnostic(component_name, &format!("./{component_name}")))
}

/// `Foo.svelte` + `.ts` → `Foo.svelte.ts`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    // Real files, because the rule's whole job is to notice a sibling on
    // disk. `Tester` roots relative rule paths at `fixtures/import`.
    let pass = vec![
        // A component with no same-named module beside it.
        ("<div />", None, None, Some(PathBuf::from("svelte-names/Alone.svelte"))),
        // A module whose stem does not end in `.svelte`.
        ("export const x = 1;", None, None, Some(PathBuf::from("svelte-names/plain.ts"))),
        // A component whose sibling module has a different stem.
        ("<div />", None, None, Some(PathBuf::from("svelte-names/Other.svelte"))),
    ];
    let fail = vec![
        ("<div />", None, None, Some(PathBuf::from("svelte-names/Conflict.svelte"))),
        (
            "export const state = 1;",
            None,
            None,
            Some(PathBuf::from("svelte-names/Conflict.svelte.ts")),
        ),
    ];

    Tester::new(NoConflictingModuleNames::NAME, NoConflictingModuleNames::PLUGIN, pass, fail)
        .test_and_snapshot();
}
