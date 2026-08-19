use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{ContentEq, GetSpan, SourceType, Span};
use oxc_syntax::operator::LogicalOperator;
use vue_sfc_parser::ast::{Attribute, Element, Node};

use crate::{
    rule::Rule,
    utils::{get_directive, walk_elements_with_siblings},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn no_dupe_v_else_if_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "This branch can never execute. Its condition is a duplicate or covered by previous conditions in the `v-if` / `v-else-if` chain.",
    )
    .with_help("Remove the duplicate branch, or change its condition so it covers a distinct case.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDupeVElseIf;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplicate conditions in `v-if` / `v-else-if` chains in Vue
    /// `<template>` blocks: a later branch whose condition is identical to,
    /// or logically covered by (in the "each `||`-operand's `&&`-operands
    /// form a subset" sense), an earlier branch's condition can never run.
    ///
    /// ### Why is this bad?
    ///
    /// A `v-else-if` branch only runs once every earlier branch's condition
    /// evaluated to `false`. If its own condition is implied by one of
    /// those (a duplicate, or a superset of an earlier `&&` clause, or an
    /// `||` operand that already appeared earlier), the branch can never
    /// execute — almost always a copy-paste mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="a" />
    ///   <div v-else-if="a" />
    ///   <div v-if="a" />
    ///   <div v-else-if="a && b" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="a" />
    ///   <div v-else-if="b" />
    /// </template>
    /// ```
    NoDupeVElseIf,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow duplicate conditions in `v-if` / `v-else-if` chains.",
);

impl Rule for NoDupeVElseIf {}

impl VueTemplateRule for NoDupeVElseIf {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements_with_siblings(nodes, &mut |element, siblings, index| {
            if let Some(attribute) = get_directive(element, "else-if", None) {
                check_else_if(attribute, siblings, index, ctx);
            }
        });
    }
}

/// eslint-plugin-vue `no-dupe-v-else-if`'s `VAttribute` handler: parse the
/// `v-else-if`'s own condition (silently doing nothing if it has no value or
/// fails to parse — mirroring `if (!node.value || !node.value.expression)
/// return;`), decompose it into "candidate conditions" to check (the whole
/// condition, plus — when it is itself a top-level `&&` — each of its own
/// conjuncts individually, matching upstream's handling of e.g. `v-if="a"`
/// `v-else-if="a && b"` being dead because `a && b` implies `a`), then walk
/// backward through preceding `v-if`/`v-else-if` siblings, at each step
/// removing any `||`-operand of each candidate that a preceding branch's
/// `||`-operand already covers (its `&&`-conjuncts are a subset of the
/// candidate's). A candidate with no `||`-operands left is fully covered:
/// report on its own span and stop (matches upstream's single report per
/// `v-else-if`, and its `return` on the first fully-covered candidate).
fn check_else_if<'a>(
    attribute: &Attribute<'a>,
    siblings: &[Node<'a>],
    index: usize,
    ctx: &mut VueTemplateContext<'a>,
) {
    let Some(value) = &attribute.value else { return };
    let allocator = Allocator::new();
    let Ok(test) = Parser::new(&allocator, value.text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
    else {
        return;
    };

    // Upstream order matters: `[...splitByAnd(test), test]` — each conjunct
    // individually, THEN the whole test — because the first candidate in
    // this list to become fully covered (by a preceding branch, checked
    // below) is the one reported, and a top-level `&&`'s own conjunct can
    // become covered independently of (and before) the whole conjunction
    // does. Reversing this order changes *which* span gets reported for
    // e.g. `v-if="a"` `v-else-if="a && b"` (must report the 1-char `a`
    // inside `a && b`, not the whole `a && b` span) — verified against real
    // eslint-plugin-vue: see this rule's tests.
    let mut conditions_to_check: Vec<&Expression> = Vec::new();
    if let Expression::LogicalExpression(logical) = &test
        && logical.operator == LogicalOperator::And
    {
        conditions_to_check.extend(split_by_and(&test));
    }
    conditions_to_check.push(&test);

    // `(report_span, or_branches)`; `or_branches` shrinks as preceding
    // branches cover more of it.
    let mut list_to_check: Vec<(Span, Vec<Vec<&Expression>>)> = conditions_to_check
        .into_iter()
        .map(|expr| (expr.span(), split_by_or(expr).into_iter().map(split_by_and).collect()))
        .collect();

    let value_start = value.span.start;
    let mut cursor = index;
    while let Some((prev_index, prev_element)) = prev_element_sibling_with_index(siblings, cursor) {
        let v_if = get_directive(prev_element, "if", None);
        let Some(current_test_dir) = v_if.or_else(|| get_directive(prev_element, "else-if", None))
        else {
            // No `v-if`/`v-else-if` on the immediately preceding element:
            // the chain is broken, abandon the whole check (no report).
            break;
        };

        if let Some(prev_value) = &current_test_dir.value {
            let prev_allocator = Allocator::new();
            if let Ok(prev_test) = Parser::new(&prev_allocator, prev_value.text, SourceType::ts())
                .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
                .parse_expression()
            {
                let current_or_operands: Vec<Vec<&Expression>> =
                    split_by_or(&prev_test).into_iter().map(split_by_and).collect();

                for (report_span, or_branches) in &mut list_to_check {
                    or_branches.retain(|or_branch| {
                        !current_or_operands.iter().any(|current| is_subset(current, or_branch))
                    });
                    if or_branches.is_empty() {
                        ctx.diagnostic(no_dupe_v_else_if_diagnostic(Span::new(
                            value_start + report_span.start,
                            value_start + report_span.end,
                        )));
                        return;
                    }
                }
            }
        }

        if v_if.is_some() {
            break;
        }
        cursor = prev_index;
    }
}

/// The nearest preceding `Element` sibling of `nodes[index]`, along with its
/// own index in `nodes` (so the caller can keep walking backward from it) —
/// like `crate::utils::prev_element_sibling`, but also yielding the index.
/// Kept local (this rule is the only one that needs to keep walking past the
/// first match) rather than added to the shared helpers.
fn prev_element_sibling_with_index<'e, 'a>(
    nodes: &'e [Node<'a>],
    index: usize,
) -> Option<(usize, &'e Element<'a>)> {
    nodes[..index].iter().enumerate().rev().find_map(|(i, node)| {
        if let Node::Element(element) = node { Some((i, element)) } else { None }
    })
}

fn split_by_or<'e, 'a>(expr: &'e Expression<'a>) -> Vec<&'e Expression<'a>> {
    split_by_logical_operator(expr, LogicalOperator::Or)
}

fn split_by_and<'e, 'a>(expr: &'e Expression<'a>) -> Vec<&'e Expression<'a>> {
    split_by_logical_operator(expr, LogicalOperator::And)
}

/// eslint-plugin-vue's `splitByLogicalOperator`. `ParenthesizedExpression` is
/// also unwrapped transparently here (on top of parsing with
/// `preserve_parens: false`, which already avoids most of them) since a
/// paren can still sit around one operand of a *different* logical
/// expression, e.g. `(a || b) && c`'s left operand.
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

/// eslint-plugin-vue's `isSubset`: every conjunct of `a` (an `&&`-operand
/// list) has an equal conjunct in `b`.
fn is_subset(a: &[&Expression], b: &[&Expression]) -> bool {
    a.iter().all(|conjunct_a| b.iter().any(|conjunct_b| expressions_equal(conjunct_a, conjunct_b)))
}

/// eslint-plugin-vue's `equal`: `||`/`&&` are treated as commutative; anything
/// else falls back to structural content equality (`ContentEq`, ignoring
/// spans/comments) — a closer match to upstream's token-stream comparison
/// than a text comparison would be, and it works across the two independent
/// `oxc_parser` allocations this rule compares (the current `v-else-if`'s
/// condition and each preceding branch's), since `ContentEq` compares by
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

    use super::NoDupeVElseIf;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-if="a" /><div v-else-if="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Unrelated conjunction: no coverage relationship either way.
            (
                r#"<template><div v-if="a" /><div v-else-if="c && b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Chain broken by a plain element with neither `v-if` nor
            // `v-else-if`: upstream abandons the check entirely.
            (
                r#"<template><div v-if="a" /><div /><div v-else-if="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `v-else-if` with no value at all: nothing to compare.
            (
                r#"<template><div v-if="a" /><div v-else-if /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Exact duplicate.
            (
                r#"<template><div v-if="a" /><div v-else-if="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `a` covers the `a` operand of `a || b`.
            (
                r#"<template><div v-if="a || b" /><div v-else-if="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `a && b` implies `a`: dead once `a` already failed.
            (
                r#"<template><div v-if="a" /><div v-else-if="a && b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Commutative `&&`: order of conjuncts doesn't matter.
            (
                r#"<template><div v-if="a" /><div v-else-if="b && a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Commutative `||`: order of operands doesn't matter.
            (
                r#"<template><div v-if="a || b" /><div v-else-if="b || a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Parenthesization doesn't defeat the comparison.
            (
                r#"<template><div v-if="(a)" /><div v-else-if="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Walks back through the whole chain, not just the immediately
            // preceding branch.
            (
                r#"<template><div v-if="a" /><div v-else-if="b" /><div v-else-if="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoDupeVElseIf::NAME, NoDupeVElseIf::PLUGIN, pass, fail).test_and_snapshot();
    }
}
