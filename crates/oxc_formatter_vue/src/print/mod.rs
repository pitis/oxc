//! The SFC printer.
//!
//! Stage 1 prints the component's top-level blocks — their tags, their order,
//! the blank line between them — and hands `<script>` and `<style>` bodies to
//! the formatters that own those languages. The `<template>` body is passed
//! through as written; printing the markup itself is stage 2.

use cow_utils::CowUtils;
use oxc_formatter_core::{
    Buffer, BufferExtensions, DispatchRequest, DispatchResponse, Format, FormatElement, Formatter,
    InputKind,
    builders::{block_indent, empty_line, hard_line_break, text, token},
    write,
};
use oxc_span::Span;
use vue_sfc_parser::{Sfc, ast::Attribute};

use crate::context::VueFormatContext;

pub type VueFormatter<'buf, 'a> = Formatter<'buf, 'a, VueFormatContext<'a>>;

/// Print the whole component: every top-level block, in the order it was
/// written, separated by one blank line — which is what Prettier does for a
/// `.vue` file.
pub fn write_root<'a>(sfc: &Sfc<'a>, f: &mut VueFormatter<'_, 'a>) {
    let mut first = true;
    for block in &sfc.blocks {
        if !first {
            write!(f, empty_line());
        }
        first = false;
        write_block(block, f);
    }
}

fn write_block<'a>(block: &vue_sfc_parser::SfcBlock<'a>, f: &mut VueFormatter<'_, 'a>) {
    write!(f, [token("<"), text(block.name)]);
    for attribute in &block.attributes {
        write!(f, token(" "));
        write_attribute(attribute, f);
    }
    write!(f, token(">"));

    write_block_body(block, f);

    write!(f, [token("</"), text(block.name), token(">")]);
}

/// A block's attribute, printed rather than copied so spacing and quoting are
/// normalized: Prettier always writes a value in double quotes.
fn write_attribute<'a>(attribute: &Attribute<'a>, f: &mut VueFormatter<'_, 'a>) {
    write!(f, text(attribute.name));
    if let Some(value) = &attribute.value {
        write!(f, [token("=\""), text(value.text), token("\"")]);
    }
}

fn write_block_body<'a>(block: &vue_sfc_parser::SfcBlock<'a>, f: &mut VueFormatter<'_, 'a>) {
    let body = block.content;
    // Nothing between the tags at all, so they meet.
    if body.trim().is_empty() {
        if !body.is_empty() {
            write!(f, hard_line_break());
        }
        return;
    }

    let Some(language) = language_of(block) else {
        write_verbatim(block.content_span, f);
        return;
    };

    let response = f.session().dispatch(DispatchRequest {
        language,
        text: body,
        input_kind: InputKind::Fragment,
        parent_context: None,
    });
    let Ok(DispatchResponse::Formatted(payload)) = response else {
        write_verbatim(block.content_span, f);
        return;
    };

    let mut ir = payload.into_doc(f.context_mut());
    // A whole-program IR ends with the newline a *file* wants; here the close
    // tag supplies it.
    while matches!(ir.last(), Some(FormatElement::Line(_))) {
        ir.pop();
    }
    let content = WriteElements(std::cell::RefCell::new(Some(ir)));
    if f.options().indent_script_and_style {
        write!(f, block_indent(&content));
    } else {
        write!(f, [hard_line_break(), &content, hard_line_break()]);
    }
}

/// The language a block's body is written in, or `None` when nothing here
/// owns it — including `<template>`, whose markup stage 2 will print.
fn language_of(block: &vue_sfc_parser::SfcBlock<'_>) -> Option<&'static str> {
    let lang = block.lang().unwrap_or("").cow_to_ascii_lowercase();
    match block.name {
        "script" => match lang.as_ref() {
            "" | "js" | "javascript" => Some("js"),
            "ts" | "typescript" => Some("ts"),
            "jsx" => Some("jsx"),
            "tsx" => Some("tsx"),
            _ => None,
        },
        "style" => match lang.as_ref() {
            "" | "css" | "postcss" | "pcss" => Some("css"),
            "scss" => Some("scss"),
            "less" => Some("less"),
            _ => None,
        },
        _ => None,
    }
}

/// Keep a span byte for byte.
fn write_verbatim(span: Span, f: &mut VueFormatter<'_, '_>) {
    let source = f.context().source_text();
    write!(f, text(&source.as_str()[span.start as usize..span.end as usize]));
}

/// The child's IR, written once into whatever position the layout puts it.
struct WriteElements<'a>(
    std::cell::RefCell<Option<oxc_allocator::ArenaVec<'a, FormatElement<'a>>>>,
);

impl<'a> Format<'a, VueFormatContext<'a>> for WriteElements<'a> {
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        if let Some(elements) = self.0.borrow_mut().take() {
            f.write_elements(elements);
        }
    }
}
