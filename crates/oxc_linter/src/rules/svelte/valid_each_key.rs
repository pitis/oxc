use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_ast_visit::Visit;
use oxc_diagnostics::OxcDiagnostic;
use oxc_ecmascript::BoundNames;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashSet;
use svelte_markup_parser::ast::{BlockKind, EachBlock, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn valid_each_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Expected key to use the variables which are defined by the `{#each}` block.",
    )
    .with_help(
        "Key the block by its own item or index, e.g. `(item.id)`; a key built from outside state or a constant cannot distinguish the rows.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidEachKey;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the `(key)` of a keyed `{#each}` block to use at least one
    /// of the variables the block itself defines — the item pattern or the
    /// index.
    ///
    /// ### Why is this bad?
    ///
    /// The key exists to give each row a per-item identity. A key that
    /// ignores the block's own variables — a constant, or a variable from
    /// outside the block — evaluates the same for every row (or changes for
    /// all rows at once), so Svelte cannot track which item moved where and
    /// the keyed block silently degrades to positional reconciliation.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {#each items as item (outside)}
    ///   <Row {item} />
    /// {/each}
    /// {#each items as item (1)}
    ///   <Row {item} />
    /// {/each}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {#each items as item (item.id)}
    ///   <Row {item} />
    /// {/each}
    /// {#each items as item, i (i)}
    ///   <Row {item} />
    /// {/each}
    /// ```
    ValidEachKey,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Enforce keys in `{#each}` blocks to use the block's own variables.",
);

impl Rule for ValidEachKey {}

// Ports eslint-plugin-svelte's `valid-each-key`.
//
// Upstream resolves the key's identifiers with full scope analysis and
// checks whether any of them is a variable whose definition sits inside the
// block's item pattern or index (it also admits definitions inside the
// iterated expression — an exotic case this port skips). Here the key and
// the item pattern are parsed in isolation with `oxc_parser` and compared by
// name, with no scope analysis, so an identifier in the key that shadows or
// coincides with an each variable's name (e.g. an arrow parameter inside the
// key expression) counts as a use of it.
impl SvelteTemplateRule for ValidEachKey {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut spans = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| {
            if let Node::Block(block) = node
                && let BlockKind::Each(each) = &block.kind
                && let Some(span) = check_each_key(each)
            {
                spans.push(span);
            }
        });
        for span in spans {
            ctx.diagnostic(valid_each_key_diagnostic(span));
        }
    }
}

/// The span to report for a keyed each block whose key uses none of the
/// block's variables, or `None` when the block is fine (or unkeyed, or not
/// statically analyzable).
fn check_each_key(each: &EachBlock<'_>) -> Option<Span> {
    let key = each.key.as_ref()?;
    let (key_text, key_span) = key.trimmed();
    if key_text.is_empty() {
        return None;
    }

    let mut block_variables: FxHashSet<&str> = FxHashSet::default();
    if let Some(context) = &each.context {
        collect_pattern_names(context.trimmed().0, &mut block_variables);
    }
    if let Some(index) = &each.index {
        let (index_text, _) = index.trimmed();
        if !index_text.is_empty() {
            block_variables.insert(index_text);
        }
    }

    let allocator = Allocator::new();
    let Ok(key_expression) = Parser::new(&allocator, key_text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
    else {
        // Upstream never gets this far on unparsable input.
        return None;
    };

    let mut collector = ReferenceCollector { names: Vec::new() };
    collector.visit_expression(&key_expression);
    if collector.names.iter().any(|name| block_variables.contains(name.as_str())) {
        None
    } else {
        Some(key_span)
    }
}

/// Collect the names bound by an `as` pattern (`item`, `{ id, name }`,
/// `[a, b]`, …) by parsing it as an arrow-function parameter. A pattern
/// that does not parse contributes no names.
fn collect_pattern_names<'s>(pattern_text: &'s str, names: &mut FxHashSet<&'s str>) {
    if pattern_text.is_empty() {
        return;
    }
    let allocator = Allocator::new();
    let wrapped = format!("({pattern_text}) => 0");
    let Ok(expression) = Parser::new(&allocator, &wrapped, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
    else {
        return;
    };
    if let Expression::ArrowFunctionExpression(arrow) = &expression {
        arrow.params.bound_names(&mut |identifier| {
            // The parsed names are slices of `wrapped`, whose pattern part
            // starts at byte 1; map them back onto `pattern_text` so they
            // outlive this function's allocator.
            let start = (identifier.span.start as usize).saturating_sub(1);
            let end = (identifier.span.end as usize).saturating_sub(1);
            if let Some(name) = pattern_text.get(start..end) {
                names.insert(name);
            }
        });
    }
}

struct ReferenceCollector {
    names: Vec<String>,
}

impl<'a> Visit<'a> for ReferenceCollector {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.push(it.name.as_str().to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidEachKey;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                "{#each things as thing (thing.id)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Destructured item pattern.
            (
                "{#each things as { id, name } (id)}{name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each pairs as [a, b] (a)}{b}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // The index is a block variable too.
            (
                "{#each things as thing, index (index)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Any expression using a block variable is fine.
            (
                "{#each things as thing (fn(thing))}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each things as thing (`thing_id=${thing.id}`)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each things as thing (foo + thing.id)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Unkeyed blocks are `svelte/require-each-key`'s concern.
            (
                "{#each things as thing}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Svelte 5 item-less forms have no key to check.
            (
                "{#each { length: 8 }, rank}{#each { length: 8 }}{rank}{/each}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested blocks: each key uses its own block's variables.
            (
                "{#each rows as row (row.id)}{#each row.cells as cell (cell.id)}<td>{cell}</td>{/each}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            // A variable from outside the block.
            (
                "{#each things as thing (foo)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // An `{@const}` from the block body is not an iteration variable.
            (
                "{#each things as thing (key)}{@const key = thing.id}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Constants evaluate the same for every row.
            (
                "{#each things as thing (1)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each things as thing ('x')}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each things as thing, i (foo.bar)}{thing.name}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // The outer block's variable does not key the inner block.
            (
                "{#each rows as row (row.id)}{#each row.cells as cell (row.id)}<td>{cell}</td>{/each}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(ValidEachKey::NAME, ValidEachKey::PLUGIN, pass, fail).test_and_snapshot();
    }
}
