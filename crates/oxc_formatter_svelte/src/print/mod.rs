//! The markup printer.
//!
//! S2 prints the markup itself — elements, attributes, text, comments — and
//! passes everything else through as the source wrote it. Each later stage
//! replaces one of those passthroughs: `<script>` and `<style>` bodies in
//! S3, `{…}` expressions in S4, `{#…}` blocks in S5.

use oxc_formatter_core::{
    Buffer, Format, Formatter,
    builders::{empty_line, expand_parent, group, soft_line_break, soft_line_break_or_space, text},
    write,
};
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, Node};

use crate::context::SvelteFormatContext;

use crate::options::SortOrder;

use self::{
    block::{write_block, write_tag},
    children::{ChildLayout, Trim, plan_children, plan_some_children},
    classify::{is_empty_text, is_only_collapsible_whitespace},
    element::write_element,
    expression::write_mustache,
    text::write_text,
};

mod attribute;
mod block;
mod children;
mod classify;
mod element;
mod expression;
mod raw_text;
mod text;

pub type SvelteFormatter<'buf, 'a> = Formatter<'buf, 'a, SvelteFormatContext<'a>>;

/// Print the component's top-level nodes.
///
/// The file's own edges are not content: whatever whitespace sits before the
/// first node and after the last is dropped, and the single trailing newline
/// is written by the caller.
pub fn write_root<'a>(nodes: &[Node<'a>], f: &mut SvelteFormatter<'_, 'a>) {
    let order = f.options().sort_order;
    if order == SortOrder::None {
        // The file's contents are one group: a component that fits on one
        // line stays on one line, whatever the layout inside it would allow.
        write!(f, group(&FormatRoot { nodes, section: None }));
        return;
    }

    // A component's top-level sections are printed in the configured order,
    // wherever the author put them, separated by a blank line.
    let mut first = true;
    for section in order.sections() {
        // Whitespace alone is not a section: the blank line between sections
        // is written here, so a run of it left over at the file's edge must
        // not conjure one.
        let present = nodes
            .iter()
            .enumerate()
            .any(|(index, node)| !is_empty_text(node) && section_of(nodes, index, node) == section);
        if !present {
            continue;
        }
        if !first {
            // A blank line between sections. Two hard breaks would collapse
            // into one: the printer only writes a newline onto a line that
            // has something on it.
            write!(f, empty_line());
        }
        first = false;
        write!(f, group(&FormatRoot { nodes, section: Some(section) }));
    }
}

/// Whether a `<script>` is the component's module script, which is printed
/// before its instance script.
fn is_module_script(node: &Node<'_>) -> bool {
    let Node::Element(element) = node else { return false };
    element.attributes.iter().any(|attribute| {
        attribute.name().is_some_and(|name| name == "module")
            || matches!(&attribute.kind, AttributeKind::Plain { name: "context", value: Some(value), .. }
                if value.as_static_text() == Some("module"))
    })
}

/// Which top-level section a root node belongs to.
fn section_of(nodes: &[Node<'_>], index: usize, node: &Node<'_>) -> Section {
    match node {
        Node::Element(element) if element.name.eq_ignore_ascii_case("script") => Section::Scripts,
        Node::Element(element) if element.name.eq_ignore_ascii_case("style") => Section::Styles,
        Node::Element(element) if element.svelte_name() == Some("options") => Section::Options,
        // Whitespace between two sections belongs to neither: the blank line
        // between them is written by the ordering, not carried across.
        Node::Text(text) if is_only_collapsible_whitespace(text.value) => {
            neighbour_section(nodes, index)
        }
        _ => Section::Markup,
    }
}

/// The section a run of whitespace goes with: the markup, unless it sits
/// between two nodes of the same non-markup section.
fn neighbour_section(nodes: &[Node<'_>], index: usize) -> Section {
    let before = nodes[..index]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, node)| !matches!(node, Node::Text(text) if is_only_collapsible_whitespace(text.value)))
        .map(|(i, node)| section_of(nodes, i, node));
    let after = nodes[index + 1..]
        .iter()
        .enumerate()
        .find(|(_, node)| !matches!(node, Node::Text(text) if is_only_collapsible_whitespace(text.value)))
        .map(|(i, node)| section_of(nodes, index + 1 + i, node));
    match (before, after) {
        (Some(before), Some(after)) if before == after => before,
        _ => Section::Markup,
    }
}

/// One of the four groups a component's top level splits into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Options,
    Scripts,
    Markup,
    Styles,
}

struct FormatRoot<'n, 'a> {
    nodes: &'n [Node<'a>],
    /// The section to print, or `None` for every node in source order.
    section: Option<Section>,
}

impl<'a> Format<'a, SvelteFormatContext<'a>> for FormatRoot<'_, 'a> {
    fn fmt(&self, f: &mut SvelteFormatter<'_, 'a>) {
        match self.section {
            None => write_root_nodes(self.nodes, f),
            Some(Section::Markup) => {
                let indices: Vec<usize> = (0..self.nodes.len())
                    .filter(|&index| {
                        section_of(self.nodes, index, &self.nodes[index]) == Section::Markup
                    })
                    .collect();
                write_root_of(self.nodes, &indices, f);
            }
            // The other three sections are made of whole elements, and the
            // whitespace the author put between them is not carried across —
            // they are separated by a blank line wherever they came from.
            Some(section) => {
                let mut elements: Vec<usize> = (0..self.nodes.len())
                    .filter(|&index| {
                        matches!(&self.nodes[index], Node::Element(_))
                            && section_of(self.nodes, index, &self.nodes[index]) == section
                    })
                    .collect();
                if section == Section::Scripts {
                    // `<script module>` comes first however it was written.
                    elements.sort_by_key(|&index| !is_module_script(&self.nodes[index]));
                }
                for (position, index) in elements.iter().enumerate() {
                    if position > 0 {
                        write!(f, empty_line());
                    }
                    write_node(&self.nodes[*index], Trim::default(), f);
                }
            }
        }
    }
}

fn write_root_nodes<'a>(nodes: &[Node<'a>], f: &mut SvelteFormatter<'_, 'a>) {
    let all: Vec<usize> = (0..nodes.len()).collect();
    write_root_of(nodes, &all, f);
}

/// Print the given root nodes, dropping the whitespace at either end of the
/// run: the file's own edges are not content, and the trailing newline is
/// the caller's.
fn write_root_of<'a>(nodes: &[Node<'a>], indices: &[usize], f: &mut SvelteFormatter<'_, 'a>) {
    let mut trims = vec![Trim::default(); nodes.len()];
    let position = |at: usize| indices.iter().position(|&index| index == at);
    let first = indices.iter().copied().find(|&index| !is_empty_text(&nodes[index]));
    let first_position = first.and_then(position).unwrap_or(indices.len().saturating_sub(1));
    for &index in indices.iter().take(first_position + 1) {
        trims[index].left = true;
    }
    let last = indices.iter().copied().rev().find(|&index| !is_empty_text(&nodes[index]));
    let last_position = last.and_then(position).unwrap_or(0);
    for &index in indices.iter().skip(last_position) {
        trims[index].right = true;
    }
    let plan = plan_some_children(nodes, indices, &trims);
    for (index, layout) in &plan.entries {
        write_child(&nodes[*index], *layout, f);
    }
    if plan.force_break {
        write!(f, expand_parent());
    }
}

/// Print a run of sibling nodes, laid out by [`plan_children`].
pub fn write_children<'a>(children: &[Node<'a>], outer: &[Trim], f: &mut SvelteFormatter<'_, 'a>) {
    write_planned(children, outer, true, f);
}

fn write_planned<'a>(
    children: &[Node<'a>],
    outer: &[Trim],
    keep_force_break: bool,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    let plan = plan_children(children, outer);
    if plan.is_empty() {
        return;
    }
    for (index, layout) in &plan.entries {
        write_child(&children[*index], *layout, f);
    }
    if plan.force_break && keep_force_break {
        // At least one block among several children: the content can never
        // sit on one line.
        write!(f, expand_parent());
    }
}

fn write_child<'a>(node: &Node<'a>, layout: ChildLayout, f: &mut SvelteFormatter<'_, 'a>) {
    if layout.softline_before {
        write!(f, soft_line_break());
    }

    if layout.hug_previous_whitespace || layout.trailing_line {
        // The break travels with the node, so the two move together when the
        // content wraps.
        write!(f, group(&FormatNode { node, layout }));
    } else {
        write_node(node, layout.trim, f);
    }

    if layout.softline_after {
        write!(f, soft_line_break());
    }
}

/// A node together with the break the layout puts beside it.
struct FormatNode<'n, 'a> {
    node: &'n Node<'a>,
    layout: ChildLayout,
}

impl<'a> Format<'a, SvelteFormatContext<'a>> for FormatNode<'_, 'a> {
    fn fmt(&self, f: &mut SvelteFormatter<'_, 'a>) {
        if self.layout.hug_previous_whitespace {
            write!(f, soft_line_break_or_space());
        }
        write_node(self.node, self.layout.trim, f);
        if self.layout.trailing_line {
            write!(f, soft_line_break_or_space());
        }
    }
}

fn write_node<'a>(node: &Node<'a>, trim: Trim, f: &mut SvelteFormatter<'_, 'a>) {
    match node {
        Node::Element(element) => write_element(element, f),
        // `write_text` handles the whitespace-only case itself, once the
        // trim has been applied — a node the layout has taken over entirely
        // must print nothing at all.
        Node::Text(node_text) => write_text(node_text.value, trim, f),
        Node::Mustache(tag) => write_mustache(tag, f),
        Node::Tag(tag) => write_tag(tag, f),
        Node::Block(block) => write_block(block, f),
        // A comment is the author's prose; it keeps its exact spelling.
        Node::Comment(comment) => write_source(comment.span, f),
        Node::Raw(span) => write_source(*span, f),
    }
}

/// Write a span of the source exactly as it was written.
pub fn write_source(span: Span, f: &mut SvelteFormatter<'_, '_>) {
    let source = f.context().source_text().as_str();
    write!(f, text(&source[span.start as usize..span.end as usize]));
}

/// A `Format` from a closure, so the layout above reads as the shape it
/// produces rather than as buffer plumbing.
fn format_with<'a, F>(closure: F) -> FormatWith<F>
where
    F: Fn(&mut SvelteFormatter<'_, 'a>),
{
    FormatWith(closure)
}

struct FormatWith<F>(F);

impl<'a, F> Format<'a, crate::context::SvelteFormatContext<'a>> for FormatWith<F>
where
    F: Fn(&mut SvelteFormatter<'_, 'a>),
{
    fn fmt(&self, f: &mut SvelteFormatter<'_, 'a>) {
        (self.0)(f);
    }
}
