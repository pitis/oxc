use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use oxc_vue_parser::ast::{Element, Node};

use crate::{
    rule::Rule,
    utils::{
        directive_key_span, directive_modifiers_span, directive_value_missing, get_directive,
        has_directive,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn inside_v_for_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-memo' directive does not work inside 'v-for'.")
        .with_help("Move the `v-memo` to the `v-for`'d element itself, or remove it.")
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-memo' directives require no argument.")
        .with_help("Remove the argument, e.g. use `v-memo=\"[a, b]\"`.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-memo' directives require no modifier.")
        .with_help("Remove the modifier; `v-memo` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-memo' directives require that attribute value.")
        .with_help("Give `v-memo` a dependency array, e.g. `v-memo=\"[a, b]\"`.")
        .with_label(span)
}

fn expected_array_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-memo' directives require the attribute value to be an array.")
        .with_help("Wrap the dependencies in an array, e.g. `v-memo=\"[a, b]\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVMemo;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-memo` directives (Vue 3.2+) in Vue `<template>`
    /// blocks: no argument, no modifiers, a required array-valued
    /// dependency list, and it must not appear on a descendant of a
    /// `v-for`'d element (only on the `v-for`'d element itself, or outside
    /// any `v-for`).
    ///
    /// ### Why is this bad?
    ///
    /// `v-memo` accepts none of these variations; a non-array value defeats
    /// its dependency comparison, and placing it on a descendant of a
    /// `v-for`'d element does not memoize anything meaningful.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-memo />
    ///   <div v-memo:foo="[a]" />
    ///   <div v-memo="a" />
    ///   <div v-for="item in items"><span v-memo="[item.id]" /></div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-memo="[a, b]" />
    ///   <div v-for="item in items" v-memo="[item.id]" />
    /// </template>
    /// ```
    ValidVMemo,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-memo` directives.",
);

impl Rule for ValidVMemo {}

impl VueTemplateRule for ValidVMemo {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk(nodes, None, ctx);
    }
}

/// eslint-plugin-vue `valid-v-memo`'s `VElement`/`VElement:exit` pair: track
/// the nearest v-for'd ancestor-or-self, set only on first encounter (a
/// nested `v-for` inside another `v-for`'d subtree does not update it) and
/// implicitly cleared on return from that element's subtree.
fn walk<'a, 'e>(
    nodes: &'e [Node<'a>],
    v_for_ancestor: Option<&'e Element<'a>>,
    ctx: &mut VueTemplateContext<'a>,
) {
    for node in nodes {
        let Node::Element(element) = node else { continue };

        let effective_ancestor = match v_for_ancestor {
            Some(ancestor) => Some(ancestor),
            None if has_directive(element, "for", None) => Some(element),
            None => None,
        };

        if let Some(attribute) = get_directive(element, "memo", None) {
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            if let Some(v_for_element) = effective_ancestor
                && !std::ptr::eq(v_for_element, element)
            {
                ctx.diagnostic(inside_v_for_diagnostic(directive_key_span(attribute)));
            }
            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            if !directive.modifiers.is_empty() {
                ctx.diagnostic(unexpected_modifier_diagnostic(directive_modifiers_span(
                    attribute,
                    ctx.source_text(),
                )));
            }
            if directive_value_missing(attribute) {
                ctx.diagnostic(expected_value_diagnostic(attribute.span));
            } else if let Some(value) = &attribute.value {
                check_array_value(value.text, value.span.start, ctx);
            }
        }

        walk(&element.children, effective_ancestor, ctx);
    }
}

/// eslint-plugin-vue's expression-type walk from `valid-v-memo`: reports
/// when the value's expression can plainly never be an array (an object,
/// class, function, literal, template literal, unary/binary/update
/// expression), and looks through the few wrapper expressions upstream
/// unwraps (assignment's RHS, a TS `as` cast, a sequence's last expression,
/// both branches of a conditional) plus, since oxc's AST — unlike the
/// ESTree shape eslint-plugin-vue walks — represents parentheses as their
/// own node, parenthesized expressions.
///
/// Deviation: upstream works off vue-eslint-parser's already-parsed
/// `node.value.expression` and silently skips this check when parsing
/// failed (`!node.value.expression`). This parser doesn't parse directive
/// values, so it parses `text` itself with `oxc_parser` (tolerant of
/// TypeScript syntax, since a `<script setup lang="ts">` SFC's template
/// expressions may use `as`); a parse failure is likewise treated as
/// "nothing to check" rather than an error of this rule's own.
fn check_array_value<'a>(text: &'a str, value_start: u32, ctx: &mut VueTemplateContext<'a>) {
    let allocator = Allocator::new();
    let Ok(expression) = Parser::new(&allocator, text, SourceType::ts()).parse_expression() else {
        return;
    };

    let mut stack = vec![expression];
    while let Some(expression) = stack.pop() {
        match expression {
            Expression::ObjectExpression(_)
            | Expression::ClassExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::TemplateLiteral(_)
            | Expression::UnaryExpression(_)
            | Expression::BinaryExpression(_)
            | Expression::UpdateExpression(_) => {
                let span = expression.span();
                ctx.diagnostic(expected_array_diagnostic(Span::new(
                    value_start + span.start,
                    value_start + span.end,
                )));
            }
            Expression::AssignmentExpression(assignment) => stack.push(assignment.unbox().right),
            Expression::TSAsExpression(as_expression) => {
                stack.push(as_expression.unbox().expression);
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                stack.push(parenthesized.unbox().expression);
            }
            Expression::SequenceExpression(mut sequence) => {
                if let Some(last) = sequence.expressions.pop() {
                    stack.push(last);
                }
            }
            Expression::ConditionalExpression(conditional) => {
                let conditional = conditional.unbox();
                stack.push(conditional.consequent);
                stack.push(conditional.alternate);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVMemo;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-memo="[a, b]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-memo="[]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // On the v-for'd element itself.
            (
                r#"<template><div v-for="item in items" v-memo="[item.id]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Outside any v-for.
            (
                r#"<template><div v-for="item in items" /><span v-memo="[a]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Ternary resolving to arrays on both sides.
            (
                r#"<template><div v-memo="cond ? [a] : [b]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A bare identifier isn't flagged: it might hold an array at
            // runtime and upstream's type check can't rule that out either.
            (
                r#"<template><div v-memo="deps" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // No value.
            (r"<template><div v-memo /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Argument.
            (
                r#"<template><div v-memo:foo="[a]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-memo.foo="[a]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Not an array.
            (
                r#"<template><div v-memo="1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-memo="{ a }" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-memo="a + b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // On a descendant of a v-for'd element.
            (
                r#"<template><div v-for="item in items"><span v-memo="[item.id]" /></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVMemo::NAME, ValidVMemo::PLUGIN, pass, fail).test_and_snapshot();
    }
}
