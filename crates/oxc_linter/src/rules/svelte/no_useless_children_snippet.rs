use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{BlockKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_useless_children_snippet_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Found an unnecessary children snippet.")
        .with_help(
            "Remove the `{#snippet children()}` wrapper and place its content directly inside the element.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoUselessChildrenSnippet;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows an explicit parameterless `{#snippet children()}` block
    /// written directly inside an element.
    ///
    /// ### Why is this bad?
    ///
    /// In Svelte 5, content placed directly inside a component is
    /// implicitly passed as its `children` snippet. Wrapping that content
    /// in an explicit `{#snippet children()}` block with no parameters
    /// changes nothing — the wrapper is redundant noise. Only snippets
    /// with parameters, or with a different name, need to be explicit.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <Foo>
    ///   {#snippet children()}
    ///     Hello
    ///   {/snippet}
    /// </Foo>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <Foo>Hello</Foo>
    ///
    /// <Foo>
    ///   {#snippet children(val)}
    ///     Hello {val}
    ///   {/snippet}
    /// </Foo>
    /// ```
    NoUselessChildrenSnippet,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Disallow useless `children` snippets.",
);

impl Rule for NoUselessChildrenSnippet {}

impl SvelteTemplateRule for NoUselessChildrenSnippet {
    // Ports eslint-plugin-svelte's `no-useless-children-snippet`: report a
    // snippet block whose parent is an element, whose name is exactly
    // `children`, and which declares no parameters. A top-level `children`
    // snippet (or one nested in a block) is legitimate — it can be rendered
    // explicitly with `{@render children()}` — so only direct element
    // children are checked, matching upstream.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut spans = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for child in &element.children {
                if let Node::Block(block) = child
                    && let BlockKind::Snippet(snippet) = &block.kind
                    && snippet.name == "children"
                    // `params` is `None` when no parens were written
                    // (invalid but recovered); treat that as parameterless
                    // too.
                    && snippet.params.as_ref().is_none_or(|params| params.trimmed().0.is_empty())
                {
                    // Upstream labels the whole snippet block; we label just
                    // the `{#snippet children()}` header (same start
                    // position) to keep the diagnostic focused.
                    spans.push(snippet.header_span);
                }
            }
        });
        for span in spans {
            ctx.diagnostic(no_useless_children_snippet_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUselessChildrenSnippet;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Implicit children content.
            ("<Foo>\n  Hello\n</Foo>", None, None, Some(PathBuf::from("test.svelte"))),
            // A differently named snippet must stay explicit.
            (
                "<Foo>\n  {#snippet bar()}\n    Hello\n  {/snippet}\n</Foo>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A `children` snippet with parameters must stay explicit.
            (
                "<Foo>\n  {#snippet children(val)}\n    Hello {val}\n  {/snippet}\n</Foo>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A standalone top-level `children` snippet is legitimate.
            (
                "{#snippet children()}\n  Hello\n{/snippet}\n\n{@render children()}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside a block, the snippet's parent is not an element.
            (
                "{#if a}{#snippet children()}Hello{/snippet}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<Foo>{#if a}{#snippet children()}Hello{/snippet}{/if}</Foo>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            (
                "<Foo>\n  {#snippet children()}\n    Hello\n  {/snippet}\n</Foo>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Upstream checks for any element parent, not just components.
            (
                "<div>{#snippet children()}Hello{/snippet}</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Whitespace-only parens still declare no parameters.
            (
                "<Foo>{#snippet children(  )}Hello{/snippet}</Foo>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // The element itself may sit anywhere, including inside blocks.
            (
                "{#each items as item}<Foo>{#snippet children()}{item}{/snippet}</Foo>{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoUselessChildrenSnippet::NAME, NoUselessChildrenSnippet::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
