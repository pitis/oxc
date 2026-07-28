//! `.vue` single-file-component block splitting.
//!
//! Splits a file into its top-level blocks without interpreting their
//! contents. Two rules matter for correctness:
//!
//! - `<script>` and `<style>` are raw text: nothing inside them — not even
//!   text that looks like `</template>` in a string or a `<script>` mention
//!   in a comment — opens or closes blocks. Only their own closing tag ends
//!   them.
//! - `<template>` nests: a `<template>` element *inside* the template block
//!   must not close the block.

use oxc_span::Span;

use crate::ast::{Attribute, AttributeValue};
use crate::parser::parse_template;

#[derive(Debug)]
pub struct Sfc<'a> {
    pub blocks: Vec<SfcBlock<'a>>,
    /// Spans of non-whitespace top-level content (stray text, and markup
    /// such as unmatched closing tags or doctypes consumed as `Raw`) that
    /// falls between/outside recognised blocks. Whitespace-only text is not
    /// included. Lets a future no-parsing-error rule surface it instead of
    /// it silently vanishing.
    pub orphan_spans: Vec<Span>,
}

#[derive(Debug)]
pub struct SfcBlock<'a> {
    /// From `<` of the open tag to `>` of the close tag.
    pub span: Span,
    /// `template`, `script`, `style`, or a custom block name.
    pub name: &'a str,
    pub attributes: Vec<Attribute<'a>>,
    /// The content between the tags.
    pub content_span: Span,
    pub content: &'a str,
    /// `true` when the block's closing tag was missing in the source (it
    /// was recovered at EOF). An unclosed `<script>`/`<style>` swallows the
    /// rest of the file as its content — consumers may want to warn.
    pub unclosed: bool,
}

impl<'a> SfcBlock<'a> {
    /// The `lang` attribute value, e.g. `ts` on `<script lang="ts">`.
    pub fn lang(&self) -> Option<&'a str> {
        self.attribute_value("lang")
    }

    /// Whether the block has a bare attribute, e.g. `setup` / `scoped`.
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|attribute| attribute.name == name)
    }

    pub fn attribute_value(&self, name: &str) -> Option<&'a str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .and_then(|attribute| attribute.value.as_ref())
            .map(|value: &AttributeValue<'a>| value.text)
    }
}

/// Split a `.vue` file into blocks.
///
/// Content outside recognised block structure is not attached to any block,
/// matching how Vue's own SFC compiler treats it; non-whitespace stray
/// content is still surfaced via [`Sfc::orphan_spans`] rather than being
/// silently dropped.
pub fn parse_sfc(source: &str) -> Sfc<'_> {
    // The template parser already implements exactly the tag scanning,
    // attribute parsing, raw-text and nesting rules needed at the top level:
    // `script`/`style` are raw-text elements, `template` nests.
    // A `.vue` file at the top level IS a tiny HTML document.
    let nodes = parse_template(source);
    let mut blocks = Vec::new();
    let mut orphan_spans = Vec::new();
    for node in nodes {
        match node {
            crate::ast::Node::Element(element) => {
                let content_span = match element.raw_text {
                    Some(span) => span,
                    // `<template>`: content spans from after the open tag to
                    // before the close tag; recover it from child spans.
                    // Child text nodes cover every byte, so first/last spans
                    // are exact. An empty block has no children to recover
                    // from, so anchor at the open tag's `>` instead of
                    // falling back to `element.span.end`, which is past the
                    // close tag entirely.
                    None => match (element.children.first(), element.children.last()) {
                        (Some(first), Some(last)) => Span::new(first.span().start, last.span().end),
                        _ => Span::empty(element.open_tag_end),
                    },
                };
                blocks.push(SfcBlock {
                    span: element.span,
                    name: element.name,
                    attributes: element.attributes,
                    content_span,
                    content: &source[content_span.start as usize..content_span.end as usize],
                    unclosed: element.unclosed,
                });
            }
            // Non-whitespace top-level text, and markup that doesn't belong
            // to a block (stray closing tags, doctypes, etc., consumed as
            // `Raw`) is content that would otherwise silently vanish.
            crate::ast::Node::Text(text) => {
                if !text.value.trim().is_empty() {
                    orphan_spans.push(text.span);
                }
            }
            crate::ast::Node::Raw(span) => orphan_spans.push(span),
            crate::ast::Node::Comment(_) | crate::ast::Node::Interpolation(_) => {}
        }
    }
    Sfc { blocks, orphan_spans }
}
