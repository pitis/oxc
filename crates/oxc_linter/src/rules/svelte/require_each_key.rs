use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{BlockKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn require_each_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Each block should have a key.")
        .with_help(
            "Add a keyed expression, e.g. `{#each items as item (item.id)}`, so Svelte can track each item's identity when the list changes.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireEachKey;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires keyed `{#each}` blocks: `{#each items as item (item.id)}`.
    ///
    /// ### Why is this bad?
    ///
    /// Without a key, Svelte reconciles an `{#each}` list positionally:
    /// when items are inserted, removed, or reordered, existing DOM nodes
    /// and component state are reassigned to *different* items rather than
    /// moved with them. Transitions fire on the wrong rows, inputs keep the
    /// wrong values, and components keep the wrong internal state. A key
    /// lets Svelte track each item's identity.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {#each items as item}
    ///   <Row {item} />
    /// {/each}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {#each items as item (item.id)}
    ///   <Row {item} />
    /// {/each}
    /// ```
    RequireEachKey,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Require keyed `{#each}` blocks.",
);

impl Rule for RequireEachKey {}

impl SvelteTemplateRule for RequireEachKey {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut spans = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| {
            if let Node::Block(block) = node
                && let BlockKind::Each(each) = &block.kind
                && each.key.is_none()
            {
                spans.push(each.header_span);
            }
        });
        for span in spans {
            ctx.diagnostic(require_each_key_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireEachKey;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                "{#each items as item (item.id)}<Row {item} />{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each items as { id, name }, i (id)}<li>{name}</li>{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            (
                "{#each items as item}<Row {item} />{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Svelte 5's item-less form has no key either.
            ("{#each new Array(3)}<hr>{/each}", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested unkeyed each inside a keyed one still reports.
            (
                "{#each rows as row (row.id)}{#each row.cells as cell}<td>{cell}</td>{/each}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(RequireEachKey::NAME, RequireEachKey::PLUGIN, pass, fail).test_and_snapshot();
    }
}
