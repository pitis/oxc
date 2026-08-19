use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{ContentEq, GetSpan, SourceType, Span};
use oxc_syntax::operator::LogicalOperator;
use svelte_markup_parser::ast::{BlockKind, ExpressionSlot, IfBlock, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

fn no_dupe_else_if_blocks_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "This branch can never execute. Its condition is a duplicate or covered by previous conditions in the `{#if}` / `{:else if}` chain.",
    )
    .with_help("Remove the duplicate branch, or change its condition so it covers a distinct case.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDupeElseIfBlocks;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplicate conditions in `{#if}` / `{:else if}` chains: a
    /// later branch whose condition is identical to, or logically covered by
    /// (in the "each `||`-operand's `&&`-operands form a subset" sense), an
    /// earlier branch's condition can never run.
    ///
    /// ### Why is this bad?
    ///
    /// An `{:else if}` branch only runs once every earlier branch's
    /// condition evaluated to `false`. If its own condition is implied by
    /// one of those (a duplicate, or a superset of an earlier `&&` clause,
    /// or an `||` operand that already appeared earlier), the branch can
    /// never execute — almost always a copy-paste mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {#if a}
    ///   <div />
    /// {:else if a}
    ///   <div />
    /// {/if}
    ///
    /// {#if a}
    ///   <div />
    /// {:else if a && b}
    ///   <div />
    /// {/if}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {#if a}
    ///   <div />
    /// {:else if b}
    ///   <div />
    /// {/if}
    /// ```
    NoDupeElseIfBlocks,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow duplicate conditions in `{#if}` / `{:else if}` chains.",
);

impl Rule for NoDupeElseIfBlocks {}

impl SvelteTemplateRule for NoDupeElseIfBlocks {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut reports = Vec::new();
        check_nodes(nodes, &[], &mut reports);
        for span in reports {
            ctx.diagnostic(no_dupe_else_if_blocks_diagnostic(span));
        }
    }
}

/// Walk the tree, checking each `{#if}` chain. `inherited` carries the
/// conditions a nested chain inherits from its enclosing chain(s), nearest
/// first: eslint-plugin-svelte's `iterateIfElseIf` walks through
/// `SvelteElseBlock` parents, so an `{#if}` written as a *direct* child of a
/// bare `{:else}` continues the enclosing chain (it only evaluates once every
/// earlier condition was false), while any deeper nesting — inside an
/// element, another block kind, or a non-else branch — starts a fresh chain.
fn check_nodes<'a>(nodes: &[Node<'a>], inherited: &[ExpressionSlot<'a>], reports: &mut Vec<Span>) {
    for node in nodes {
        match node {
            Node::Element(element) => check_nodes(&element.children, &[], reports),
            Node::Block(block) => match &block.kind {
                BlockKind::If(if_block) => check_if_chain(if_block, inherited, reports),
                BlockKind::Each(each) => {
                    check_nodes(&each.children, &[], reports);
                    if let Some(fallback) = &each.fallback {
                        check_nodes(fallback, &[], reports);
                    }
                }
                BlockKind::Await(await_block) => {
                    check_nodes(&await_block.pending, &[], reports);
                    if let Some(children) = &await_block.then_children {
                        check_nodes(children, &[], reports);
                    }
                    if let Some(children) = &await_block.catch_children {
                        check_nodes(children, &[], reports);
                    }
                }
                BlockKind::Key(key) => check_nodes(&key.children, &[], reports),
                BlockKind::Snippet(snippet) => check_nodes(&snippet.children, &[], reports),
                BlockKind::Unknown(unknown) => check_nodes(&unknown.children, &[], reports),
            },
            _ => {}
        }
    }
}

/// The earlier conditions branch `index` of `if_block` is checked against, in
/// the order upstream's `iterateIfElseIf` visits them: this chain's preceding
/// branches nearest-first, then the enclosing chains' inherited conditions.
fn earlier_conditions<'a>(
    if_block: &IfBlock<'a>,
    index: usize,
    inherited: &[ExpressionSlot<'a>],
) -> Vec<ExpressionSlot<'a>> {
    if_block.branches[..index]
        .iter()
        .rev()
        .filter_map(|branch| branch.expression)
        .chain(inherited.iter().copied())
        .collect()
}

fn check_if_chain<'a>(
    if_block: &IfBlock<'a>,
    inherited: &[ExpressionSlot<'a>],
    reports: &mut Vec<Span>,
) {
    // Every branch with a condition is checked against all earlier
    // conditions in its chain (upstream visits each `SvelteIfBlock` — the
    // `{#if}` and each `{:else if}` — independently, so one chain can report
    // several branches).
    for (index, branch) in if_block.branches.iter().enumerate() {
        let Some(condition) = branch.expression else { continue };
        let earlier = earlier_conditions(if_block, index, inherited);
        if !earlier.is_empty() {
            check_branch(condition, &earlier, reports);
        }
    }

    // Recurse into branch children. Only the bare `{:else}` branch's direct
    // children stay in the chain (see `check_nodes`).
    for (index, branch) in if_block.branches.iter().enumerate() {
        if branch.expression.is_none() && branch.is_else {
            let else_inherited = earlier_conditions(if_block, index, inherited);
            check_nodes(&branch.children, &else_inherited, reports);
        } else {
            check_nodes(&branch.children, &[], reports);
        }
    }
}

/// eslint-plugin-svelte `no-dupe-else-if-blocks`'s `SvelteIfBlock` handler
/// (itself a port of eslint core's `no-dupe-else-if`; the same machinery as
/// this fork's `vue/no-dupe-v-else-if`): decompose the branch's condition
/// into "candidate conditions" to check (the whole condition, plus — when it
/// is itself a top-level `&&` — each of its own conjuncts individually),
/// then walk the earlier branches nearest-first, at each step removing any
/// `||`-operand of each candidate that an earlier branch's `||`-operand
/// already covers (its `&&`-conjuncts are a subset of the candidate's). A
/// candidate with no `||`-operands left is fully covered: report on its own
/// span and stop (matches upstream's single report per branch, and its
/// `return` on the first fully-covered candidate).
///
/// Conditions here are text slices, so each one is parsed with `oxc_parser`;
/// a branch whose condition does not parse is skipped (upstream never gets
/// this far on unparsable input).
fn check_branch<'a>(
    condition: ExpressionSlot<'a>,
    earlier: &[ExpressionSlot<'a>],
    reports: &mut Vec<Span>,
) {
    let (text, trimmed_span) = condition.trimmed();
    let allocator = Allocator::new();
    let Ok(test) = Parser::new(&allocator, text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
    else {
        return;
    };

    // Upstream order matters: `[...splitByAnd(test), test]` — each conjunct
    // individually, THEN the whole test — because the first candidate in
    // this list to become fully covered is the one reported, and a top-level
    // `&&`'s own conjunct can become covered independently of (and before)
    // the whole conjunction does. E.g. `{#if a}{:else if a && b}` must
    // report the 1-char `a` inside `a && b`, not the whole `a && b` span.
    let mut conditions_to_check: Vec<&Expression> = Vec::new();
    if let Expression::LogicalExpression(logical) = &test
        && logical.operator == LogicalOperator::And
    {
        conditions_to_check.extend(split_by_and(&test));
    }
    conditions_to_check.push(&test);

    // `(report_span, or_branches)`; `or_branches` shrinks as earlier
    // branches cover more of it. Parsed spans are relative to the trimmed
    // condition text, so reports are shifted by its file offset.
    let mut list_to_check: Vec<(Span, Vec<Vec<&Expression>>)> = conditions_to_check
        .into_iter()
        .map(|expr| (expr.span(), split_by_or(expr).into_iter().map(split_by_and).collect()))
        .collect();

    let base = trimmed_span.start;
    for previous in earlier {
        let (previous_text, _) = previous.trimmed();
        let previous_allocator = Allocator::new();
        let Ok(previous_test) = Parser::new(&previous_allocator, previous_text, SourceType::ts())
            .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
            .parse_expression()
        else {
            // An unparsable earlier condition can't cover anything: skip it
            // and keep walking the chain.
            continue;
        };

        let current_or_operands: Vec<Vec<&Expression>> =
            split_by_or(&previous_test).into_iter().map(split_by_and).collect();

        for (report_span, or_branches) in &mut list_to_check {
            or_branches.retain(|or_branch| {
                !current_or_operands.iter().any(|current| is_subset(current, or_branch))
            });
            if or_branches.is_empty() {
                reports.push(Span::new(base + report_span.start, base + report_span.end));
                return;
            }
        }
    }
}

fn split_by_or<'e, 'a>(expr: &'e Expression<'a>) -> Vec<&'e Expression<'a>> {
    split_by_logical_operator(expr, LogicalOperator::Or)
}

fn split_by_and<'e, 'a>(expr: &'e Expression<'a>) -> Vec<&'e Expression<'a>> {
    split_by_logical_operator(expr, LogicalOperator::And)
}

/// Upstream's `splitByLogicalOperator`. `ParenthesizedExpression` is also
/// unwrapped transparently here (on top of parsing with `preserve_parens:
/// false`, which already avoids most of them) since a paren can still sit
/// around one operand of a *different* logical expression, e.g.
/// `(a || b) && c`'s left operand.
fn split_by_logical_operator<'e, 'a>(
    expr: &'e Expression<'a>,
    operator: LogicalOperator,
) -> Vec<&'e Expression<'a>> {
    match expr {
        Expression::LogicalExpression(logical) if logical.operator == operator => [
            split_by_logical_operator(&logical.left, operator),
            split_by_logical_operator(&logical.right, operator),
        ]
        .concat(),
        Expression::ParenthesizedExpression(parenthesized) => {
            split_by_logical_operator(&parenthesized.expression, operator)
        }
        _ => vec![expr],
    }
}

/// Upstream's `isSubset`: every conjunct of `a` (an `&&`-operand list) has an
/// equal conjunct in `b`.
fn is_subset(a: &[&Expression], b: &[&Expression]) -> bool {
    a.iter().all(|conjunct_a| b.iter().any(|conjunct_b| expressions_equal(conjunct_a, conjunct_b)))
}

/// Upstream's `equal`: `||`/`&&` are treated as commutative; anything else
/// falls back to structural content equality (`ContentEq`, ignoring
/// spans/comments) — a closer match to upstream's token-stream comparison
/// than a text comparison would be, and it works across the two independent
/// `oxc_parser` allocations this rule compares (the current branch's
/// condition and each earlier branch's), since `ContentEq` compares by
/// value, not identity.
fn expressions_equal(a: &Expression, b: &Expression) -> bool {
    if let (Expression::LogicalExpression(left), Expression::LogicalExpression(right)) = (a, b)
        && matches!(left.operator, LogicalOperator::Or | LogicalOperator::And)
        && left.operator == right.operator
    {
        return (expressions_equal(&left.left, &right.left)
            && expressions_equal(&left.right, &right.right))
            || (expressions_equal(&left.left, &right.right)
                && expressions_equal(&left.right, &right.left));
    }
    a.content_eq(b)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDupeElseIfBlocks;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("{#if a}{:else if b}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "{#if a}{:else if b}{:else if c}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Unrelated conjunction: no coverage relationship either way.
            ("{#if a}{:else if c && b}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // `a && b` being false doesn't imply `a` is false.
            ("{#if a && b}{:else if a}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{#if a}{:else}<div />{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // A nested `{#if}` inside the *consequent* starts a fresh chain.
            ("{#if a}{#if a}<div />{/if}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Only a *direct* child of `{:else}` continues the chain;
            // wrapped in an element, upstream's chain walk stops.
            (
                "{#if a}{:else}<div>{#if a}<hr />{/if}</div>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each items as item}{#if item}<hr />{/if}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        let fail = vec![
            // Exact duplicate.
            ("{#if a}{:else if a}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Walks back through the whole chain, not just the immediately
            // preceding branch.
            (
                "{#if a}{:else if b}{:else if a}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `a` covers the `a` operand of `a || b`.
            ("{#if a || b}{:else if a}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Every `||`-operand already appeared earlier in the chain.
            (
                "{#if a}{:else if b}{:else if a || b}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `a && b` implies `a`: dead once `a` already failed. Reports
            // the `a` conjunct inside `a && b`, matching upstream.
            ("{#if a}{:else if a && b}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Commutative `&&`: order of conjuncts doesn't matter.
            ("{#if a}{:else if b && a}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Commutative `||`: order of operands doesn't matter.
            ("{#if a || b}{:else if b || a}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Parenthesization doesn't defeat the comparison.
            ("{#if (a)}{:else if a}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // An `{#if}` written as a direct child of a bare `{:else}`
            // continues the chain (upstream's `iterateIfElseIf` walks
            // through `SvelteElseBlock` parents).
            (
                "{#if a}{:else}{#if a}<hr />{/if}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside another block kind.
            (
                "{#each items as item}{#if item.a}{:else if item.a}{/if}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Multiline chain.
            (
                "{#if foo}\n\t<div />\n{:else if foo}\n\t<span />\n{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoDupeElseIfBlocks::NAME, NoDupeElseIfBlocks::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
