//! Which elements are block-level, and how a tag relates to its content.
//!
//! Layout in HTML follows from whether whitespace around an element is
//! significant, so the printer has to know an element's display category
//! before it can decide where it may break.

use svelte_markup_parser::ast::{Element, Node};

use crate::options::WhitespaceSensitivity;

/// Elements CSS lays out as blocks, so surrounding whitespace does not show.
///
/// `prettier-plugin-svelte`'s own list, which is MDN's block-level elements.
static BLOCK_ELEMENTS: [&str; 33] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "dialog",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

/// Whether the node is a plain HTML element laid out as a block.
///
/// Components, `<svelte:*>` elements, blocks, text and tags are all `false`:
/// only a plain HTML tag qualifies, which is what makes whitespace around it
/// insignificant.
pub fn is_block_element(node: &Node<'_>, sensitivity: WhitespaceSensitivity) -> bool {
    let Node::Element(element) = node else { return false };
    is_block_tag(element, sensitivity)
}

pub fn is_block_tag(element: &Element<'_>, sensitivity: WhitespaceSensitivity) -> bool {
    is_plain_html(element)
        && match sensitivity {
            // Every element's surrounding whitespace matters, so none of them
            // may be moved onto a line of its own.
            WhitespaceSensitivity::Strict => false,
            // None of it does, so all of them may be.
            WhitespaceSensitivity::Ignore => true,
            WhitespaceSensitivity::Css => BLOCK_ELEMENTS.contains(&element.name),
        }
}

/// Whether the node is a plain HTML element laid out inline, where the
/// whitespace on either side of it is part of the rendered text.
pub fn is_inline_element(node: &Node<'_>, sensitivity: WhitespaceSensitivity) -> bool {
    let Node::Element(element) = node else { return false };
    is_inline_tag(element, sensitivity)
}

pub fn is_inline_tag(element: &Element<'_>, sensitivity: WhitespaceSensitivity) -> bool {
    is_plain_html(element) && !is_block_tag(element, sensitivity)
}

/// `<svelte:boundary>`, whose content never hugs its tags: its children are
/// what it swaps in and out, and they read as a block whatever they are.
pub fn is_boundary(element: &Element<'_>) -> bool {
    element.svelte_name() == Some("boundary")
}

/// An ordinary HTML tag: not a component, not `<svelte:…>`, and not one of
/// the two tags Svelte gives a node of its own — `<slot>` and `<title>`.
/// Those are laid out like neither a block nor an inline element: the
/// whitespace at their edges is dropped, but they do not force a break.
///
/// Svelte calls this a `RegularElement`, and a few rules key off it directly
/// rather than off the display category — see [`is_regular_element`].
pub fn is_regular_element(element: &Element<'_>) -> bool {
    is_plain_html(element)
}

fn is_plain_html(element: &Element<'_>) -> bool {
    !element.is_component_like()
        && element.svelte_name().is_none()
        && !matches!(element.name, "slot" | "title")
}

/// Whether the element renders its own whitespace, so its content is kept
/// exactly as written. Its *tag* is still printed as markup.
///
/// Only the HTML elements: a component may be called `<Textarea>` and very
/// often is, and what it renders is its own business. Svelte tells the two
/// apart by the capital, which is why the name alone is not the question.
pub fn is_pre_content(element: &Element<'_>) -> bool {
    is_regular_element(element)
        && (element.name.eq_ignore_ascii_case("pre")
            || element.name.eq_ignore_ascii_case("textarea"))
}

/// Whether the element's body is a foreign language this printer hands to
/// another formatter rather than printing as markup.
pub fn is_raw_text_element(element: &Element<'_>) -> bool {
    element.raw_text.is_some()
}

/// Whether the tag has no content to lay out, so the printer only has to
/// decide how to close it.
pub fn is_empty(children: &[Node<'_>]) -> bool {
    children.iter().all(is_empty_text)
}

/// A text node that is nothing but whitespace, which HTML collapses away.
pub fn is_empty_text(node: &Node<'_>) -> bool {
    matches!(node, Node::Text(text) if is_only_collapsible_whitespace(text.value))
}

/// The whitespace HTML collapses: space, tab, form feed, CR, LF.
pub fn is_collapsible_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

pub fn is_only_collapsible_whitespace(text: &str) -> bool {
    text.bytes().all(is_collapsible_whitespace)
}

pub fn starts_with_collapsible_whitespace(text: &str) -> bool {
    text.bytes().next().is_some_and(is_collapsible_whitespace)
}

pub fn ends_with_collapsible_whitespace(text: &str) -> bool {
    text.bytes().next_back().is_some_and(is_collapsible_whitespace)
}

/// Whether the text begins with `count` line breaks, ignoring the horizontal
/// whitespace around them.
pub fn starts_with_line_breaks(text: &str, count: usize) -> bool {
    let mut seen = 0;
    for byte in text.bytes() {
        match byte {
            b'\n' => {
                seen += 1;
                if seen == count {
                    return true;
                }
            }
            b' ' | b'\t' | b'\r' | 0x0c => {}
            _ => return false,
        }
    }
    false
}

/// The mirror of [`starts_with_line_breaks`], from the end.
pub fn ends_with_line_breaks(text: &str, count: usize) -> bool {
    let mut seen = 0;
    for byte in text.bytes().rev() {
        match byte {
            b'\n' => {
                seen += 1;
                if seen == count {
                    return true;
                }
            }
            b' ' | b'\t' | b'\r' | 0x0c => {}
            _ => return false,
        }
    }
    false
}

/// A text node's value with the whitespace the layout has already accounted
/// for removed from either end.
pub fn trimmed(text: &str, trim_left: bool, trim_right: bool) -> &str {
    let collapsible = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}');
    let mut value = text;
    if trim_left {
        value = value.trim_start_matches(collapsible);
    }
    if trim_right {
        value = value.trim_end_matches(collapsible);
    }
    value
}
