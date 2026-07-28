use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::{Sfc, SfcBlock};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    vue_template::{VueSfcRule, VueTemplateContext},
};

fn block_order_diagnostic(
    name: &str,
    name_attrs: &str,
    span: Span,
    first_name: &str,
    first_attrs: &str,
    line: usize,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "'<{name}{name_attrs}>' should be above '<{first_name}{first_attrs}>' on line {line}."
    ))
    .with_help("Reorder the SFC's top-level blocks to match the configured `order`.")
    .with_label(span)
}

/// One entry of the `order` option: either a single selector, or a group of
/// selectors that share the same rank (no relative order is enforced
/// between members of the same group — mirrors upstream's default
/// `[["script", "template"], "style"]`).
#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(untagged)]
enum OrderEntry {
    Single(String),
    Group(Vec<String>),
}

fn default_order() -> Vec<OrderEntry> {
    vec![
        OrderEntry::Group(vec!["script".to_string(), "template".to_string()]),
        OrderEntry::Single("style".to_string()),
    ]
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct BlockOrderConfig {
    /// The required top-level block order. Each entry is either a single
    /// block-name selector or an array of selectors that rank equally
    /// (their relative order isn't checked against each other). A selector
    /// is a block name (`"script"`, `"template"`, `"style"`, or a custom
    /// block name), optionally followed by `[attr]` / `[attr=value]`
    /// attribute constraints (e.g. `"script[setup]"`). Default:
    /// `[["script", "template"], "style"]`.
    #[serde(default = "default_order")]
    order: Vec<OrderEntry>,
}

impl Default for BlockOrderConfig {
    fn default() -> Self {
        Self { order: default_order() }
    }
}

// Boxed: `order: Vec<OrderEntry>` alone is 24 bytes, which would blow
// `RuleEnum`'s 16-byte budget (see `no_v_html.rs` for the same pattern).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct BlockOrder(Box<BlockOrderConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a consistent order for a Vue SFC's top-level blocks
    /// (`<script>`, `<template>`, `<style>`, and any custom blocks). The
    /// default order is `[["script", "template"], "style"]` — `<script>`
    /// and `<template>` may appear in either relative order, but both must
    /// come before `<style>`.
    ///
    /// ### Why is this bad?
    ///
    /// A consistent block order across a codebase's SFCs makes files easier
    /// to scan and diff.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (default order):
    /// ```vue
    /// <style>
    /// .a { color: red; }
    /// </style>
    /// <template>
    ///   <div class="a" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule (default order):
    /// ```vue
    /// <template>
    ///   <div class="a" />
    /// </template>
    /// <style>
    /// .a { color: red; }
    /// </style>
    /// ```
    BlockOrder,
    vue,
    style,
    config = BlockOrder,
    version = "1.77.0",
    short_description = "Enforce order of component top-level elements.",
);

impl Rule for BlockOrder {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueSfcRule for BlockOrder {
    fn run_on_sfc<'a>(&self, sfc: &Sfc<'a>, _path: &Path, ctx: &mut VueTemplateContext<'a>) {
        let mut orders: Vec<(Selector, usize)> = Vec::new();
        for (index, entry) in self.0.order.iter().enumerate() {
            match entry {
                OrderEntry::Single(text) => orders.push((parse_selector(text), index)),
                OrderEntry::Group(items) => {
                    for text in items {
                        orders.push((parse_selector(text), index));
                    }
                }
            }
        }

        // Only top-level blocks that match a selector participate in
        // ordering at all — an unmatched block (e.g. a custom block not
        // named in `order`) is skipped entirely, same as upstream's
        // `elementsWithOrder`.
        let ordered: Vec<(&SfcBlock<'a>, usize)> = sfc
            .blocks
            .iter()
            .filter_map(|block| {
                orders
                    .iter()
                    .find(|(selector, _)| selector_matches(selector, block))
                    .map(|(_, index)| (block, *index))
            })
            .collect();

        let source = ctx.source_text();
        for (i, (block, order_index)) in ordered.iter().enumerate() {
            // The earliest-ranked block appearing before this one, out of
            // those that rank strictly after it — i.e. the block this one
            // should have been placed above.
            let first_unordered = ordered[..i]
                .iter()
                .filter(|(_, other_index)| *order_index < *other_index)
                .min_by_key(|(_, other_index)| *other_index);

            if let Some((first_block, _)) = first_unordered {
                ctx.diagnostic(block_order_diagnostic(
                    block.name,
                    &attribute_suffix(block),
                    opening_tag_span(source, block.span.start),
                    first_block.name,
                    &attribute_suffix(first_block),
                    line_number(source, first_block.span.start),
                ));
            }
        }
    }
}

/// `name` or `name=value` for each plain (non-directive) attribute, mirroring
/// upstream's `getAttributeString` — used to render e.g. `<script setup>` in
/// the diagnostic message.
fn attribute_suffix(block: &SfcBlock<'_>) -> String {
    let attrs = block
        .attributes
        .iter()
        .filter(|attribute| attribute.directive.is_none())
        .map(|attribute| match &attribute.value {
            Some(value) if !value.text.is_empty() => format!("{}={}", attribute.name, value.text),
            _ => attribute.name.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    if attrs.is_empty() { String::new() } else { format!(" {attrs}") }
}

/// From `<` of the opening tag to (and including) its closing `>`, tracking
/// quoted attribute values so a `>` inside e.g. `lang=">"` doesn't end the
/// scan early.
fn opening_tag_span(source: &str, start: u32) -> Span {
    let bytes = source.as_bytes();
    let mut i = start as usize;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let byte = bytes[i];
        match quote {
            Some(q) => {
                if byte == q {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => {
                    return Span::new(
                        start,
                        u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1),
                    );
                }
                _ => {}
            },
        }
        i += 1;
    }
    Span::new(start, u32::try_from(bytes.len()).unwrap_or(u32::MAX))
}

/// 1-indexed line number of the byte offset `pos` within `source`.
fn line_number(source: &str, pos: u32) -> usize {
    source[..pos as usize].matches('\n').count() + 1
}

/// A parsed `order` selector: an optional tag name (`None` matches any tag)
/// plus zero or more `[attr]` / `[attr=value]` constraints.
///
/// This is a deliberately small subset of upstream's full CSS-selector
/// engine (`postcss-selector-parser`, which also supports combinators,
/// pseudo-classes, and class/id selectors) — SFC top-level blocks are a
/// flat, sibling-only list with no meaningful class/id concept, and every
/// real-world `block-order` config found in practice uses only plain tag
/// names or a single `tag[attr]` constraint (e.g. `"script[setup]"` to rank
/// `<script setup>` separately from `<script>`).
#[derive(Debug, Clone)]
struct Selector {
    tag: Option<String>,
    attrs: Vec<(String, Option<String>)>,
}

fn parse_selector(text: &str) -> Selector {
    let tag_end = text.find('[').unwrap_or(text.len());
    let tag_part = text[..tag_end].trim();
    let tag =
        if tag_part.is_empty() || tag_part == "*" { None } else { Some(tag_part.to_string()) };

    let mut attrs = Vec::new();
    let mut rest = &text[tag_end..];
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else { break };
        let inner = &rest[open + 1..open + close];
        attrs.push(match inner.find('=') {
            Some(eq) => {
                let name = inner[..eq].trim().to_string();
                let value = inner[eq + 1..].trim();
                let value = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .unwrap_or(value);
                (name, Some(value.to_string()))
            }
            None => (inner.trim().to_string(), None),
        });
        rest = &rest[open + close + 1..];
    }

    Selector { tag, attrs }
}

fn selector_matches(selector: &Selector, block: &SfcBlock<'_>) -> bool {
    if let Some(tag) = &selector.tag
        && block.name != tag
    {
        return false;
    }
    selector.attrs.iter().all(|(name, value)| match value {
        Some(value) => block.attribute_value(name) == Some(value.as_str()),
        None => block.has_attribute(name),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::BlockOrder;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Default order: script then template then style.
            (
                "<script>1</script><template><div/></template><style>.a{}</style>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default order: script and template may appear in either
            // relative order (same group).
            (
                "<template><div/></template><script>1</script><style>.a{}</style>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No style block at all.
            (
                "<script>1</script><template><div/></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Only a template.
            ("<template><div/></template>", None, None, Some(PathBuf::from("test.vue"))),
            // A custom block not named in `order` doesn't participate.
            (
                "<docs>hi</docs><script>1</script><template><div/></template><style>.a{}</style>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom `order` respected.
            (
                "<template><div/></template><script>1</script><style>.a{}</style>",
                Some(json!([{ "order": ["template", "script", "style"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Attribute selector: `<script setup>` ranked separately and
            // correctly, plain `<script>` is unmatched (and thus
            // unconstrained) under this config.
            (
                "<script setup>1</script><template><div/></template>",
                Some(json!([{ "order": ["script[setup]", "template"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Default order: style before script/template.
            (
                "<style>.a{}</style><script>1</script><template><div/></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<style>.a{}</style><template><div/></template><script>1</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Unmatched custom block doesn't shield the following blocks
            // from being checked against the block before it.
            (
                "<docs>hi</docs><style>.a{}</style><script>1</script><template><div/></template>",
                Some(json!([{ "order": [["script", "template"], "style"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom order violated.
            (
                "<script>1</script><template><div/></template>",
                Some(json!([{ "order": ["template", "script"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Attribute selector: `<script setup>` must be above `<template>`
            // under this custom order.
            (
                "<template><div/></template><script setup>1</script>",
                Some(json!([{ "order": ["script[setup]", "template"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(BlockOrder::NAME, BlockOrder::PLUGIN, pass, fail).test_and_snapshot();
    }
}
