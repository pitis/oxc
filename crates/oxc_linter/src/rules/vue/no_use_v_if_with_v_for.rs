use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, ForStatementLeft, Statement};
use oxc_ast_visit::Visit;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use oxc_vue_parser::ast::Node;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{get_directive, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn moved_to_wrapper_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("This 'v-if' should be moved to the wrapper element.")
        .with_help("Wrap the `v-for`'d element in a parent and move `v-if` there, or use a computed property to pre-filter the list.")
        .with_label(span)
}

fn should_use_computed_diagnostic(iterator_name: &str, kind: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The '{iterator_name}' {kind} inside 'v-for' directive should be replaced with a computed property that returns filtered array instead. You should not mix 'v-for' with 'v-if'."
    ))
    .with_help("Move the filtering into a computed property and `v-for` over that instead.")
    .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoUseVIfWithVFor {
    /// Whether a `v-if` that references one of the `v-for`'s own iteration
    /// variables (e.g. `v-for="item in items" v-if="item.active"`) is
    /// allowed without a report (still reported, with a different message,
    /// when this is `false`, the default). Default `false`.
    allow_using_iteration_var: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows using `v-if` on the same element as `v-for` in Vue
    /// `<template>` blocks.
    ///
    /// ### Why is this bad?
    ///
    /// `v-for` has higher precedence than `v-if` when both sit on the same
    /// element, so the condition re-runs for every item and, when it
    /// references the loop's own iteration variable, that variable is
    /// evaluated once per item just to filter — better expressed as a
    /// computed property. Worse, when the condition does *not* reference
    /// the iteration variable, the whole list is needlessly re-evaluated on
    /// every re-render; that case should move `v-if` to a wrapper element
    /// instead.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items" v-if="shouldShow" />
    ///   <div v-for="item in items" v-if="item.active" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <template v-if="shouldShow">
    ///     <div v-for="item in items" />
    ///   </template>
    ///   <div v-for="item in activeItems" />
    /// </template>
    /// ```
    NoUseVIfWithVFor,
    vue,
    correctness,
    config = NoUseVIfWithVFor,
    version = "1.77.0",
    short_description = "Disallow using `v-if` on the same element as `v-for`.",
);

impl Rule for NoUseVIfWithVFor {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoUseVIfWithVFor {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(v_if) = get_directive(element, "if", None) else { return };
            let Some(v_for) = get_directive(element, "for", None) else { return };

            let for_aliases =
                v_for.value.as_ref().map(|value| for_alias_names(value.text)).unwrap_or_default();

            let references = v_if
                .value
                .as_ref()
                .map(|value| expression_reference_names(value.text))
                .unwrap_or_default();
            let is_using_iteration_var =
                !references.is_empty() && references.iter().any(|name| for_aliases.contains(name));

            if !is_using_iteration_var {
                ctx.diagnostic(moved_to_wrapper_diagnostic(v_if.span));
                return;
            }

            if self.allow_using_iteration_var {
                return;
            }

            let Some(for_value) = &v_for.value else { return };
            let Some((iterator_name, kind)) = iterator_name_and_kind(for_value.text) else {
                return;
            };
            ctx.diagnostic(should_use_computed_diagnostic(&iterator_name, kind, v_if.span));
        });
    }
}

/// eslint-plugin-vue's `getVForUsingIterationVar`'s tail: once a `v-if` is
/// known to reference one of the `v-for`'s own aliases, resolve the message's
/// `iteratorName`/`kind` from the `v-for`'s right-hand side (the iterated
/// expression) — a bare identifier reports as `variable` (using its own
/// name), anything else reports as `expression` (using its raw source text).
/// Returns `None` only when the `v-for` value itself doesn't parse (should
/// not happen here, since `for_alias_names` already required it to parse to
/// have produced a non-empty alias set in the first place).
fn iterator_name_and_kind(for_value_text: &str) -> Option<(String, &'static str)> {
    let (_, sep_end) = find_for_separator(for_value_text)?;
    let iterator_raw = for_value_text[sep_end..].trim();
    if iterator_raw.is_empty() {
        return None;
    }

    let allocator = Allocator::new();
    if let Ok(Expression::Identifier(identifier)) =
        Parser::new(&allocator, iterator_raw, SourceType::ts()).parse_expression()
    {
        return Some((identifier.name.as_str().to_string(), "variable"));
    }
    Some((iterator_raw.to_string(), "expression"))
}

struct NameCollector {
    names: Vec<String>,
}

impl<'a> Visit<'a> for NameCollector {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.push(it.name.as_str().to_string());
    }

    fn visit_binding_identifier(&mut self, it: &oxc_ast::ast::BindingIdentifier<'a>) {
        self.names.push(it.name.as_str().to_string());
    }
}

/// eslint-plugin-vue's `isUsingIterationVar`'s reference extraction: every
/// identifier referenced anywhere in the expression. Mirrors
/// `no-v-for-template-key-on-child`'s `expression_reference_names` (kept
/// local per this fork's established convention of duplicating this small
/// helper per rule file rather than sharing it). Silently returns nothing on
/// any parse failure — matching this rule's other parse-failure handling
/// (`for_alias_names` below), which folds into `is_using_iteration_var` being
/// `false` and thus a `movedToWrapper` report, matching upstream's own
/// behavior when `vIf.value.references` would be empty.
fn expression_reference_names(text: &str) -> Vec<String> {
    let allocator = Allocator::new();
    let snippet = format!("({text});");
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let Some(Statement::ExpressionStatement(statement)) = parser_ret.program.body.first() else {
        return Vec::new();
    };
    let mut collector = NameCollector { names: Vec::new() };
    collector.visit_expression(&statement.expression);
    collector.names
}

/// Mirrors vue-eslint-parser's `ALIAS_ITERATOR` regex — the first (leftmost)
/// whole-word `in`/`of` immediately preceded by whitespace or `)`. Copied
/// from `valid-v-for`'s `find_for_separator` (kept local per this fork's
/// established convention — see `no-v-for-template-key-on-child`'s copy of
/// the same helper).
fn find_for_separator(text: &str) -> Option<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (index, &(byte_pos, _)) in chars.iter().enumerate() {
        let preceded_ok = index > 0 && {
            let previous = chars[index - 1].1;
            previous.is_whitespace() || previous == ')'
        };
        if !preceded_ok {
            continue;
        }
        for keyword in ["in", "of"] {
            if !text[byte_pos..].starts_with(keyword) {
                continue;
            }
            let after = byte_pos + keyword.len();
            let word_boundary_ok = match text[after..].chars().next() {
                None => true,
                Some(next) => !(next.is_alphanumeric() || next == '_' || next == '$'),
            };
            if word_boundary_ok {
                return Some((byte_pos, after));
            }
        }
    }
    None
}

/// The `v-for` alias *names* declared by a `v-for="<aliases> in/of <expr>"`
/// value, via the same parse-as-a-real-`for`-statement mechanism as
/// `valid-v-for`'s `check_for_value` / `no-v-for-template-key-on-child`'s
/// `for_alias_names` (kept local per this fork's established convention).
/// Silently returns nothing on any parse failure, matching `valid-v-for`'s
/// silent-on-parse-failure discipline.
fn for_alias_names(raw: &str) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let Some((sep_start, sep_end)) = find_for_separator(raw) else { return names };
    let aliases_raw = &raw[..sep_start];
    if aliases_raw.trim().is_empty() {
        return names;
    }
    let delimiter = &raw[sep_start..sep_end];
    let iterator_raw = &raw[sep_end..];

    let trimmed = aliases_raw.trim();
    let inner = if trimmed.len() >= 2 && trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        aliases_raw
    };

    let snippet = format!("for(let [{inner}]{delimiter}{iterator_raw});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return names;
    }
    let left = match parser_ret.program.body.first() {
        Some(Statement::ForInStatement(statement)) => &statement.left,
        Some(Statement::ForOfStatement(statement)) => &statement.left,
        _ => return names,
    };
    let ForStatementLeft::VariableDeclaration(declaration) = left else { return names };
    let Some(declarator) = declaration.declarations.first() else { return names };
    let BindingPattern::ArrayPattern(array_pattern) = &declarator.id else { return names };

    for pattern in array_pattern.elements.iter().flatten() {
        collect_binding_names(pattern, &mut names);
    }
    names
}

fn collect_binding_names(pattern: &BindingPattern<'_>, out: &mut FxHashSet<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            out.insert(ident.name.as_str().to_string());
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_binding_names(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for pattern in array.elements.iter().flatten() {
                collect_binding_names(pattern, out);
            }
            if let Some(rest) = &array.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_binding_names(&assignment.left, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoUseVIfWithVFor;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // `v-if` on an ancestor, not the same element as `v-for`.
            (
                r#"<template><div v-if="show"><div v-for="item in items" /></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No `v-for` at all.
            (
                r#"<template><div v-if="show" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // References the iteration variable, and the option allows it.
            (
                r#"<template><div v-for="item in items" v-if="item.active" /></template>"#,
                Some(json!([{ "allowUsingIterationVar": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Condition unrelated to the iteration variable: moved-to-wrapper.
            (
                r#"<template><div v-for="item in items" v-if="show" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Condition references the iteration variable directly: computed.
            (
                r#"<template><div v-for="item in items" v-if="item.active" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // References it via a logical expression.
            (
                r#"<template><div v-for="item in items" v-if="item.active && other" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // References the second (index) alias slot.
            (
                r#"<template><div v-for="(item, index) in items" v-if="index % 2 === 0" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `allowUsingIterationVar` only silences the "references it"
            // case, not the unrelated-condition case.
            (
                r#"<template><div v-for="item in items" v-if="show" /></template>"#,
                Some(json!([{ "allowUsingIterationVar": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoUseVIfWithVFor::NAME, NoUseVIfWithVFor::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
