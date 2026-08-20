//! Laying out an element's children.
//!
//! A transliteration of `prettier-plugin-svelte`'s `printChildren`. That
//! function decides layout and prints in one pass, reaching back to rewrap
//! the doc it just emitted and trimming text nodes in place as it goes.
//! Neither is available here — the IR is appended to a buffer, and the tree
//! belongs to the caller — so the same decisions are taken first, into a
//! plan, and the printing reads the plan.
//!
//! The trimming matters to more than the printing: once a text node has
//! given its trailing whitespace to a neighbour, later checks see it as
//! *not* ending in whitespace, and lay the next child out accordingly. So
//! every check below reads the value as trimmed so far, never the original.

use svelte_markup_parser::ast::Node;

use crate::options::WhitespaceSensitivity;

use super::classify::{
    ends_with_collapsible_whitespace, ends_with_line_breaks, is_block_element, is_empty_text,
    is_inline_element, is_only_collapsible_whitespace, starts_with_collapsible_whitespace,
    starts_with_line_breaks, trimmed,
};

/// How much of a text child's own whitespace the surrounding layout has
/// taken responsibility for, and must therefore not print again.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Trim {
    pub left: bool,
    pub right: bool,
}

/// What the layout puts around one child.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChildLayout {
    /// The child is the last thing inside a block element, so nothing renders
    /// after it and `bracketSameLine` may keep its `>` on the previous line.
    pub last_in_block_parent: bool,
    /// The child is kept exactly as written: a `prettier-ignore` before it,
    /// or an enclosing `prettier-ignore-start` range.
    pub verbatim: bool,
    /// A `softline` before the child, so a block breaks onto its own line.
    pub softline_before: bool,
    /// A `softline` after it, for the same reason.
    pub softline_after: bool,
    /// The child is emitted as `group([line, child])`: an inline element
    /// taking over the whitespace of the text that preceded it.
    pub hug_previous_whitespace: bool,
    /// The child is emitted as `group([child, line])`: the leading
    /// whitespace of the text that follows, moved onto this child.
    pub trailing_line: bool,
    pub trim: Trim,
}

/// The laid-out children of one element.
#[derive(Debug, Default)]
pub struct ChildrenPlan {
    /// One entry per child that is printed, in source order, as
    /// `(index into the original children, layout)`.
    pub entries: Vec<(usize, ChildLayout)>,
    /// Whether the content must break: set when a block element sits among
    /// more than one child.
    pub force_break: bool,
}

impl ChildrenPlan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Decide the layout of `children`, honouring trims the caller has already
/// applied to the first and last child.
pub fn plan_children(children: &[Node<'_>], outer: &[Trim], context: PlanContext) -> ChildrenPlan {
    let all: Vec<usize> = (0..children.len()).collect();
    plan_some_children(children, &all, outer, context)
}

/// What the enclosing layout tells the plan about these children.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext {
    pub sensitivity: WhitespaceSensitivity,
    /// Whether the parent is a block element, which decides whether the last
    /// child may keep a `bracketSameLine` `>` on the line before it.
    pub parent_is_block: bool,
    /// Whether these are the component's own top-level nodes, which is where
    /// a `prettier-ignore-start` range is recognised.
    pub top_level: bool,
}

/// As [`plan_children`], over a chosen subset of the children — the top-level
/// sections are laid out one at a time, and each is a subset of the file's
/// nodes rather than a contiguous run.
pub fn plan_some_children(
    children: &[Node<'_>],
    candidates: &[usize],
    outer: &[Trim],
    context: PlanContext,
) -> ChildrenPlan {
    let sensitivity = context.sensitivity;
    let trim_of = |index: usize| outer.get(index).copied().unwrap_or_default();
    // Text that is empty once the caller's trims are applied is not a child
    // at all; every index below is into this list, and `entries` carries the
    // original index back out.
    let prepared: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&index| match &children[index] {
            Node::Text(text) => {
                let trim = trim_of(index);
                !trimmed(text.value, trim.left, trim.right).is_empty()
            }
            _ => true,
        })
        .collect();

    let mut plan = ChildrenPlan::default();
    if prepared.is_empty() {
        return plan;
    }

    let mut layouts: Vec<ChildLayout> = prepared
        .iter()
        .map(|&index| ChildLayout { trim: trim_of(index), ..ChildLayout::default() })
        .collect();

    // Whether the text child just laid out gave its trailing whitespace to
    // whatever comes next, which then has to print a `line` of its own.
    let mut previous_text_gave_up_whitespace = false;

    for position in 0..prepared.len() {
        let node = &children[prepared[position]];
        if matches!(node, Node::Text(_)) {
            plan_text_child(
                children,
                &prepared,
                &mut layouts,
                position,
                sensitivity,
                &mut previous_text_gave_up_whitespace,
            );
        } else if is_block_element(node, sensitivity) {
            plan_block_child(
                children,
                &prepared,
                &mut layouts,
                position,
                sensitivity,
                previous_text_gave_up_whitespace,
            );
            previous_text_gave_up_whitespace = false;
        } else if is_inline_element(node, sensitivity) {
            layouts[position].hug_previous_whitespace = previous_text_gave_up_whitespace;
            previous_text_gave_up_whitespace = false;
        } else {
            previous_text_gave_up_whitespace = false;
        }
    }

    // More than one child with a block among them can never sit on one line.
    plan.force_break = prepared.len() > 1
        && prepared.iter().any(|&index| is_block_element(&children[index], sensitivity));
    if context.parent_is_block
        && let Some(last) = layouts.last_mut()
    {
        last.last_in_block_parent = true;
    }
    mark_ignored(children, &prepared, &mut layouts, context.top_level);
    plan.entries = prepared.into_iter().zip(layouts).collect();
    plan
}

/// Flag the children a `prettier-ignore` covers.
///
/// `<!-- prettier-ignore -->` covers the next child that is printed at all;
/// `<!-- prettier-ignore-start -->` covers everything up to its `-end`, and
/// only among the component's top-level nodes, which is where Prettier
/// recognises the pair.
fn mark_ignored(
    children: &[Node<'_>],
    prepared: &[usize],
    layouts: &mut [ChildLayout],
    top_level: bool,
) {
    let mut ignore_next = false;
    let mut in_range = false;
    for (position, &index) in prepared.iter().enumerate() {
        let node = &children[index];
        let directive = ignore_directive(node);
        if top_level && directive == Some(Directive::Start) {
            in_range = true;
        } else if top_level && directive == Some(Directive::End) {
            in_range = false;
        } else if directive == Some(Directive::Next) {
            ignore_next = true;
        } else if is_empty_text(node) {
            // The whitespace between the directive and what it covers is not
            // what it covers.
        } else if ignore_next || in_range {
            layouts[position].verbatim = true;
            ignore_next = false;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Directive {
    Next,
    Start,
    End,
}

fn ignore_directive(node: &Node<'_>) -> Option<Directive> {
    let Node::Comment(comment) = node else { return None };
    match comment.content.trim() {
        "prettier-ignore" => Some(Directive::Next),
        "prettier-ignore-start" => Some(Directive::Start),
        "prettier-ignore-end" => Some(Directive::End),
        _ => None,
    }
}

/// The text of the child at `position`, as trimmed so far, or `None` when it
/// is not a text node.
fn value_at<'a>(
    children: &[Node<'a>],
    prepared: &[usize],
    layouts: &[ChildLayout],
    position: usize,
) -> Option<&'a str> {
    let Node::Text(text) = &children[prepared[position]] else { return None };
    let trim = layouts[position].trim;
    Some(trimmed(text.value, trim.left, trim.right))
}

/// A block child gets a `softline` on each side, unless the neighbour on
/// that side already provides the break.
fn plan_block_child(
    children: &[Node<'_>],
    prepared: &[usize],
    layouts: &mut [ChildLayout],
    position: usize,
    sensitivity: WhitespaceSensitivity,
    previous_text_gave_up_whitespace: bool,
) {
    if let Some(previous_position) = position.checked_sub(1) {
        let previous = &children[prepared[previous_position]];
        let previous_ends_with_whitespace =
            value_at(children, prepared, layouts, previous_position)
                .is_some_and(ends_with_collapsible_whitespace);
        let previous_is_text = matches!(previous, Node::Text(_));
        if !is_block_element(previous, sensitivity)
            && (!previous_is_text
                || previous_text_gave_up_whitespace
                || !previous_ends_with_whitespace)
        {
            layouts[position].softline_before = true;
        }
    }

    if position + 1 < prepared.len() {
        let next_position = position + 1;
        let next_value = value_at(children, prepared, layouts, next_position);
        let needs_softline = match next_value {
            Some(value) => {
                // Only text that starts with whitespace and then says
                // something — or is empty but followed by an inline element,
                // so that element lands on its own line when things break.
                let followed_by_inline = prepared
                    .get(position + 2)
                    .is_some_and(|&index| is_inline_element(&children[index], sensitivity));
                (!is_only_collapsible_whitespace(value) || followed_by_inline)
                    && !starts_with_line_breaks(value, 1)
            }
            None => true,
        };
        if needs_softline {
            layouts[position].softline_after = true;
        }
    }
}

/// A text child hands its outer whitespace to the neighbours that can render
/// it as a break, and stops printing that whitespace itself.
fn plan_text_child(
    children: &[Node<'_>],
    prepared: &[usize],
    layouts: &mut [ChildLayout],
    position: usize,
    sensitivity: WhitespaceSensitivity,
    previous_text_gave_up_whitespace: &mut bool,
) {
    *previous_text_gave_up_whitespace = false;

    // The first and last child's outer whitespace belongs to the element's
    // own layout, which has already dealt with it.
    if position == 0 || position == prepared.len() - 1 {
        return;
    }

    let Some(value) = value_at(children, prepared, layouts, position) else { return };
    let previous = &children[prepared[position - 1]];
    let next = &children[prepared[position + 1]];
    let is_empty = is_only_collapsible_whitespace(value);

    if starts_with_collapsible_whitespace(value) && !is_empty {
        if is_inline_element(previous, sensitivity) && !starts_with_line_breaks(value, 1) {
            layouts[position].trim.left = true;
            layouts[position - 1].trailing_line = true;
        }
        if is_block_element(previous, sensitivity) && !starts_with_line_breaks(value, 1) {
            layouts[position].trim.left = true;
        }
    }

    // Re-read: the left trim above may have consumed the whole run.
    let Some(value) = value_at(children, prepared, layouts, position) else { return };
    if ends_with_collapsible_whitespace(value) {
        if is_inline_element(next, sensitivity) && !ends_with_line_breaks(value, 1) {
            *previous_text_gave_up_whitespace = !is_block_element(previous, sensitivity);
            layouts[position].trim.right = true;
        }
        if is_block_element(next, sensitivity) && !ends_with_line_breaks(value, 2) {
            *previous_text_gave_up_whitespace = !is_block_element(previous, sensitivity);
            layouts[position].trim.right = true;
        }
    }
}
