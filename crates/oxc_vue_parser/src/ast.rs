//! Template AST.
//!
//! All nodes carry [`Span`]s into the original source so both the formatter
//! (which needs byte-faithful reprinting) and the linter (which needs
//! diagnostics locations) can use the same tree.

use oxc_span::Span;

#[derive(Debug)]
pub enum Node<'a> {
    Element(Element<'a>),
    /// A run of text with no markup in it.
    Text(Text<'a>),
    /// `{{ expr }}`.
    Interpolation(Interpolation<'a>),
    /// `<!-- ... -->`, `content_span` excludes the delimiters.
    Comment(Comment<'a>),
    /// Doctype, processing instructions, and anything unrecognised —
    /// copied through as-is.
    Raw(Span),
}

impl Node<'_> {
    pub fn span(&self) -> Span {
        match self {
            Node::Element(element) => element.span,
            Node::Text(text) => text.span,
            Node::Interpolation(interpolation) => interpolation.span,
            Node::Comment(comment) => comment.span,
            Node::Raw(span) => *span,
        }
    }
}

#[derive(Debug)]
pub struct Text<'a> {
    pub span: Span,
    pub value: &'a str,
}

#[derive(Debug)]
pub struct Interpolation<'a> {
    /// Includes the `{{ }}` delimiters.
    pub span: Span,
    /// The expression text between the delimiters, untrimmed.
    pub expression_span: Span,
    pub expression: &'a str,
}

#[derive(Debug)]
pub struct Comment<'a> {
    pub span: Span,
    pub content_span: Span,
    pub content: &'a str,
}

#[derive(Debug)]
pub struct Element<'a> {
    /// From `<` to the end of the closing tag (or of the open tag when
    /// self-closing / void / unclosed).
    pub span: Span,
    pub name: &'a str,
    pub name_span: Span,
    pub attributes: Vec<Attribute<'a>>,
    pub children: Vec<Node<'a>>,
    /// Written as `<x />`.
    pub self_closing: bool,
    /// A void element like `<br>` (never has children or a closing tag).
    pub is_void: bool,
    /// For raw-text elements (`pre`, `textarea`) the body span is kept
    /// byte-for-byte and `children` is empty.
    pub raw_text: Option<Span>,
    /// `true` when the closing tag was missing in the source
    /// (recovered; the element ends where its parent closes).
    pub unclosed: bool,
    /// Byte offset immediately after the opening tag's `>` (or after the
    /// `/>` of a self-closing/void element). Kept because it can't be
    /// recovered from `children` when the element has none — e.g. an empty
    /// `<template></template>`, where the content starts at this offset,
    /// not at `span.end`.
    pub open_tag_end: u32,
}

impl Element<'_> {
    /// Component name heuristic per the Vue style guide: anything that is not
    /// a known HTML/SVG/MathML element or a built-in is a component usage.
    /// Kept simple: capitalised names and names containing `-` count as
    /// components/custom elements.
    pub fn is_component_like(&self) -> bool {
        self.name.starts_with(|c: char| c.is_ascii_uppercase()) || self.name.contains('-')
    }
}

/// One attribute of an element, with any directive syntax decomposed.
#[derive(Debug)]
pub struct Attribute<'a> {
    pub span: Span,
    /// The raw attribute name exactly as written, e.g. `@click.stop`,
    /// `:class`, `v-for`, `#default`, `disabled`.
    pub name: &'a str,
    pub name_span: Span,
    /// The value between the quotes, when present.
    pub value: Option<AttributeValue<'a>>,
    /// `Some` when the attribute is any directive form
    /// (`v-*`, `:`, `.`, `@`, `#`).
    pub directive: Option<Directive<'a>>,
}

#[derive(Debug)]
pub struct AttributeValue<'a> {
    /// Excludes the quotes.
    pub span: Span,
    pub text: &'a str,
    /// `"`, `'`, or `0` when the value was unquoted.
    pub quote: u8,
}

/// A decomposed directive: `v-on:click.stop` / `@click.stop` →
/// name `on`, argument `click`, modifiers `[stop]`, shorthand `@`.
#[derive(Debug)]
pub struct Directive<'a> {
    /// The canonical directive name without the `v-` prefix:
    /// `bind`, `on`, `if`, `for`, `slot`, `model`, custom names, ….
    pub name: &'a str,
    /// `click` in `@click`, `class` in `:class`, `[key]` for dynamic
    /// arguments (brackets included).
    pub argument: Option<DirectiveArgument<'a>>,
    /// `.stop`, `.prevent`, … in source order, without the dots.
    pub modifiers: Vec<&'a str>,
    /// Which shorthand introduced it, if any.
    pub shorthand: Option<DirectiveShorthand>,
}

#[derive(Debug)]
pub struct DirectiveArgument<'a> {
    pub span: Span,
    pub text: &'a str,
    /// `:[someKey]` / `@[event]` dynamic arguments.
    pub dynamic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveShorthand {
    /// `:` for `v-bind:`
    Bind,
    /// `.` for `v-bind:*.prop`
    BindProp,
    /// `@` for `v-on:`
    On,
    /// `#` for `v-slot:`
    Slot,
}

/// Elements that never have children or a closing tag.
pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose content is not markup and must survive untouched.
/// (`script`/`style` never appear inside `<template>` in practice, but the
/// browser treats them as raw text, so the parser must too.)
pub const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style", "pre", "textarea"];

pub fn is_void_element(name: &str) -> bool {
    VOID_ELEMENTS.iter().any(|void| void.eq_ignore_ascii_case(name))
}

pub fn is_raw_text_element(name: &str) -> bool {
    RAW_TEXT_ELEMENTS.iter().any(|raw| raw.eq_ignore_ascii_case(name))
}
