//! Laying out a run of sibling nodes.
//!
//! Every gap between two siblings gets exactly one answer — nothing, a break
//! that may become a space, a break that may vanish, or a break that is always
//! taken — and [`between_line`] is where that answer is decided. The rest of
//! this module is about *whose* group the chosen break belongs to, which is
//! what makes a line wrap take its neighbour with it instead of stranding a
//! tag on a line by itself.

use oxc_formatter_core::{
    Buffer, GroupId,
    builders::{
        empty_line, expand_parent, group, hard_line_break, if_group_fits_on_line, soft_line_break,
        soft_line_break_or_space, text,
    },
    write,
};

use super::{
    VueFormatter, format_with,
    tag::{
        closing_tag_end_marker, closing_tag_suffix, needs_to_borrow_next_opening_tag_start_marker,
        needs_to_borrow_parent_closing_tag_start_marker,
        needs_to_borrow_prev_closing_tag_end_marker, opening_tag_prefix, opening_tag_start_marker,
    },
    tree::{Kind, NodeId, Tree},
    write_node,
};

/// What goes between two siblings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BetweenLine {
    /// Nothing at all: the two nodes touch, and a break would change what the
    /// page renders.
    None,
    /// A break that renders as a space when it is not taken.
    Line,
    /// A break that renders as nothing when it is not taken.
    SoftLine,
    /// A break that is always taken.
    HardLine,
    /// A blank line, which the author wrote and the printer keeps. Never
    /// returned by [`between_line`] — only the layout above it decides that a
    /// gap in the source survives.
    Blank,
}

impl BetweenLine {
    fn write(self, f: &mut VueFormatter<'_, '_>) {
        match self {
            Self::None => {}
            Self::Line => write!(f, soft_line_break_or_space()),
            Self::SoftLine => write!(f, soft_line_break()),
            Self::HardLine => write!(f, hard_line_break()),
            Self::Blank => write!(f, empty_line()),
        }
    }
}

/// The break that belongs between `prev` and `next`.
pub fn between_line(tree: &Tree<'_, '_>, prev: NodeId, next: NodeId) -> BetweenLine {
    let prev_node = tree.node(prev);
    let next_node = tree.node(next);

    // Two runs of prose: the gap is a word break, or it is nothing.
    if prev_node.kind.is_text_like() && next_node.kind.is_text_like() {
        if prev_node.is_trailing_space_sensitive {
            if !prev_node.has_trailing_spaces {
                return BetweenLine::None;
            }
            return if prefer_hardline_as_leading_spaces(tree, next) {
                BetweenLine::HardLine
            } else {
                BetweenLine::Line
            };
        }
        return if prefer_hardline_as_leading_spaces(tree, next) {
            BetweenLine::HardLine
        } else {
            BetweenLine::SoftLine
        };
    }

    // A delimiter has already been borrowed across this position, so the
    // break travels with it rather than being written here.
    let prev_lends_opening_marker = needs_to_borrow_next_opening_tag_start_marker(tree, prev)
        && (tree.has_prettier_ignore(next)
            || tree.first_child(next).is_some()
            || next_node.is_self_closing
            || (next_node.kind == Kind::Element && !next_node.attributes().is_empty()));
    let prev_is_self_closing_and_lends = prev_node.kind == Kind::Element
        && prev_node.is_self_closing
        && needs_to_borrow_prev_closing_tag_end_marker(tree, next);
    if prev_lends_opening_marker || prev_is_self_closing_and_lends {
        return BetweenLine::None;
    }

    // `<div>{{ x }}<!-- note --></div>`: the comment sits right against what
    // precedes it, so the break must be able to vanish.
    if next_node.kind == Kind::Comment
        && next_node.is_leading_space_sensitive
        && !next_node.has_leading_spaces
    {
        return BetweenLine::SoftLine;
    }

    // Three borrows deep, the closing tags have nowhere left to go but a line
    // of their own — Prettier's `</a\n>.` case.
    let deep_borrow_chain = needs_to_borrow_prev_closing_tag_end_marker(tree, next)
        && tree.last_child(prev).is_some_and(|last| {
            needs_to_borrow_parent_closing_tag_start_marker(tree, last)
                && tree.last_child(last).is_some_and(|inner| {
                    needs_to_borrow_parent_closing_tag_start_marker(tree, inner)
                })
        });
    if !next_node.is_leading_space_sensitive
        || prefer_hardline_as_leading_spaces(tree, next)
        || deep_borrow_chain
    {
        return BetweenLine::HardLine;
    }

    if next_node.has_leading_spaces { BetweenLine::Line } else { BetweenLine::SoftLine }
}

/// Whether the whitespace before the node should become a line break rather
/// than a space that may or may not be taken.
fn prefer_hardline_as_leading_spaces(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    prefer_hardline_as_surrounding_spaces(tree, id)
        || tree.prev(id).is_some_and(|prev| prefer_hardline_as_trailing_spaces(tree, prev))
        || has_surrounding_line_break(tree, id)
}

fn prefer_hardline_as_trailing_spaces(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    prefer_hardline_as_surrounding_spaces(tree, id)
        // A `<br />` *is* a line break; nothing should share its line.
        || (node.kind == Kind::Element && node.name() == "br")
        || has_surrounding_line_break(tree, id)
}

fn prefer_hardline_as_surrounding_spaces(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    match node.kind {
        Kind::Comment | Kind::Raw => true,
        Kind::Element => matches!(node.name(), "script" | "select"),
        _ => false,
    }
}

/// Whether the author put the node on a line of its own, which the printer
/// keeps rather than reflowing.
fn has_surrounding_line_break(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    has_leading_line_break(tree, id) && has_trailing_line_break(tree, id)
}

pub fn has_leading_line_break(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    if !node.has_leading_spaces {
        return false;
    }
    match tree.prev(id) {
        Some(prev) => tree.line_of(tree.node(prev).span.end) < tree.line_of(node.span.start),
        None => node.parent.is_some_and(|parent| {
            tree.node(parent).kind == Kind::Root
                || tree.line_of(tree.start_span(parent).end) < tree.line_of(node.span.start)
        }),
    }
}

pub fn has_trailing_line_break(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    if !node.has_trailing_spaces {
        return false;
    }
    match tree.next(id) {
        Some(next) => tree.line_of(tree.node(next).span.start) > tree.line_of(node.span.end),
        None => node.parent.is_some_and(|parent| {
            tree.node(parent).kind == Kind::Root
                || tree
                    .end_span(parent)
                    .is_some_and(|span| tree.line_of(span.start) > tree.line_of(node.span.end))
        }),
    }
}

/// Whether the author left a blank line after the node, which is preserved.
pub fn force_next_empty_line(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let Some(next) = tree.next(id) else { return false };
    tree.line_of(tree.node(id).span.end) + 1 < tree.line_of(tree.node(next).span.start)
}

/// Whether the element's content can never sit on one line with its tags.
pub fn force_break_content(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    if force_break_children(tree, id) {
        return true;
    }
    let node = tree.node(id);
    if node.kind == Kind::Element && !node.children.is_empty() {
        if matches!(node.name(), "body" | "script" | "style") {
            return true;
        }
        // A grandchild that is not text means real structure below, which
        // reads as a block however short it is.
        if node.children.iter().any(|child| has_non_text_child(tree, *child)) {
            return true;
        }
    }
    // A lone non-text child the author put on its own line stays on it.
    let (Some(first), Some(last)) = (tree.first_child(id), tree.last_child(id)) else {
        return false;
    };
    first == last
        && tree.node(first).kind != Kind::Text
        && has_leading_line_break(tree, first)
        && (!tree.node(last).is_trailing_space_sensitive || has_trailing_line_break(tree, last))
}

fn has_non_text_child(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    tree.node(id).children.iter().any(|child| tree.node(*child).kind != Kind::Text)
}

/// Whether every gap between the element's children is a line break: a list
/// or a table never puts two items on one line.
pub fn force_break_children(tree: &Tree<'_, '_>, id: NodeId) -> bool {
    let node = tree.node(id);
    node.kind == Kind::Element
        && !node.children.is_empty()
        && (matches!(node.name(), "html" | "head" | "ul" | "ol" | "select")
            || (node.css_display.is_table_part()
                && node.css_display != super::classify::Display::TableCell))
}

/// Print the children of `id`.
pub fn write_children<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let children = tree.node(id).children.clone();

    if force_break_children(tree, id) {
        write!(f, expand_parent());
        for child in children {
            if let Some(prev) = tree.prev(child) {
                let between = between_line(tree, prev, child);
                if between != BetweenLine::None {
                    // A blank line the author left between two items survives
                    // here as it does anywhere else. One element, not a break
                    // plus a break: the printer only starts a new line once
                    // per line of output.
                    if force_next_empty_line(tree, prev) {
                        write!(f, empty_line());
                    } else {
                        between.write(f);
                    }
                }
            }
            write_child(tree, child, f);
        }
        return;
    }

    // One group per child, so a later sibling can ask whether the one before
    // it had to break.
    let group_ids: Vec<GroupId> =
        children.iter().map(|_| f.state().group_id("vue-child")).collect();

    for (index, &child) in children.iter().enumerate() {
        if tree.node(child).kind.is_text_like() {
            write_text_like_child(tree, child, f);
            continue;
        }
        write_structured_child(tree, child, index, &group_ids, f);
    }
}

/// A run of prose, which only ever needs the break before it.
fn write_text_like_child<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if let Some(prev) = tree.prev(id)
        && tree.node(prev).kind.is_text_like()
    {
        let between = between_line(tree, prev, id);
        if between != BetweenLine::None {
            if force_next_empty_line(tree, prev) {
                // One element, not two hard breaks: this printer only starts
                // a new line once per line of output, so a pair would
                // collapse into a single break.
                write!(f, empty_line());
            } else {
                between.write(f);
            }
        }
    }
    write_child(tree, id, f);
}

/// A node with a tag of its own, whose surrounding breaks are split between
/// its own group and the enclosing one.
///
/// A break that is always taken goes outside the group — it is not the child's
/// to decide. A break that might vanish goes inside, so it wraps together with
/// the content it belongs to.
fn write_structured_child<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    index: usize,
    group_ids: &[GroupId],
    f: &mut VueFormatter<'_, 'a>,
) {
    let prev = tree.prev(id);
    let next = tree.next(id);
    let prev_between = prev.map_or(BetweenLine::None, |prev| between_line(tree, prev, id));
    let next_between = next.map_or(BetweenLine::None, |next| between_line(tree, id, next));

    // Before the group: the breaks the layout has already committed to.
    let mut leading = BetweenLine::None;
    let mut leading_from_previous_group = false;
    if prev_between != BetweenLine::None {
        let prev = prev.expect("a break before implies a previous sibling");
        if force_next_empty_line(tree, prev) {
            write!(f, empty_line());
        } else if prev_between == BetweenLine::HardLine {
            write!(f, hard_line_break());
        } else if tree.node(prev).kind.is_text_like() {
            leading = prev_between;
        } else {
            // The previous sibling's group decides: if it had to break, the
            // break between the two is already there.
            leading_from_previous_group = true;
        }
    }

    let mut trailing = BetweenLine::None;
    // The break *after* the node, which only a following run of prose takes:
    // anything else prints its own leading break.
    let mut after = BetweenLine::None;
    if next_between != BetweenLine::None {
        let next = next.expect("a break after implies a next sibling");
        let next_is_text_like = tree.node(next).kind.is_text_like();
        if force_next_empty_line(tree, id) {
            if next_is_text_like {
                after = BetweenLine::Blank;
            }
        } else if next_between == BetweenLine::HardLine {
            if next_is_text_like {
                after = BetweenLine::HardLine;
            }
        } else {
            trailing = next_between;
        }
    }

    let inner_id = group_ids[index];
    let previous_id = index.checked_sub(1).map(|previous| group_ids[previous]);
    write!(
        f,
        group(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            if leading_from_previous_group {
                write!(f, if_group_fits_on_line(&soft_line_break()).with_group_id(previous_id));
            } else {
                leading.write(f);
            }
            write!(
                f,
                group(&format_with(|f: &mut VueFormatter<'_, 'a>| {
                    write_child(tree, id, f);
                    trailing.write(f);
                }))
                .with_group_id(Some(inner_id))
            );
        }))
    );

    after.write(f);
}

/// One child, or the source it was written as when the author said not to
/// touch it.
fn write_child<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    if !tree.has_prettier_ignore(id) {
        write_node(tree, id, f);
        return;
    }

    // Reproduce the source, minus whatever a neighbour borrowed: those
    // delimiters are printed by the neighbour, so leaving them here too would
    // duplicate them.
    let node = tree.node(id);
    let mut start = node.span.start as usize;
    if tree.prev(id).is_some_and(|prev| needs_to_borrow_next_opening_tag_start_marker(tree, prev)) {
        start += opening_tag_start_marker(tree, id).len();
    }
    let mut end = end_location(tree, id) as usize;
    if tree.next(id).is_some_and(|next| needs_to_borrow_prev_closing_tag_end_marker(tree, next)) {
        end -= closing_tag_end_marker(tree, id).len();
    }

    write!(f, opening_tag_prefix(tree, id));
    write!(f, text(tree.source()[start..end].trim_end()));
    write!(f, closing_tag_suffix(tree, id));
}

/// Where a node ends in the source. An element the author never closed ends
/// where its last child does, not where the parser had to stop.
fn end_location(tree: &Tree<'_, '_>, id: NodeId) -> u32 {
    let node = tree.node(id);
    let end = node.span.end;
    if node.kind == Kind::Element && tree.end_span(id).is_none() && !node.children.is_empty() {
        let last = *node.children.last().expect("checked non-empty");
        return end.max(end_location(tree, last));
    }
    end
}
