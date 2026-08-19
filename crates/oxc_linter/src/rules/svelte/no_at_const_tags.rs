use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{Node, TagKind};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{SVELTE_RUNES, svelte_scripts, walk_svelte_nodes},
};

fn no_at_const_tags_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Use `{const ...}` declaration tag instead of legacy `{@const ...}`.")
        .with_help(
            "Drop the `@`, and wrap the initializer in `$derived(...)` if it should stay reactive.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAtConstTags;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers the `{const ...}` declaration tag over the legacy
    /// `{@const ...}` tag.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte 5.56 introduced `{const ...}`, which may appear anywhere in the
    /// markup rather than only as the immediate child of a block, and which
    /// is not implicitly reactive. `{@const ...}` is kept for compatibility.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {#each items as item}
    ///   {@const total = item.a + item.b}
    ///   <p>{total}</p>
    /// {/each}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {#each items as item}
    ///   {const total = $derived(item.a + item.b)}
    ///   <p>{total}</p>
    /// {/each}
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream only runs when the installed Svelte is 5.56 or newer *and*
    /// the component is in runes mode. oxlint cannot read the installed
    /// version, so it applies the runes half of that test only: the rule
    /// fires when the component's `<script>` uses a rune. The rule is off by
    /// default.
    NoAtConstTags,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Prefer `{const ...}` over the legacy `{@const ...}` tag.",
);

impl Rule for NoAtConstTags {}

impl SvelteTemplateRule for NoAtConstTags {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        if !uses_runes(nodes, ctx.source_text()) {
            return;
        }
        let mut spans = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| {
            if let Node::Tag(tag) = node
                && tag.kind == TagKind::Const
            {
                spans.push(tag.span);
            }
        });
        for span in spans {
            ctx.diagnostic(no_at_const_tags_diagnostic(span));
        }
    }
}

/// Whether any `<script>` block mentions a rune, the local stand-in for
/// upstream's project-wide runes detection.
fn uses_runes(nodes: &[Node<'_>], source_text: &str) -> bool {
    svelte_scripts(nodes, source_text).iter().any(|script| {
        SVELTE_RUNES.iter().any(|rune| {
            script.content.match_indices(rune).any(|(index, _)| {
                // Require a call or member access so a mention inside a
                // longer identifier (`my$state`) does not count.
                let before_ok = index == 0
                    || !script.content.as_bytes()[index - 1].is_ascii_alphanumeric()
                        && script.content.as_bytes()[index - 1] != b'_';
                let after = script.content[index + rune.len()..].trim_start();
                before_ok && (after.starts_with('(') || after.starts_with('.'))
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoAtConstTags;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            // Not a runes component: upstream would not run at all.
            (
                "<script>\n\texport let items;\n</script>\n{#each items as item}{@const t = item.a}{t}{/each}",
                None,
                None,
                path(),
            ),
            // Runes component with no `{@const}`.
            (
                "<script>\n\tlet { items } = $props();\n</script>\n{#each items as item}<p>{item}</p>{/each}",
                None,
                None,
                path(),
            ),
            // A name that merely contains a rune name is not a rune call.
            (
                "<script>\n\tlet my$state = 1;\n</script>\n{#each items as item}{@const t = item.a}{t}{/each}",
                None,
                None,
                path(),
            ),
        ];
        let fail = vec![
            (
                "<script>\n\tlet { items } = $props();\n</script>\n{#each items as item}{@const t = item.a + item.b}<p>{t}</p>{/each}",
                None,
                None,
                path(),
            ),
            (
                "<script>\n\tlet count = $state(0);\n</script>\n{#if count}{@const doubled = count * 2}{doubled}{/if}",
                None,
                None,
                path(),
            ),
        ];

        Tester::new(NoAtConstTags::NAME, NoAtConstTags::PLUGIN, pass, fail).test_and_snapshot();
    }
}
