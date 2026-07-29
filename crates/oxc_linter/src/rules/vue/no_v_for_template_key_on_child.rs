use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, ForStatementLeft, Statement};
use oxc_ast_visit::Visit;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use oxc_vue_parser::ast::{Attribute, Element, Node};
use rustc_hash::FxHashSet;

use crate::{
    rule::Rule,
    utils::{element_name_eq_lower, get_directive, has_directive},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn v_for_template_key_placement_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`<template v-for>` key should be placed on the `<template>` tag.")
        .with_help("Move `:key` from the child element onto the `<template v-for>` tag itself.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoVForTemplateKeyOnChild;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// On a `<template v-for>`, disallows placing the loop's `:key` on a
    /// child element instead of on the `<template>` tag itself, when that
    /// child's `:key` references one of the `v-for`'s own iteration
    /// variables.
    ///
    /// ### Why is this bad?
    ///
    /// Vue 3's compiler specifically requires the key of a `<template
    /// v-for>` to sit on the `<template>` tag (`X_V_FOR_TEMPLATE_KEY_PLACEMENT`);
    /// a key placed on the child instead is not recognized as the loop's
    /// key.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <template v-for="item in items">
    ///     <div :key="item.id" />
    ///   </template>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <template v-for="item in items" :key="item.id">
    ///     <div />
    ///   </template>
    /// </template>
    /// ```
    NoVForTemplateKeyOnChild,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow key of `<template v-for>` placed on child elements.",
);

impl Rule for NoVForTemplateKeyOnChild {}

impl VueTemplateRule for NoVForTemplateKeyOnChild {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk(nodes, ctx);
    }
}

fn walk<'a>(nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
    for node in nodes {
        let Node::Element(element) = node else { continue };
        if !element_name_eq_lower(element, "template") {
            walk(&element.children, ctx);
            continue;
        }

        if let Some(v_for) = get_directive(element, "for", None) {
            check_template(element, v_for, ctx);
        }

        walk(&element.children, ctx);
    }
}

fn check_template<'a>(
    element: &Element<'a>,
    v_for: &Attribute<'a>,
    ctx: &mut VueTemplateContext<'a>,
) {
    let Some(for_value) = v_for.value.as_ref() else { return };
    let for_aliases = for_alias_names(for_value.text);
    if for_aliases.is_empty() {
        return;
    }

    let template_key = get_directive(element, "bind", Some("key"));
    if let Some(template_key) = template_key
        && key_uses_iteration_var(template_key, &for_aliases)
    {
        // The key is correctly placed on the `<template>` itself.
        return;
    }

    for child in &element.children {
        let Node::Element(child_element) = child else { continue };
        if has_directive(child_element, "if", None)
            || has_directive(child_element, "else-if", None)
            || has_directive(child_element, "else", None)
            || has_directive(child_element, "for", None)
        {
            continue;
        }
        let Some(child_key) = get_directive(child_element, "bind", Some("key")) else { continue };
        if key_uses_iteration_var(child_key, &for_aliases) {
            ctx.diagnostic(v_for_template_key_placement_diagnostic(child_key.span));
        }
    }
}

/// eslint-plugin-vue's `isUsingIterationVar`, approximated: does `key`'s
/// value expression reference any name declared by the `v-for`'s alias
/// pattern? Upstream resolves this via real scope analysis (which
/// identifiers a `v-for`'s scope actually declares and which references
/// resolve to them); this rule instead re-parses both sides with
/// `oxc_parser` and checks for a name in common, matching this fork's other
/// `valid-v-*`/`valid-v-slot` rules' documented tradeoff for the same class
/// of check. A coincidental name collision with an unrelated outer-scope
/// variable would be a false positive here, but is rare in practice.
fn key_uses_iteration_var(key: &Attribute<'_>, for_aliases: &FxHashSet<String>) -> bool {
    let Some(value) = &key.value else { return false };
    let referenced = expression_reference_names(value.text);
    referenced.iter().any(|name| for_aliases.contains(name))
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
/// established convention — see `valid-v-model`'s copy of the same helper).
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
/// `valid-v-for`'s `check_for_value` / `valid-v-model`'s `for_alias_names`
/// (kept local per this fork's established convention of duplicating this
/// small helper per rule file rather than sharing it). Silently returns
/// nothing on any parse failure, matching `valid-v-for`'s
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

    use super::NoVForTemplateKeyOnChild;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Key correctly placed on the template, referencing the loop var.
            (
                r#"<template><template v-for="item in items" :key="item.id"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Child key that doesn't reference the loop var at all.
            (
                r#"<template><template v-for="item in items"><div :key="unrelated" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Child with its own v-if is skipped regardless of its key.
            (
                r#"<template><template v-for="item in items"><div v-if="cond" :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items"><div v-else-if="cond" :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items"><div v-else :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items"><div v-for="x in item.xs" :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No v-for at all: not this rule's concern.
            (
                r#"<template><template><div :key="anything" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No key anywhere.
            (
                r#"<template><template v-for="item in items"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (
                r#"<template><Template v-for="x in y"><div :key="x" /></Template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No key on template; child's key references the loop var.
            (
                r#"<template><template v-for="item in items"><div :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Template's own key does NOT reference the loop var, so the
            // child is still checked.
            (
                r#"<template><template v-for="item in items" :key="unrelated"><div :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Second alias slot (index) used instead of the first.
            (
                r#"<template><template v-for="(item, idx) in items"><div :key="idx" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoVForTemplateKeyOnChild::NAME, NoVForTemplateKeyOnChild::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
