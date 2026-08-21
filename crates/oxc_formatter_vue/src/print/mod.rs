//! The component printer.
//!
//! A `.vue` file is one HTML document whose top-level elements happen to be
//! blocks, so it is printed as one: the same markup rules apply to
//! `<template>`'s contents and to the `<script>` tag around a program. What
//! each block's *body* is written in is another language's business, and goes
//! through the session's dispatcher (see [`embed`]).
//!
//! The layout follows Prettier's HTML printer, whose central idea is in
//! [`tag`]: where whitespace is significant a break cannot go between two
//! nodes, so the tag delimiters move instead.

use oxc_formatter_core::{
    Buffer, Format, Formatter,
    builders::{group, text},
    write,
};
use oxc_span::Span;

use crate::context::VueFormatContext;

use self::{
    children::write_children,
    element::write_element,
    embed::{element_language, write_interpolation_text, write_script_like_text},
    tag::{
        write_closing_tag_end, write_closing_tag_suffix, write_opening_tag_prefix,
        write_opening_tag_start,
    },
    text::write_text,
    tree::{Kind, NodeId, ROOT, Tree},
};

mod attribute;
mod children;
pub mod classify;
mod element;
mod embed;
mod tag;
mod text;
pub mod tree;

pub type VueFormatter<'buf, 'a> = Formatter<'buf, 'a, VueFormatContext<'a>>;

/// Print the whole component.
pub fn write_root<'a>(tree: &Tree<'_, 'a>, f: &mut VueFormatter<'_, 'a>) {
    write!(f, group(&format_with(|f: &mut VueFormatter<'_, 'a>| write_children(tree, ROOT, f))));
}

/// Print one node, whatever it is.
pub fn write_node<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let node = tree.node(id);
    match node.kind {
        Kind::Root => write_children(tree, id, f),
        Kind::Element => write_element(tree, id, f),
        Kind::Interpolation => {
            write_opening_tag_start(tree, id, f);
            for child in &node.children {
                write_node(tree, *child, f);
            }
            write_closing_tag_end(tree, id, f);
        }
        Kind::Text => {
            let parent = node.parent.expect("a text node always has a parent");
            if tree.node(parent).kind == Kind::Interpolation {
                write_interpolation_text(tree, id, f);
            } else if tree.is_script_like(parent) {
                write_script_like_text(tree, id, element_language(tree, parent), f);
            } else {
                write_text(tree, id, f);
            }
        }
        // A comment is the author's prose; it keeps every byte of its
        // spelling, and only the delimiters around it may move.
        Kind::Comment => {
            write_opening_tag_prefix(tree, id, f);
            write_source(tree, node.span, f);
            write_closing_tag_suffix(tree, id, f);
        }
        Kind::Raw => write_source(tree, node.span, f),
    }
}

fn write_source<'a>(tree: &Tree<'_, 'a>, span: Span, f: &mut VueFormatter<'_, 'a>) {
    write!(f, text(tree.text(span)));
}

/// A [`Format`] from a closure, so a layout reads as the shape it produces
/// rather than as buffer plumbing.
pub fn format_with<'a, F>(closure: F) -> FormatWith<F>
where
    F: Fn(&mut VueFormatter<'_, 'a>),
{
    FormatWith(closure)
}

pub struct FormatWith<F>(F);

impl<'a, F> Format<'a, VueFormatContext<'a>> for FormatWith<F>
where
    F: Fn(&mut VueFormatter<'_, 'a>),
{
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        (self.0)(f);
    }
}
