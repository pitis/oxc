use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn no_nested_style_tag_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Nested `<style>` elements are not scoped and may lead to unintended styles being applied.",
    )
    .with_help("Move the rules into the component's top-level `<style>` element.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoNestedStyleTag;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `<style>` elements nested inside other elements or blocks.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte only scopes the component's top-level `<style>` element. A
    /// nested one is emitted into the DOM as-is, so its rules apply globally
    /// and can leak into unrelated components.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div>
    ///   <style>p { color: red }</style>
    /// </div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div></div>
    ///
    /// <style>p { color: red }</style>
    /// ```
    NoNestedStyleTag,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow `<style>` nested inside other elements.",
);

impl Rule for NoNestedStyleTag {}

impl SvelteTemplateRule for NoNestedStyleTag {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        // Only the component's own top-level `<style>` is scoped, so walk the
        // children of the root nodes rather than the root nodes themselves.
        let mut spans = Vec::new();
        for node in nodes {
            walk_svelte_nodes(std::slice::from_ref(node), &mut |descendant| {
                // Skip the root node itself; only its descendants are nested.
                if std::ptr::eq(descendant, node) {
                    return;
                }
                if let Node::Element(element) = descendant
                    && element.name.eq_ignore_ascii_case("style")
                {
                    spans.push(element.span);
                }
            });
        }
        for span in spans {
            ctx.diagnostic(no_nested_style_tag_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoNestedStyleTag;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<div></div>\n<style>p { color: red }</style>", None, None, path()),
            // Top level, after other markup.
            ("<style>\n\tp { color: red }\n</style>", None, None, path()),
        ];
        let fail = vec![
            ("<div>\n\t<style>p { color: red }</style>\n</div>", None, None, path()),
            ("{#if a}<style>p { color: red }</style>{/if}", None, None, path()),
        ];

        Tester::new(NoNestedStyleTag::NAME, NoNestedStyleTag::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
