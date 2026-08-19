use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn no_textarea_mustache_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected mustache. Use 'v-model' instead.")
        .with_help("`<textarea>` content is not reactive to mustache changes; bind it with `v-model` instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoTextareaMustache;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `{{ }}` mustache interpolations inside `<textarea>`.
    ///
    /// ### Why is this bad?
    ///
    /// Vue's compiler does interpolate mustaches placed inside `<textarea>`,
    /// but the result overwrites the element's `value` only once, at render
    /// time; it never updates again as the underlying data changes, unlike
    /// every other use of mustache interpolation. `v-model` is the correct,
    /// reactive way to bind a `<textarea>`'s content.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <textarea>{{ text }}</textarea>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <textarea v-model="text"></textarea>
    /// </template>
    /// ```
    NoTextareaMustache,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow mustaches in `<textarea>`.",
);

impl Rule for NoTextareaMustache {}

impl VueTemplateRule for NoTextareaMustache {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let mut spans = Vec::new();
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue's selector matches `rawName='textarea'`
            // exactly (case-sensitively) — not `name` (which some other
            // rules' selectors use). In an SFC, an uppercase/mixed-case tag
            // like `<TEXTAREA>` resolves as a *component* reference, not the
            // native HTML element, so it must NOT match here even though
            // this fork's own parser still treats it as raw text for
            // reprinting purposes (case-insensitively, matching real HTML
            // tokenizing rules) — verified against a real eslint-plugin-vue
            // run: `<TEXTAREA>{{ x }}</TEXTAREA>` is not flagged.
            if element.name != "textarea" {
                return;
            }
            let Some(raw_text) = element.raw_text else { return };
            spans.extend(find_mustaches(source_text, raw_text));
        });
        for span in spans {
            ctx.diagnostic(no_textarea_mustache_diagnostic(span));
        }
    }
}

/// Scans `raw_text`'s slice of `source` for `{{ ... }}` mustaches.
///
/// This fork's parser treats `<textarea>` as a raw-text element (like
/// `<script>`/`<style>`) for byte-faithful reprinting, so its body
/// never becomes `Interpolation` nodes the way ordinary text content does —
/// this rule has to re-scan the raw bytes itself, mirroring
/// `vue_sfc_parser`'s own `{{` → matching `}}` interpolation scanner
/// (`Parser::interpolation`).
///
/// An unterminated `{{` (no matching `}}` before the element closes) is not
/// reported and stops the scan — verified against a real
/// `eslint-plugin-vue@10.9.1` run: `<textarea>{{ unterminated</textarea>`
/// produces no `no-textarea-mustache` diagnostic, matching
/// vue-eslint-parser's own mustache scanner, which never emits a
/// `VExpressionContainer` for an unterminated `{{`.
fn find_mustaches(source: &str, raw_text: Span) -> Vec<Span> {
    let bytes = source.as_bytes();
    let end = raw_text.end as usize;
    let mut spans = Vec::new();
    let mut index = raw_text.start as usize;
    while index + 1 < end {
        if bytes[index] != b'{' || bytes[index + 1] != b'{' {
            index += 1;
            continue;
        }
        let start = index;
        let mut scan = index + 2;
        let mut closed = false;
        while scan + 1 < end {
            if bytes[scan] == b'}' && bytes[scan + 1] == b'}' {
                closed = true;
                break;
            }
            scan += 1;
        }
        if !closed {
            break;
        }
        let close_end = scan + 2;
        spans.push(Span::new(
            u32::try_from(start).unwrap_or(raw_text.start),
            u32::try_from(close_end).unwrap_or(raw_text.end),
        ));
        index = close_end;
    }
    spans
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoTextareaMustache;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><textarea v-model="text"></textarea></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><textarea>plain text</textarea></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Mustaches elsewhere are not this rule's concern.
            (
                r"<template><div>{{ text }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Unterminated `{{` inside a textarea: not reported (verified
            // against a real eslint-plugin-vue run — see find_mustaches's
            // doc comment).
            (
                r"<template><textarea>{{ unterminated</textarea></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><textarea /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Element-name matching is case-SENSITIVE (mirrors upstream's
            // `rawName='textarea'` selector, not `name`): an uppercase or
            // mixed-case tag resolves as a component reference in an SFC,
            // not the native `<textarea>` element, so mustaches inside it
            // are not this rule's concern. Verified against a real
            // eslint-plugin-vue run — see the tag-name check's doc comment.
            (
                r"<template><TEXTAREA>{{ text }}</TEXTAREA></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r"<template><textarea>{{ text }}</textarea></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Multiple mustaches, each reported.
            (
                r"<template><textarea>{{ a }} and {{ b }}</textarea></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoTextareaMustache::NAME, NoTextareaMustache::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
