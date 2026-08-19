use oxc_allocator::Allocator;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    rules::svelte::no_navigation_without_base::{base_path_names, scan_navigation_calls},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::svelte_scripts,
};

fn no_goto_without_base_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Found a goto() call with a url that isn't prefixed with the base path.")
        .with_help("Prefix the URL with `base` from `$app/paths`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoGotoWithoutBase;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the URL passed to SvelteKit's `goto()` to be prefixed with
    /// `base` from `$app/paths`.
    ///
    /// ### Why is this bad?
    ///
    /// An app served under a base path needs every internal URL to carry that
    /// prefix; a bare `goto('/foo')` navigates out of the app.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// import { goto } from '$app/navigation';
    ///
    /// goto('/foo');
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { goto } from '$app/navigation';
    /// import { base } from '$app/paths';
    ///
    /// goto(`${base}/foo`);
    /// ```
    ///
    /// ### Deprecated
    ///
    /// `eslint-plugin-svelte` deprecates this rule in favour of
    /// [`svelte/no-navigation-without-resolve`]. It is kept so existing
    /// configurations keep resolving.
    ///
    /// [`svelte/no-navigation-without-resolve`]: ./no-navigation-without-resolve.html
    NoGotoWithoutBase,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Disallow `goto()` without the `base` path (deprecated).",
);

impl Rule for NoGotoWithoutBase {}

impl SvelteTemplateRule for NoGotoWithoutBase {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let allocator = Allocator::new();
        let scripts = svelte_scripts(nodes, ctx.source_text());
        let base_names =
            base_path_names(&scripts.iter().map(|s| s.content).collect::<Vec<_>>(), &allocator);

        let mut spans = Vec::new();
        for script in &scripts {
            spans.extend(
                scan_navigation_calls(script.content, script.offset, &allocator, &base_names, true)
                    .into_iter()
                    .map(|(_, span)| span),
            );
        }
        for span in spans {
            ctx.diagnostic(no_goto_without_base_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoGotoWithoutBase;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\timport { base } from '$app/paths';\n\tgoto(`${base}/foo`);\n</script>",
                None,
                None,
                path(),
            ),
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\timport { base } from '$app/paths';\n\tgoto(base + '/foo');\n</script>",
                None,
                None,
                path(),
            ),
            // An external URL carries a scheme.
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\tgoto('https://svelte.dev');\n</script>",
                None,
                None,
                path(),
            ),
            // `pushState` is not this rule's business.
            (
                "<script>\n\timport { pushState } from '$app/navigation';\n\tpushState('/foo', {});\n</script>",
                None,
                None,
                path(),
            ),
        ];
        let fail = vec![
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\tgoto('/foo');\n</script>",
                None,
                None,
                path(),
            ),
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\tgoto(url);\n</script>",
                None,
                None,
                path(),
            ),
        ];

        Tester::new(NoGotoWithoutBase::NAME, NoGotoWithoutBase::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
