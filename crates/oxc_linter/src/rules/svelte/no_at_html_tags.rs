use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{Node, TagKind};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn no_at_html_tags_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`{@html}` can lead to XSS attack.")
        .with_help(
            "Rendering raw HTML bypasses Svelte's escaping; sanitize the value or render it as text instead.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAtHtmlTags;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the use of `{@html}` tags.
    ///
    /// ### Why is this bad?
    ///
    /// `{@html}` renders its expression as raw HTML, bypassing Svelte's
    /// automatic escaping. If any part of the value is user-controlled, it
    /// can inject markup and scripts — a cross-site-scripting (XSS) attack.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div>{@html post.content}</div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div>{post.content}</div>
    /// ```
    NoAtHtmlTags,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Disallow use of `{@html}` to prevent XSS attack.",
);

impl Rule for NoAtHtmlTags {}

impl SvelteTemplateRule for NoAtHtmlTags {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut spans = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| {
            if let Node::Tag(tag) = node
                && tag.kind == TagKind::Html
            {
                spans.push(tag.span);
            }
        });
        for span in spans {
            ctx.diagnostic(no_at_html_tags_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoAtHtmlTags;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<div>{content}</div>", None, None, Some(PathBuf::from("test.svelte"))),
            // Other tags are not this rule's concern.
            ("{@debug content}", None, None, Some(PathBuf::from("test.svelte"))),
            // Suppressible via HTML comment directives.
            (
                "<!-- eslint-disable-next-line svelte/no-at-html-tags -->\n{@html content}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            ("<div>{@html post.content}</div>", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested inside blocks.
            (
                "{#if a}{@html a}{:else}{@html b}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each items as item}{@html item}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoAtHtmlTags::NAME, NoAtHtmlTags::PLUGIN, pass, fail).test_and_snapshot();
    }
}
