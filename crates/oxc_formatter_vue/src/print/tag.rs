//! Tags, and the delimiters neighbours borrow from one another.
//!
//! This is the mechanism that makes HTML formatting different from every
//! other language's. Where whitespace is significant, a break cannot go
//! between two nodes — it would add a space the page renders. So the printer
//! puts the break *inside* the neighbouring tag instead, and the tag's
//! delimiter travels to whichever node owns the position where a break is
//! allowed:
//!
//! ```html
//! Use our<a
//!   ><b>mailing address</b></a
//! >.
//! ```
//!
//! The `>` that closes `<a`'s opening tag has moved onto the next line, and
//! the `>` that closes `</a` has moved down to sit beside the `.`. Neither
//! move changes a single rendered character. Every `needs_to_borrow_*`
//! predicate below answers one instance of "does my neighbour own my
//! delimiter?", and every `write_*` consults them so that exactly one of the
//! two prints it.

use oxc_formatter_core::{
    Buffer, Format,
    builders::{hard_line_break, indent, soft_line_break, soft_line_break_or_space, space, text},
    write,
};

use crate::context::VueFormatContext;

use super::{
    VueFormatter,
    attribute::write_attribute,
    format_with,
    tree::{Kind, NodeId, Tree},
};

/// A tag delimiter.
///
/// Carried as a value rather than written straight out because a
/// `prettier-ignore`d node is reproduced by slicing the source, and whatever
/// a neighbour borrowed has to be sliced back off — which needs the marker's
/// length without printing it.
#[derive(Clone, Copy)]
pub enum Marker<'a> {
    Empty,
    Static(&'static str),
    /// A delimiter that carries the element's name: `<div`, `</div`.
    Named(&'static str, &'a str),
}

impl Marker<'_> {
    pub fn len(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Static(text) => text.len(),
            Self::Named(prefix, name) => prefix.len() + name.len(),
        }
    }
}

impl<'a> Format<'a, VueFormatContext<'a>> for Marker<'a> {
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        match *self {
            Self::Empty => {}
            Self::Static(value) => write!(f, text(value)),
            Self::Named(prefix, name) => write!(f, [text(prefix), text(name)]),
        }
    }
}

// ---------------------------------------------------------------------------
// Who owns which delimiter
// ---------------------------------------------------------------------------

/// `<p></p\n>123` — the text owns the `>` that closes the previous tag,
/// because the break has to go before it and there is no space to break at.
pub fn needs_to_borrow_prev_closing_tag_end_marker(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let Some(prev) = tree.prev(id) else { return false };
    let node = tree.node(id);
    tree.node(prev).kind != Kind::Raw
        && !tree.node(prev).kind.is_text_like()
        && node.is_leading_space_sensitive
        && !node.has_leading_spaces
}

/// `<p\n  ><a></a\n  ></p\n>` — the parent's closing `>` belongs to its last
/// child, which is where the break can go.
pub fn needs_to_borrow_last_child_closing_tag_end_marker(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let Some(last) = tree.last_child(id) else { return false };
    let last_node = tree.node(last);
    last_node.is_trailing_space_sensitive
        && !last_node.has_trailing_spaces
        && !tree.node(tree.last_descendant(last)).kind.is_text_like()
        && !tree.is_pre_like(id)
}

/// `<p>\n  123</p\n>` — the trailing text owns the `</p`, so the break lands
/// before the tag rather than inside the rendered text.
pub fn needs_to_borrow_parent_closing_tag_start_marker(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    tree.next(id).is_none()
        && !node.has_trailing_spaces
        && node.is_trailing_space_sensitive
        && tree.node(tree.last_descendant(id)).kind.is_text_like()
}

/// `123<p\n>` — the text owns the `<p` that follows it.
pub fn needs_to_borrow_next_opening_tag_start_marker(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let Some(next) = tree.next(id) else { return false };
    let node = tree.node(id);
    !tree.node(next).kind.is_text_like()
        && node.kind.is_text_like()
        && node.is_trailing_space_sensitive
        && !node.has_trailing_spaces
}

/// `<p\n  >123` — the first child owns the `>` that ends the opening tag.
pub fn needs_to_borrow_parent_opening_tag_end_marker(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    tree.prev(id).is_none() && node.is_leading_space_sensitive && !node.has_leading_spaces
}

// ---------------------------------------------------------------------------
// The delimiters themselves
// ---------------------------------------------------------------------------

pub fn opening_tag_start_marker<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> Marker<'a> {
    match tree.node(id).kind {
        Kind::Interpolation => Marker::Static("{{"),
        Kind::Element => Marker::Named("<", tree.node(id).name()),
        _ => Marker::Empty,
    }
}

pub fn opening_tag_end_marker<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> Marker<'a> {
    let _ = (tree, id);
    Marker::Static(">")
}

pub fn closing_tag_start_marker<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> Marker<'a> {
    if should_not_print_closing_tag(tree, id) {
        return Marker::Empty;
    }
    Marker::Named("</", tree.node(id).name())
}

pub fn closing_tag_end_marker<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> Marker<'a> {
    if should_not_print_closing_tag(tree, id) {
        return Marker::Empty;
    }
    let node = tree.node(id);
    match node.kind {
        Kind::Interpolation => Marker::Static("}}"),
        Kind::Element if node.is_self_closing => Marker::Static("/>"),
        _ => Marker::Static(">"),
    }
}

/// An element the author never closed, whose content is being reproduced from
/// the source: fabricating the missing tag would change the document.
fn should_not_print_closing_tag(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    !node.is_self_closing
        && tree.end_span(id).is_none()
        && (tree.has_prettier_ignore(id)
            || node.parent.is_some_and(|parent| tree.should_preserve_content(parent)))
}

// ---------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------

pub fn write_opening_tag<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    write_opening_tag_start(tree, id, f);
    write_attributes(tree, id, f);
    if !tree.node(id).is_self_closing {
        write_opening_tag_end(tree, id, f);
    }
}

pub fn write_opening_tag_start<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if tree.prev(id).is_some_and(|prev| needs_to_borrow_next_opening_tag_start_marker(tree, prev)) {
        // The previous node already printed it.
        return;
    }
    write_opening_tag_prefix(tree, id, f);
    write!(f, opening_tag_start_marker(tree, id));
}

fn write_opening_tag_end<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if tree
        .first_child(id)
        .is_some_and(|child| needs_to_borrow_parent_opening_tag_end_marker(tree, child))
    {
        return;
    }
    write!(f, opening_tag_end_marker(tree, id));
}

/// Whatever this node borrowed from the node before it.
pub fn write_opening_tag_prefix<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    write!(f, opening_tag_prefix(tree, id));
}

pub fn opening_tag_prefix<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> Marker<'a> {
    if needs_to_borrow_parent_opening_tag_end_marker(tree, id) {
        let parent = tree.node(id).parent.expect("a node that borrows has a parent");
        return opening_tag_end_marker(tree, parent);
    }
    if needs_to_borrow_prev_closing_tag_end_marker(tree, id) {
        let prev = tree.prev(id).expect("a node that borrows from prev has one");
        return closing_tag_end_marker(tree, prev);
    }
    Marker::Empty
}

pub fn write_closing_tag<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if !tree.node(id).is_self_closing {
        write_closing_tag_start(tree, id, f);
    }
    write_closing_tag_end(tree, id, f);
}

fn write_closing_tag_start<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if tree
        .last_child(id)
        .is_some_and(|child| needs_to_borrow_parent_closing_tag_start_marker(tree, child))
    {
        return;
    }
    write_closing_tag_prefix(tree, id, f);
    write!(f, closing_tag_start_marker(tree, id));
}

pub fn write_closing_tag_end<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let borrowed = match tree.next(id) {
        Some(next) => needs_to_borrow_prev_closing_tag_end_marker(tree, next),
        None => tree
            .node(id)
            .parent
            .is_some_and(|parent| needs_to_borrow_last_child_closing_tag_end_marker(tree, parent)),
    };
    if borrowed {
        return;
    }
    write!(f, closing_tag_end_marker(tree, id));
    write_closing_tag_suffix(tree, id, f);
}

fn write_closing_tag_prefix<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if needs_to_borrow_last_child_closing_tag_end_marker(tree, id) {
        let last = tree.last_child(id).expect("the borrow implies a last child");
        write!(f, closing_tag_end_marker(tree, last));
    }
}

/// Whatever this node borrowed from the node after it, or from its parent.
pub fn write_closing_tag_suffix<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    write!(f, closing_tag_suffix(tree, id));
}

pub fn closing_tag_suffix<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> Marker<'a> {
    if needs_to_borrow_parent_closing_tag_start_marker(tree, id) {
        let parent = tree.node(id).parent.expect("a node that borrows has a parent");
        return closing_tag_start_marker(tree, parent);
    }
    if needs_to_borrow_next_opening_tag_start_marker(tree, id) {
        let next = tree.next(id).expect("the borrow implies a next sibling");
        return opening_tag_start_marker(tree, next);
    }
    Marker::Empty
}

/// The attribute list, and the break before the tag's own `>`.
fn write_attributes<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let node = tree.node(id);
    let attributes = node.attributes();

    if attributes.is_empty() {
        // `<br />` keeps the space before the slash; `<div>` has nothing to
        // separate.
        if node.is_self_closing {
            write!(f, space());
        }
        return;
    }

    // `<script src="…">` never breaks: the tag is the whole statement, and
    // wrapping it reads worse than a long line.
    let force_not_to_break = node.kind == Kind::Element
        && node.name() == "script"
        && attributes.len() == 1
        && attributes[0].name == "src"
        && node.children.is_empty();
    // A component's own block tags are exempt from `singleAttributePerLine`:
    // `<script setup lang="ts">` reads as one thing.
    let per_line =
        f.options().single_attribute_per_line && attributes.len() > 1 && !tree.is_vue_sfc_block(id);

    write!(
        f,
        indent(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            if force_not_to_break {
                write!(f, space());
            } else {
                write!(f, soft_line_break_or_space());
            }
            for (index, attribute) in attributes.iter().enumerate() {
                if index > 0 {
                    if per_line {
                        write!(f, hard_line_break());
                    } else {
                        write!(f, soft_line_break_or_space());
                    }
                }
                write_attribute(tree, id, attribute, f);
            }
        }))
    );

    // Where the `>` goes. When a neighbour has borrowed a delimiter across
    // this position there is no break to make: the borrowed marker is the
    // thing that moves, not this one.
    let lends_to_first_child = tree
        .first_child(id)
        .is_some_and(|child| needs_to_borrow_parent_opening_tag_end_marker(tree, child));
    let lends_to_parent = node.is_self_closing
        && node
            .parent
            .is_some_and(|parent| needs_to_borrow_last_child_closing_tag_end_marker(tree, parent));

    if lends_to_first_child || lends_to_parent || force_not_to_break {
        if node.is_self_closing {
            write!(f, space());
        }
        return;
    }
    if f.options().bracket_same_line {
        if node.is_self_closing {
            write!(f, space());
        }
        return;
    }
    if node.is_self_closing {
        write!(f, soft_line_break_or_space());
    } else {
        write!(f, soft_line_break());
    }
}
