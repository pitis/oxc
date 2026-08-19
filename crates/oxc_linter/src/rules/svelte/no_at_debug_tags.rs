use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{Node, TagKind};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn no_at_debug_tags_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected `{@debug}`.")
        .with_help("`{@debug}` pauses execution in devtools; remove it before shipping.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAtDebugTags;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the use of `{@debug}` tags.
    ///
    /// ### Why is this bad?
    ///
    /// `{@debug}` is a development aid: it logs its arguments on change and
    /// pauses execution in a `debugger` statement when devtools are open.
    /// Like `console.log` and `debugger`, it should not ship to production.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {@debug user}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <p>{user.name}</p>
    /// ```
    NoAtDebugTags,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow use of `{@debug}`.",
);

impl Rule for NoAtDebugTags {}

impl SvelteTemplateRule for NoAtDebugTags {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut spans = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| {
            if let Node::Tag(tag) = node
                && tag.kind == TagKind::Debug
            {
                spans.push(tag.span);
            }
        });
        for span in spans {
            ctx.diagnostic(no_at_debug_tags_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoAtDebugTags;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<p>{user.name}</p>", None, None, Some(PathBuf::from("test.svelte"))),
            ("{@html content}", None, None, Some(PathBuf::from("test.svelte"))),
        ];
        let fail = vec![
            ("{@debug user}", None, None, Some(PathBuf::from("test.svelte"))),
            // `{@debug}` with no arguments still pauses execution.
            ("{@debug}", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "{#if a}<div>{@debug a, b}</div>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoAtDebugTags::NAME, NoAtDebugTags::PLUGIN, pass, fail).test_and_snapshot();
    }
}
