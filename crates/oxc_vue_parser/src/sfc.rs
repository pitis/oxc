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

/// Split a `.vue` file into blocks. Content outside recognised block
/// structure (comments, whitespace, stray text) is ignored, matching how
/// Vue's own SFC compiler treats it.
pub fn parse_sfc(source: &str) -> Sfc<'_> {
    // The template parser already implements exactly the tag scanning,
    // attribute parsing, raw-text and nesting rules needed at the top level:
    // `script`/`style` are raw-text elements, `template` nests.
    // A `.vue` file at the top level IS a tiny HTML document.
    let nodes = parse_template(source);
    let mut blocks = Vec::new();
    for node in nodes {
        if let crate::ast::Node::Element(element) = node {
            let content_span = match element.raw_text {
                Some(span) => span,
                // `<template>`: content spans from after the open tag to
                // before the close tag; recover it from child spans. Child
                // text nodes cover every byte, so first/last spans are exact.
                // For an empty block the exact `>` position is unknown
                // without re-scanning; a zero-length span is good enough.
                None => match (element.children.first(), element.children.last()) {
                    (Some(first), Some(last)) => Span::new(first.span().start, last.span().end),
                    _ => Span::empty(element.span.end),
                },
            };
            blocks.push(SfcBlock {
                span: element.span,
                name: element.name,
                attributes: element.attributes,
                content_span,
                content: &source[content_span.start as usize..content_span.end as usize],
            });
        }
    }
    Sfc { blocks }
}
