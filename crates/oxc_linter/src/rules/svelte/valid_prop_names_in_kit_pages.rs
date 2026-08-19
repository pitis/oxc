use std::{ffi::OsStr, path::Path};

use oxc_ast::{
    AstKind,
    ast::{BindingPattern, Expression, ObjectPattern, PropertyKey},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

/// Props available on every SvelteKit route component under Svelte 5.
const PAGE_PROP_NAMES: [&str; 4] = ["data", "form", "params", "snapshot"];
/// The Svelte 3/4 (`export let`) prop set, which additionally had `errors`.
const LEGACY_PAGE_PROP_NAMES: [&str; 5] = ["data", "errors", "form", "params", "snapshot"];
/// `+layout.svelte` additionally receives the `children` snippet (Svelte 5).
const LAYOUT_PROP_NAMES: [&str; 5] = ["data", "form", "params", "snapshot", "children"];
/// `+error.svelte` only receives `error` (Svelte 5).
const ERROR_PROP_NAMES: [&str; 1] = ["error"];

fn valid_prop_names_diagnostic(span: Span, allowed: &[&str]) -> OxcDiagnostic {
    // Upstream's message text.
    OxcDiagnostic::warn("disallow invalid props in SvelteKit route components.")
        .with_help(format!(
            "SvelteKit passes a fixed set of props to this route component; only {} available here.",
            list_names(allowed)
        ))
        .with_label(span)
}

fn list_names(names: &[&str]) -> String {
    if let [single] = names {
        return format!("`{single}` is");
    }
    let mut out = String::new();
    for (index, name) in names.iter().enumerate() {
        if index + 1 == names.len() {
            out.push_str(" and ");
        } else if index > 0 {
            out.push_str(", ");
        }
        out.push('`');
        out.push_str(name);
        out.push('`');
    }
    out.push_str(" are");
    out
}

#[derive(Debug, Default, Clone)]
pub struct ValidPropNamesInKitPages;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows declaring props with invalid names in SvelteKit route
    /// components (`+page.svelte`, `+layout.svelte`, `+error.svelte`).
    ///
    /// ### Why is this bad?
    ///
    /// SvelteKit instantiates route components itself and passes them a fixed
    /// set of props: `data`, `form`, `params` and `snapshot` (plus `errors`
    /// in the Svelte 3/4 `export let` style, `children` in a Svelte 5
    /// `+layout.svelte`, and only `error` in a Svelte 5 `+error.svelte`).
    /// Any other prop is never populated, so declaring it is at best dead
    /// code and usually a misunderstanding of how route data flows.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <!-- +page.svelte -->
    /// <script>
    ///   export let foo;
    ///   let { bar } = $props();
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <!-- +page.svelte -->
    /// <script>
    ///   export let data;
    ///   export let form;
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
    /// - Upstream skips the `<script context="module">` block (module
    ///   exports are not props). oxlint's Svelte script pass does not expose
    ///   which block an extracted script came from, so exported declarations
    ///   in a module script are checked like instance-script props.
    /// - Upstream picks the Svelte 5 allow-lists based on the installed
    ///   `svelte` version; oxlint infers Svelte 5 from the `$props()` syntax
    ///   itself.
    ValidPropNamesInKitPages,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow props other than `data`/`errors`/`form` in SvelteKit pages.",
);

/// Upstream (`svelte-context.ts`) resolves the routes directory from
/// `svelte.config.js` / plugin settings and requires `@sveltejs/kit` to be
/// installed; oxlint resolves neither, so this reproduces the file-path part
/// of the kit-page detection heuristically.
fn is_kit_route_component(path: &Path) -> bool {
    if kit_route_file_name(path).is_none() {
        return false;
    }
    let components: Vec<&OsStr> = path.iter().collect();
    components.windows(2).any(|window| window[0] == "src" && window[1] == "routes")
}

fn kit_route_file_name(path: &Path) -> Option<&str> {
    path.file_name()
        .and_then(OsStr::to_str)
        .filter(|name| matches!(*name, "+page.svelte" | "+layout.svelte" | "+error.svelte"))
}

/// Reports every non-rest property whose key is a non-computed identifier
/// outside the allow-list, mirroring upstream's `checkProp`.
fn check_pattern_props(pattern: &ObjectPattern<'_>, allowed: &[&str], ctx: &LintContext<'_>) {
    for property in &pattern.properties {
        if let PropertyKey::StaticIdentifier(key) = &property.key
            && !property.computed
            && !allowed.contains(&key.name.as_str())
        {
            ctx.diagnostic(valid_prop_names_diagnostic(key.span, allowed));
        }
    }
}

impl Rule for ValidPropNamesInKitPages {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::VariableDeclarator(declarator) = node.kind() else {
            return;
        };

        // Svelte 3/4 style: `export let foo;` / `export let { foo } = bar;`.
        let parent = ctx.nodes().parent_node(node.id());
        let grandparent = ctx.nodes().parent_node(parent.id());
        if matches!(grandparent.kind(), AstKind::ExportDeclaration(_)) {
            match &declarator.id {
                BindingPattern::BindingIdentifier(id) => {
                    if !LEGACY_PAGE_PROP_NAMES.contains(&id.name.as_str()) {
                        ctx.diagnostic(valid_prop_names_diagnostic(
                            declarator.span,
                            &LEGACY_PAGE_PROP_NAMES,
                        ));
                    }
                    return;
                }
                BindingPattern::ObjectPattern(pattern) => {
                    check_pattern_props(pattern, &LEGACY_PAGE_PROP_NAMES, ctx);
                }
                _ => {}
            }
        }

        // Svelte 5 style: `let { foo, bar } = $props();`. The `$props()` rune
        // itself implies Svelte 5, so the version-specific allow-lists apply.
        if let Some(Expression::CallExpression(call)) = &declarator.init
            && let Expression::Identifier(callee) = &call.callee
            && callee.name == "$props"
            && let BindingPattern::ObjectPattern(pattern) = &declarator.id
        {
            let allowed: &[&str] = match kit_route_file_name(ctx.file_path()) {
                Some("+layout.svelte") => &LAYOUT_PROP_NAMES,
                Some("+error.svelte") => &ERROR_PROP_NAMES,
                _ => &PAGE_PROP_NAMES,
            };
            check_pattern_props(pattern, allowed, ctx);
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
    let layout_path = || Some(PathBuf::from("src/routes/+layout.svelte"));
    let error_path = || Some(PathBuf::from("src/routes/+error.svelte"));

    let pass = vec![
        // The full legacy prop set, plus a snapshot accessor export.
        (
            "<script>
                export let data;
                export let errors;
                export let form;
                export let params;

                let comment = '';

                export const snapshot = {
                    capture: () => comment,
                    restore: (value) => (comment = value)
                };
            </script>",
            None,
            None,
            page_path(),
        ),
        // Typed props.
        (
            r#"<script lang="ts">
                import type { ActionData } from './$types';
                export let form: ActionData;
            </script>"#,
            None,
            None,
            page_path(),
        ),
        // Destructured export: only the *keys* are checked.
        (
            "<script>
                export let { data: data2, errors: errors2 } = { data: {}, errors: {} };
            </script>",
            None,
            None,
            page_path(),
        ),
        // Svelte 5 runes.
        (
            "<script>
                let { data, form, params } = $props();
            </script>",
            None,
            None,
            page_path(),
        ),
        (
            "<script>
                let { data, children } = $props();
            </script>
            {@render children()}",
            None,
            None,
            layout_path(),
        ),
        (
            "<script>
                let { error } = $props();
            </script>
            <h1>{error.message}</h1>",
            None,
            None,
            error_path(),
        ),
        (
            r#"<script lang="ts">
                let { data: pageData }: { data: PageData } = $props();
            </script>"#,
            None,
            None,
            page_path(),
        ),
        // Rest elements are not checked.
        (
            "<script>
                let { data, ...rest } = $props();
            </script>",
            None,
            None,
            page_path(),
        ),
        // Plain (non-exported, non-`$props`) declarations are not props.
        (
            "<script>
                let foo = 1;
                const { bar } = whatever();
            </script>",
            None,
            None,
            page_path(),
        ),
        // Not a kit route component: wrong file name or not under src/routes.
        (
            "<script>
                export let foo;
                let { bar } = $props();
            </script>",
            None,
            None,
            Some(PathBuf::from("src/routes/Component.svelte")),
        ),
        (
            "<script>
                export let foo;
            </script>",
            None,
            None,
            Some(PathBuf::from("src/lib/+page.svelte")),
        ),
    ];

    let fail = vec![
        // Legacy props with invalid names.
        (
            "<script>
                export let foo;
                export let bar;
                export let { baz, qux } = data;
            </script>",
            None,
            None,
            page_path(),
        ),
        // `children` is not a Svelte 3/4 prop, not even in a layout.
        (
            "<script>
                export let children;
            </script>",
            None,
            None,
            page_path(),
        ),
        (
            "<script>
                export let children;
            </script>",
            None,
            None,
            layout_path(),
        ),
        // Svelte 5 runes with invalid names.
        (
            "<script>
                let { foo, bar } = $props();
            </script>",
            None,
            None,
            page_path(),
        ),
        // `children` is only available in `+layout.svelte`.
        (
            "<script>
                let { data, children } = $props();
            </script>",
            None,
            None,
            page_path(),
        ),
        // `+error.svelte` only receives `error` under Svelte 5.
        (
            "<script>
                let { error, children } = $props();
            </script>",
            None,
            None,
            error_path(),
        ),
        (
            "<script>
                let { data } = $props();
            </script>",
            None,
            None,
            error_path(),
        ),
        // `export var` and `export const` declare props too.
        (
            "<script>
                export var foo;
            </script>",
            None,
            None,
            page_path(),
        ),
        // DEVIATION: upstream skips `<script context=\"module\">` blocks;
        // oxlint cannot tell which extracted script block was the module one,
        // so module-script exports are checked like instance-script props.
        (
            "<script context=\"module\">
                export let data;
                export let foo;
            </script>",
            None,
            None,
            page_path(),
        ),
    ];

    Tester::new(ValidPropNamesInKitPages::NAME, ValidPropNamesInKitPages::PLUGIN, pass, fail)
        .test_and_snapshot();
}
