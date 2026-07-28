use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_syntax::identifier::{is_identifier_part, is_identifier_start};
use oxc_vue_parser::ast::{AttributeValue, Element, Node};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        directive_key_span, directive_value_missing, get_directive, is_custom_component,
        start_tag_span, walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Custom elements in iteration require 'v-bind:key' directives.")
        .with_help("Add a `:key` binding so Vue can track each node's identity.")
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require no argument.")
        .with_help("Remove the argument, e.g. use `v-for=\"item in items\"`.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require no modifier.")
        .with_help("Remove the modifier; `v-for` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require that attribute value.")
        .with_help("Give `v-for` an iteration expression, e.g. `v-for=\"item in items\"`.")
        .with_label(span)
}

fn unexpected_expression_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require the special syntax '<alias> in <expression>'.")
        .with_help("Use the form `item in items` or `(item, index) in items`.")
        .with_label(span)
}

fn invalid_empty_alias_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Invalid empty alias.")
        .with_help("Give the alias a name, or enable the `allowEmptyAlias` option.")
        .with_label(span)
}

fn invalid_alias_diagnostic(text: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Invalid alias '{text}'."))
        .with_help("Key and index aliases must be plain identifiers.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ValidVFor {
    /// Whether an omitted alias slot (e.g. `(, key) in items`) is allowed.
    /// Default `false`.
    allow_empty_alias: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-for` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, a required value in the `<alias> in
    /// <expression>` / `<alias> of <expression>` form, valid `key`/`index`
    /// aliases, and a `:key` binding on custom elements that appear directly
    /// inside the iteration.
    ///
    /// Native (non-component) elements missing `:key` are not this rule's
    /// concern — that is `vue/require-v-for-key`'s job; this rule only
    /// requires it for custom elements, matching eslint-plugin-vue's split
    /// of the same responsibility across `valid-v-for` and
    /// `require-v-for-key`.
    ///
    /// ### Why is this bad?
    ///
    /// `v-for` accepts none of these variations; using them produces a
    /// template that either fails to compile or iterates incorrectly. An
    /// un-keyed custom component in a loop loses per-item identity across
    /// re-renders.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items" v-bind:key.foo="item.id" />
    ///   <div v-for:foo="item in items" />
    ///   <div v-for="items" />
    ///   <div v-for="(item, 1) in items" />
    ///   <MyRow v-for="item in items" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items" />
    ///   <div v-for="(item, index) in items" />
    ///   <div v-for="(value, key, index) in object" />
    ///   <MyRow v-for="item in items" :key="item.id" />
    /// </template>
    /// ```
    ValidVFor,
    vue,
    correctness,
    config = ValidVFor,
    version = "1.77.0",
    short_description = "Enforce valid `v-for` directives.",
);

impl Rule for ValidVFor {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for ValidVFor {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "for", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            check_key(element, ctx);

            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            if !directive.modifiers.is_empty() {
                ctx.diagnostic(unexpected_modifier_diagnostic(directive_key_span(attribute)));
            }
            if directive_value_missing(attribute) {
                ctx.diagnostic(expected_value_diagnostic(attribute.span));
                return;
            }

            let value = attribute.value.as_ref().expect("checked by directive_value_missing");
            check_for_value(value, self.allow_empty_alias, ctx);
        });
    }
}

/// eslint-plugin-vue `valid-v-for`'s `checkKey`/`checkChildKey`, minus the
/// `isUsingIterationVar` cross-check (see [`check_for_value`]'s doc comment
/// for why): a keyed element, or one whose `:key` sits on it, is fine;
/// `<template>` (not `<slot>` — unlike `require-v-for-key`, upstream's
/// `valid-v-for` does not special-case `<slot>`) pushes the requirement down
/// to its children; a keyless custom element is reported. Native elements
/// are require-v-for-key's concern, not this rule's.
fn check_key<'a>(element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
    if get_directive(element, "bind", Some("key")).is_some() {
        return;
    }
    if element.name == "template" {
        for child in &element.children {
            if let Node::Element(child_element) = child {
                check_key(child_element, ctx);
            }
        }
        return;
    }
    if is_custom_component(element) {
        ctx.diagnostic(require_key_diagnostic(start_tag_span(element, ctx.source_text())));
    }
}

/// eslint-plugin-vue's `create`'s `VAttribute[...]` handler body from the
/// `expr.type !== "VForExpression"` check onward.
///
/// Deviation: upstream parses `value` as a JS expression and works off the
/// resulting `VForExpression` AST node (`expr.left` gives `[value, key,
/// index]` patterns directly, and a value that fails to parse at all is
/// silently ignored — `expr == null` returns early with no report). This
/// parser doesn't parse directive values as JS, so this reimplements the
/// grammar textually: split on the first top-level (bracket/quote-depth 0)
/// ` in `/` of `, optionally strip one layer of wrapping parens from the
/// alias list, then split that on top-level commas. `key`/`index` aliases
/// are validated as plain identifiers via the same identifier-character
/// tables oxc's own parser uses. The one case this can't reproduce is
/// upstream's "silently ignore a value that fails to parse entirely" — a
/// value with no top-level ` in `/` of ` at all (e.g. `v-for="items"`,
/// `v-for="1 +"`) always reports `unexpectedExpression` here, whereas
/// upstream only reports it when the value parses to a non-`VForExpression`
/// AST and stays silent on a genuine syntax error. The `isUsingIterationVar`
/// check (that a `:key` binding actually references one of the `v-for`
/// aliases) is skipped entirely — it requires resolving identifier
/// references against declared pattern names, which needs real scope
/// analysis this parser doesn't have.
fn check_for_value<'a>(
    value: &AttributeValue<'a>,
    allow_empty_alias: bool,
    ctx: &mut VueTemplateContext<'a>,
) {
    let raw = value.text;
    let Some((sep_start, sep_end)) = find_for_separator(raw) else {
        ctx.diagnostic(unexpected_expression_diagnostic(value.span));
        return;
    };
    if raw[sep_end..].trim().is_empty() {
        ctx.diagnostic(unexpected_expression_diagnostic(value.span));
        return;
    }

    let (aliases_trimmed, aliases_start) = trim_with_offset(&raw[..sep_start]);
    let (parts, has_parens): (Vec<(usize, &str)>, bool) = if aliases_trimmed.len() >= 2
        && aliases_trimmed.starts_with('(')
        && aliases_trimmed.ends_with(')')
    {
        let inner = &aliases_trimmed[1..aliases_trimmed.len() - 1];
        let inner_start = aliases_start + 1;
        (
            split_top_level_commas(inner)
                .into_iter()
                .map(|(offset, text)| (inner_start + offset, text))
                .collect(),
            true,
        )
    } else {
        (vec![(aliases_start, aliases_trimmed)], false)
    };

    let value_missing = parts.first().is_none_or(|(_, text)| text.trim().is_empty());
    if value_missing && !allow_empty_alias {
        ctx.diagnostic(invalid_empty_alias_diagnostic(value.span));
    }

    if has_parens {
        if let Some(&(offset, text)) = parts.get(1) {
            check_key_or_index_alias(text, offset, value, allow_empty_alias, ctx);
        }
        if let Some(&(offset, text)) = parts.get(2) {
            check_key_or_index_alias(text, offset, value, allow_empty_alias, ctx);
        }
    }
}

/// Validates a `key`/`index` alias slot: eslint-plugin-vue's `isValidAlias`
/// (present and an `Identifier`, or empty when `allowEmptyAlias`).
fn check_key_or_index_alias<'a>(
    text: &str,
    offset_in_value: usize,
    value: &AttributeValue<'a>,
    allow_empty_alias: bool,
    ctx: &mut VueTemplateContext<'a>,
) {
    let (trimmed, trimmed_offset) = trim_with_offset(text);
    if trimmed.is_empty() {
        if !allow_empty_alias {
            ctx.diagnostic(invalid_empty_alias_diagnostic(value.span));
        }
        return;
    }
    if !is_valid_identifier_name(trimmed) {
        let start = value.span.start + u32::try_from(offset_in_value + trimmed_offset).unwrap_or(0);
        let end = start + u32::try_from(trimmed.len()).unwrap_or(0);
        ctx.diagnostic(invalid_alias_diagnostic(trimmed, Span::new(start, end)));
    }
}

/// Whether `text` is a single valid JS identifier (what `isValidAlias`
/// requires of `key`/`index`).
fn is_valid_identifier_name(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return false };
    is_identifier_start(first) && chars.all(is_identifier_part)
}

/// The byte offset within `raw` where its trimmed content starts, alongside
/// the trimmed text itself.
fn trim_with_offset(raw: &str) -> (&str, usize) {
    let start = raw.len() - raw.trim_start().len();
    (raw.trim(), start)
}

/// The first top-level (bracket/quote-depth 0) ` in ` or ` of ` in `text`,
/// as `(separator_start, separator_end)` byte offsets.
fn find_for_separator(text: &str) -> Option<(usize, usize)> {
    let mut depth: i32 = 0;
    let mut quote: Option<char> = None;
    for (byte_pos, ch) in text.char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && quote.is_none() {
            for sep in [" in ", " of "] {
                if text[byte_pos..].starts_with(sep) {
                    return Some((byte_pos, byte_pos + sep.len()));
                }
            }
        }
    }
    None
}

/// Splits `text` on top-level (bracket/quote-depth 0) commas, pairing each
/// piece with its byte offset within `text`.
fn split_top_level_commas(text: &str) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let mut depth: i32 = 0;
    let mut quote: Option<char> = None;
    let mut start = 0usize;
    for (byte_pos, ch) in text.char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push((start, &text[start..byte_pos]));
                start = byte_pos + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push((start, &text[start..]));
    parts
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::ValidVFor;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item of items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="(item, index) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="(value, key, index) in object" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Destructured value alias, own shape unchecked.
            (
                r#"<template><div v-for="{ a, b } in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component with a key.
            (
                r#"<template><MyRow v-for="item in items" :key="item.id" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Native element without a key: not this rule's concern.
            (
                r#"<template><div v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Key pushed down through <template>.
            (
                r#"<template><template v-for="item in items"><MyRow :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty key alias allowed via option.
            (
                r#"<template><div v-for="(value, , index) in items" /></template>"#,
                Some(json!([{ "allowEmptyAlias": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Argument.
            (
                r#"<template><div v-for:foo="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-for.foo="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No value.
            (r"<template><div v-for /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div v-for="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Not the special syntax.
            (
                r#"<template><div v-for="items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Invalid key alias (not an identifier).
            (
                r#"<template><div v-for="(item, 1) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Invalid index alias.
            (
                r#"<template><div v-for="(item, key, foo.bar) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty key alias, option off.
            (
                r#"<template><div v-for="(value, , index) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component missing a key.
            (
                r#"<template><MyRow v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component missing a key, pushed down through <template>.
            (
                r#"<template><template v-for="item in items"><MyRow /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVFor::NAME, ValidVFor::PLUGIN, pass, fail).test_and_snapshot();
    }
}
