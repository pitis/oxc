use std::borrow::Cow;

use cow_utils::CowUtils;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashSet;
use vue_sfc_parser::ast::{Attribute, Element, Node};

use crate::{
    rule::Rule,
    utils::{
        TemplateExpressionKind, has_directive, template_expression_parse_error, walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn parsing_error_diagnostic(code: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Parsing error: {code}."))
        .with_help("Fix the malformed markup in the template.")
        .with_label(span)
}

fn expression_parsing_error_diagnostic(message: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Parsing error: {message}."))
        .with_help("Fix the JavaScript expression in the template.")
        .with_label(span)
}

/// A single-byte span at `start`: eslint-plugin-vue's `no-parsing-error`
/// reports a bare point location (no end line/column) for every code this
/// rule implements. When there is a real character at that position, a
/// 1-byte span matches upstream's start exactly while still being visible
/// as a label (a true zero-width span renders without an underline).
fn point_span(start: u32) -> Span {
    Span::new(start, start.saturating_add(1))
}

#[derive(Debug, Default, Clone)]
pub struct NoParsingError;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows a subset of HTML parse errors in `<template>` blocks:
    /// a repeated attribute name on one element (`duplicate-attribute`), an
    /// attribute `=` with no value before the tag closes
    /// (`missing-attribute-value`), and a quoted attribute value whose
    /// closing quote is missing before the template ends (`eof-in-tag`).
    ///
    /// eslint-plugin-vue's `no-parsing-error` reports the full WHATWG HTML
    /// parse-error list (~35 codes) sourced from vue-eslint-parser's
    /// tokenizer. This parser is error-tolerant by design (malformed markup
    /// degrades to a best-effort tree instead of failing), and its recovery
    /// model does not preserve enough information to safely reproduce most
    /// of those codes without risking false positives (see the rule's doc
    /// comment source for the full list). Only the three codes above are
    /// implemented; they are derived from the parsed attribute list and raw
    /// source text with no ambiguity, matching eslint-plugin-vue's messages
    /// exactly. Upstream reports a bare point location for every code here
    /// (its JSON output has no end line/column at all) — `duplicate-attribute`
    /// and `missing-attribute-value` render as a single-byte span at that
    /// same start position (there's a real character to underline, and a
    /// zero-width span would be invisible in a label); `eof-in-tag`'s point
    /// is the true end of the template block's content, not the start of
    /// the unterminated value, so it renders as a genuine zero-width span
    /// there instead. This is a deliberate, conservative subset —
    /// under-reporting is preferred to false positives.
    ///
    /// In addition to those three HTML codes, this rule reports **JavaScript
    /// parse errors in template expressions** — an interpolation's contents
    /// (`{{ foo &&& }}`) and every directive value that holds JS (`v-if`,
    /// `:bind`, `@on`, `v-for`, `v-model`, `v-slot`'s pattern, custom
    /// directives, …). vue-eslint-parser parses those as part of building the
    /// template body and pushes any failure into `templateBody.errors`, which
    /// upstream's `no-parsing-error` then reports; `vue_sfc_parser` has no
    /// error channel, and every expression-inspecting rule here bails
    /// silently when a value doesn't parse, so without this the whole class
    /// of broken expressions would be invisible. The diagnostic is placed on
    /// the expression's own span (upstream points at the exact offset inside
    /// it).
    ///
    /// ### Why is this bad?
    ///
    /// These are genuine HTML parse errors: at this WHATWG-tokenizer level, a
    /// repeated *raw* attribute name means only the first occurrence is
    /// honored and later duplicates are dropped outright (this is a
    /// lower-level check than `vue/no-duplicate-attributes`, which instead
    /// reasons about Vue's bound-vs-plain attribute identity and reports that
    /// the *last* write wins); an attribute left without a value after `=` is
    /// almost always a typo; and an unterminated quoted value swallows the
    /// rest of the template as part of that attribute.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div foo="a" foo="b" />
    /// </template>
    /// ```
    ///
    /// ```vue
    /// <template>
    ///   <div foo=>bar</div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div foo="a" bar="b" />
    /// </template>
    /// ```
    NoParsingError,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow parsing errors in `<template>`.",
);

impl Rule for NoParsingError {}

impl VueTemplateRule for NoParsingError {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let source = ctx.source_text();
        walk_elements(nodes, &mut |element| {
            check_duplicate_attributes(element, ctx);
            for attribute in &element.attributes {
                check_missing_attribute_value(attribute, source, ctx);
                check_eof_in_tag(attribute, source, ctx);
            }
        });
        check_expressions(nodes, ctx);
    }
}

/// Parse every expression-bearing site in `nodes` and report the failures.
///
/// Recurses by hand rather than through [`walk_elements`] because it must
/// (a) see [`Node::Interpolation`]s, not just elements, and (b) stop at a
/// `v-pre` element: vue-eslint-parser turns expression handling *off* for a
/// `v-pre` element's own attributes and its whole subtree (its
/// `expressionEnabled` flag), so directives there stay plain attributes and
/// mustaches stay literal text — nothing inside is ever parsed as JS, and
/// nothing inside can produce a parse error.
fn check_expressions<'a>(nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
    for node in nodes {
        match node {
            Node::Interpolation(interpolation) => {
                // An empty mustache is explicitly allowed upstream
                // (`parseExpression(…, { allowEmpty: true })`), and an
                // unterminated one never becomes an expression container at
                // all — its text has swallowed the rest of the template.
                //
                // The `trim()` here and its absence in
                // `check_attribute_expression`'s `value.text.is_empty()` are
                // both deliberate, and they mirror two *different* upstream
                // skips. `allowEmpty` is checked after parsing, against the
                // parsed expression, so `{{   }}` is whitespace that produced
                // no expression and is allowed — hence `trim()`. A directive
                // value instead short-circuits *before* any parse on the
                // exact test `quoted && node.value === ""`, so `v-if=" "` is
                // whitespace that upstream really does hand to the parser and
                // really does report — hence no `trim()` there.
                if interpolation.unterminated || interpolation.expression.trim().is_empty() {
                    continue;
                }
                if let Some(message) = template_expression_parse_error(
                    interpolation.expression,
                    TemplateExpressionKind::Expression,
                ) {
                    ctx.diagnostic(expression_parsing_error_diagnostic(
                        &message,
                        interpolation.expression_span,
                    ));
                }
            }
            Node::Element(element) => {
                if has_directive(element, "pre", None) {
                    continue;
                }
                for attribute in &element.attributes {
                    check_attribute_expression(element, attribute, ctx);
                }
                check_expressions(&element.children, ctx);
            }
            _ => {}
        }
    }
}

fn check_attribute_expression<'a>(
    element: &Element<'a>,
    attribute: &Attribute<'a>,
    ctx: &mut VueTemplateContext<'a>,
) {
    let Some(kind) = attribute_expression_kind(element, attribute) else { return };
    let Some(value) = &attribute.value else { return };
    // A valueless directive (`v-else`, `v-once`, `:foo` same-name shorthand)
    // and a quoted *empty* value are both skipped outright upstream — no
    // parse is attempted, so neither can be a parsing error. An unterminated
    // quoted value is already reported as `eof-in-tag` above, and its text is
    // the rest of the template rather than an expression.
    if value.text.is_empty() || value.unterminated {
        return;
    }
    if let Some(message) = template_expression_parse_error(value.text, kind) {
        ctx.diagnostic(expression_parsing_error_diagnostic(&message, value.span));
    }
}

/// Which grammar `attribute`'s value is parsed with, or `None` when it holds
/// no JavaScript at all — mirroring vue-eslint-parser's
/// `getStandardDirectiveKind` plus its `needConvertToDirective` gate (which is
/// what makes the two deprecated *plain* attributes `slot-scope` and
/// `<template scope>` expression-bearing).
fn attribute_expression_kind(
    element: &Element<'_>,
    attribute: &Attribute<'_>,
) -> Option<TemplateExpressionKind> {
    let Some(directive) = &attribute.directive else {
        // Case-sensitive, like the two `no-deprecated-*-attribute` rules:
        // vue-eslint-parser's SFC `getTagName` never case-folds for this
        // bare-attribute-to-directive conversion.
        if attribute.name == "slot-scope"
            || (element.name == "template" && attribute.name == "scope")
        {
            return Some(TemplateExpressionKind::SlotScope);
        }
        return None;
    };
    Some(match directive.name {
        "for" => TemplateExpressionKind::For,
        // Upstream gates the statement-list grammar on there being an
        // argument: argument-less `v-on="{ click: fn }"` is a plain object
        // *expression*.
        "on" if directive.argument.is_some() => TemplateExpressionKind::OnStatements,
        "slot" => TemplateExpressionKind::SlotScope,
        _ => TemplateExpressionKind::Expression,
    })
}

/// WHATWG `duplicate-attribute`: two attributes with the same raw name
/// (ASCII case-insensitive — HTML attribute names are case-insensitive) on
/// one start tag. This is a tokenizer-level check on the raw written name
/// (`:foo` vs `:foo`, not the Vue-level bound-vs-plain identity that
/// `vue/no-duplicate-attributes` covers), so it needs nothing beyond the
/// parsed attribute list.
fn check_duplicate_attributes<'a>(element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
    let mut seen: FxHashSet<Cow<'a, str>> = FxHashSet::default();
    for attribute in &element.attributes {
        let lower = attribute.name.cow_to_ascii_lowercase();
        if seen.contains(&lower) {
            ctx.diagnostic(parsing_error_diagnostic(
                "duplicate-attribute",
                point_span(attribute.span.start),
            ));
        } else {
            seen.insert(lower);
        }
    }
}

/// WHATWG `missing-attribute-value`: an attribute's `=` (optionally
/// followed by whitespace) is immediately followed by the tag's closing
/// `>`. Scanned directly off the raw source rather than the parsed
/// `AttributeValue`: this parser treats `=` followed by whitespace then
/// non-`>` content (e.g. `foo= bar="a"`) as an empty unquoted value too,
/// but that is a different, unverified error class upstream — re-scanning
/// the source keeps this check exact instead of over-firing on that case.
fn check_missing_attribute_value<'a>(
    attribute: &Attribute<'a>,
    source: &str,
    ctx: &mut VueTemplateContext<'a>,
) {
    let bytes = source.as_bytes();
    let mut index = attribute.name_span.end as usize;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b'=') {
        return;
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) == Some(&b'>') {
        let pos = u32::try_from(index).unwrap_or(attribute.span.end);
        ctx.diagnostic(parsing_error_diagnostic("missing-attribute-value", point_span(pos)));
    }
}

/// WHATWG `eof-in-tag`: the template ends while still inside a start tag —
/// here, specifically, while scanning for a quoted attribute value's
/// closing quote. The parser recovers by treating everything up to the end
/// of the template block's content as the (unterminated) value, so a
/// quoted value spans exactly to the end of `source` without ever finding
/// its closing quote is an unambiguous signal, independent of any parser
/// change.
///
/// Upstream's reported point is the true EOF, *not* the start of the
/// unterminated value (confirmed empirically: for `<div foo="a` followed by
/// EOF two lines down, eslint reports the EOF's own line:col, not the
/// quote's) — with a newline inside the unterminated value those two
/// positions can land on different lines entirely, so this must scan to
/// `source.len()`, not report `value.span`.
fn check_eof_in_tag<'a>(attribute: &Attribute<'a>, source: &str, ctx: &mut VueTemplateContext<'a>) {
    let Some(value) = &attribute.value else { return };
    if value.quote != b'"' && value.quote != b'\'' {
        return;
    }
    let bytes = source.as_bytes();
    let end = value.span.end as usize;
    if end >= bytes.len() {
        // Genuinely zero-width: `source.len()` is one past the last real
        // byte, so there is no character left to underline (unlike
        // `duplicate-attribute`/`missing-attribute-value`, which use
        // `point_span` for exactly that reason).
        let pos = u32::try_from(bytes.len()).unwrap_or(attribute.span.end);
        ctx.diagnostic(parsing_error_diagnostic("eof-in-tag", Span::new(pos, pos)));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoParsingError;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div foo="a" bar="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Different raw names, one plain one bound: not a duplicate at
            // this (HTML-tokenizer) level.
            (
                r#"<template><div foo="a" :foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A quoted empty value is valid, not a missing value.
            (r#"<template><div foo="" /></template>"#, None, None, Some(PathBuf::from("test.vue"))),
            // A boolean attribute (no `=` at all) is valid.
            (r"<template><div disabled /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // A normal unquoted value is valid.
            (r"<template><div foo=bar /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Properly closed quoted values, even at the very end of the
            // template block.
            (
                r#"<template><div foo="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Template expressions that parse cleanly.
            (
                r#"<template><div v-if="x === 1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A cross-section of real-world template expressions, guarding
            // against false positives from the wrapper snippets: object and
            // array literals (which need the parenthesised wrapper to not be
            // read as a block), optional chaining / nullish coalescing, a TS
            // cast, an inline arrow handler with a block body, a Vue 2 filter
            // pipe (valid JS as a bitwise `|`, so it must stay silent here),
            // and a three-alias `v-for` over an object.
            (
                r#"<template><div :class="{ active: isActive, 'text-danger': hasError }" :style="[base, override]" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model="form.name" :disabled="!!error" @input="$emit('update:modelValue', $event.target.value)" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-if="(a as string) === b">{{ obj?.deep ?? 'fallback' }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><button v-bind="$attrs" @keyup.enter="() => { submit() }">{{ items.map((i) => i.name).join(', ') }}</button></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><span v-for="(value, key, index) in object" :key="key">{{ value | capitalize }}{{ index }}</span></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template>{{ foo && bar }}</template>", None, None, Some(PathBuf::from("test.vue"))),
            (
                r#"<template><li v-for="(item, i) of items" :key="item.id">{{ item.name }}</li></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-on` with an argument is a statement *list*, not an
            // expression: two statements in a row must not be an error.
            (
                r#"<template><button @click="count++; save()" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument-less `v-on` is a plain object expression.
            (
                r#"<template><div v-on="{ click: onClick }" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-slot`'s value is a destructuring pattern, not an expression.
            (
                r#"<template><Comp #default="{ msg, list: [first] }">{{ msg }}{{ first }}</Comp></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template scope="props">{{ props }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A quoted empty value is never parsed upstream, and an empty
            // mustache is explicitly allowed.
            (
                r#"<template><div v-if="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template>{{ }}</template>", None, None, Some(PathBuf::from("test.vue"))),
            // Valueless directives have nothing to parse.
            (
                r#"<template><div v-if="a" /><div v-else /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-pre` turns expression handling off for the element and its
            // whole subtree.
            (
                r#"<template><div v-pre :foo="x ===">{{ foo &&& }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // An `eslint-disable` HTML comment suppresses the expression
            // diagnostic like any other.
            (
                "<template>\n<!-- eslint-disable vue/no-parsing-error -->\n<div v-if=\"x ===\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><div foo="a" foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Case-insensitive duplicate.
            (
                r#"<template><div foo="a" FOO="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Duplicate directive raw name.
            (
                r#"<template><div :foo="a" :foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div foo=>bar</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Unterminated quoted value: never finds its closing quote.
            ("<template><div foo=\"a</template>", None, None, Some(PathBuf::from("test.vue"))),
            // Same, but with newlines inside the unterminated value: locks
            // in that the reported point is the true EOF (a later line),
            // not the opening quote's line.
            (
                "<template><div foo=\"a\nb\nc</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Broken JavaScript in a directive value.
            (
                r#"<template><div v-if="x ===" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // …and in an interpolation.
            (r"<template>{{ foo &&& }}</template>", None, None, Some(PathBuf::from("test.vue"))),
            // `v-for` with no iterator after `in`.
            (
                r#"<template><li v-for="item in" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-for` with no alias list at all: upstream's `ALIAS_ITERATOR`
            // doesn't match and it reports the missing alias.
            (
                r#"<template><li v-for="items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `v-on` handler that isn't valid as statements either.
            (
                r#"<template><button @click="foo(" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A broken `v-slot` destructuring pattern.
            (
                r#"<template><Comp #default="{ msg" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // The `<template>` is not the first block: locks in that the
            // reported position is file-relative (the template AST's spans
            // already carry the block's offset), not template-relative.
            (
                "<script setup>\nconst x = 1;\n</script>\n\n<template>\n  <div v-if=\"x ===\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoParsingError::NAME, NoParsingError::PLUGIN, pass, fail).test_and_snapshot();
    }
}
