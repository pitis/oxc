use std::borrow::Cow;

use cow_utils::CowUtils;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::{Attribute, Element, Node};
use rustc_hash::FxHashSet;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn parsing_error_diagnostic(code: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Parsing error: {code}."))
        .with_help("Fix the malformed markup in the template.")
        .with_label(span)
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
    /// and reported spans exactly for those cases. This is a deliberate,
    /// conservative subset — under-reporting is preferred to false positives.
    ///
    /// ### Why is this bad?
    ///
    /// These are genuine HTML parse errors: a duplicate attribute means only
    /// the first occurrence is honored (Vue does not merge or reorder), an
    /// attribute left without a value after `=` is almost always a typo, and
    /// an unterminated quoted value swallows the rest of the template as
    /// part of that attribute.
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
    }
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
            ctx.diagnostic(parsing_error_diagnostic("duplicate-attribute", attribute.span));
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
        ctx.diagnostic(parsing_error_diagnostic(
            "missing-attribute-value",
            Span::new(pos, pos.saturating_add(1)),
        ));
    }
}

/// WHATWG `eof-in-tag`: the template ends while still inside a start tag —
/// here, specifically, while scanning for a quoted attribute value's
/// closing quote. The parser recovers by treating everything up to the end
/// of the template block's content as the (unterminated) value, so a
/// quoted value spans exactly to the end of `source` without ever finding
/// its closing quote is an unambiguous signal, independent of any parser
/// change.
fn check_eof_in_tag<'a>(attribute: &Attribute<'a>, source: &str, ctx: &mut VueTemplateContext<'a>) {
    let Some(value) = &attribute.value else { return };
    if value.quote != b'"' && value.quote != b'\'' {
        return;
    }
    let bytes = source.as_bytes();
    let end = value.span.end as usize;
    if end >= bytes.len() {
        // `value.span` already runs from the opening quote to end-of-input
        // (there was no closing quote to stop it) — highlight that whole
        // unterminated run rather than a zero-width point at EOF.
        ctx.diagnostic(parsing_error_diagnostic("eof-in-tag", value.span));
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
        ];

        Tester::new(NoParsingError::NAME, NoParsingError::PLUGIN, pass, fail).test_and_snapshot();
    }
}
