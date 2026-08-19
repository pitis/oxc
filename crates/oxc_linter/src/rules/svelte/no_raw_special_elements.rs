use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_start_tag_span, walk_svelte_elements},
};

/// Raw HTML element names that Svelte 5 no longer treats specially; each has
/// a `<svelte:...>` counterpart that must be used instead.
const INVALID_HTML_ELEMENTS: [&str; 6] =
    ["head", "body", "window", "document", "element", "options"];

fn no_raw_special_elements_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Special {name} element is deprecated in v5, use svelte:{name} instead."
    ))
    .with_help(format!(
        "Rename `<{name}>` to `<svelte:{name}>`; Svelte 5 renders the raw element literally instead of treating it specially."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoRawSpecialElements;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the raw `<head>`, `<body>`, `<window>`, `<document>`,
    /// `<element>`, and `<options>` HTML elements, which almost certainly
    /// mean the `<svelte:head>`, `<svelte:body>`, `<svelte:window>`,
    /// `<svelte:document>`, `<svelte:element>`, and `<svelte:options>`
    /// special elements.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte 4 accepted some of these raw names as aliases for the special
    /// elements; Svelte 5 deprecates that and renders them as literal DOM
    /// elements. A literal `<head>` or `<body>` nested inside a component
    /// is invalid HTML and silently does nothing like what was intended,
    /// while `<window>`, `<document>`, `<element>`, and `<options>` are not
    /// HTML elements at all.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <head>
    ///   <title>Page title</title>
    /// </head>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <svelte:head>
    ///   <title>Page title</title>
    /// </svelte:head>
    /// ```
    NoRawSpecialElements,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow raw special elements; use `<svelte:...>` instead.",
);

impl Rule for NoRawSpecialElements {}

// Ports eslint-plugin-svelte's `no-raw-special-elements`.
//
// Upstream has an autofix (prepending `svelte:` to the open and close tags);
// the Svelte markup pass does not support fixes yet (see `svelte_template.rs`).
impl SvelteTemplateRule for NoRawSpecialElements {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut reports: Vec<(&str, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Exact match only: `<svelte:head>` ("svelte:head") and
            // components (`<Head>`) have different names and never match,
            // mirroring upstream's `SvelteElement[kind="html"]` selector.
            if INVALID_HTML_ELEMENTS.contains(&element.name) {
                reports.push((element.name, svelte_start_tag_span(element)));
            }
        });
        for (name, span) in reports {
            ctx.diagnostic(no_raw_special_elements_diagnostic(name, span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoRawSpecialElements;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // The svelte: special elements themselves.
            (
                "<svelte:options />\n\n<svelte:body />\n<svelte:document />\n<svelte:element this={{}}></svelte:element>\n<svelte:head></svelte:head>\n\n<svelte:window />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Ordinary elements whose names merely resemble the list.
            ("<div><header>x</header></div>", None, None, Some(PathBuf::from("test.svelte"))),
            // Components are matched by exact (lowercase) name only.
            ("<Head title=\"x\" />", None, None, Some(PathBuf::from("test.svelte"))),
        ];
        let fail = vec![
            ("<head></head>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<body></body>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<window></window>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<document></document>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<element this={{}}></element>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<options></options>", None, None, Some(PathBuf::from("test.svelte"))),
            // Self-closing form.
            ("<body />", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested inside elements and blocks.
            ("<div><head></head></div>", None, None, Some(PathBuf::from("test.svelte"))),
            ("{#if a}<head></head>{/if}", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        Tester::new(NoRawSpecialElements::NAME, NoRawSpecialElements::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
