//! Shared helpers for `svelte/*` markup rules (see `svelte_template.rs`).

use oxc_span::Span;
use svelte_markup_parser::ast::{
    Attribute, AttributeKind, AttributeValue, BlockKind, Element, Node,
};

/// Depth-first, source-order walk over every node in the tree, descending
/// into element children and every block branch. `visit` sees each node
/// before its descendants.
pub fn walk_svelte_nodes<'a>(nodes: &[Node<'a>], visit: &mut impl FnMut(&Node<'a>)) {
    for node in nodes {
        visit(node);
        match node {
            Node::Element(element) => walk_svelte_nodes(&element.children, visit),
            Node::Block(block) => match &block.kind {
                BlockKind::If(if_block) => {
                    for branch in &if_block.branches {
                        walk_svelte_nodes(&branch.children, visit);
                    }
                }
                BlockKind::Each(each) => {
                    walk_svelte_nodes(&each.children, visit);
                    if let Some(fallback) = &each.fallback {
                        walk_svelte_nodes(fallback, visit);
                    }
                }
                BlockKind::Await(await_block) => {
                    walk_svelte_nodes(&await_block.pending, visit);
                    if let Some(children) = &await_block.then_children {
                        walk_svelte_nodes(children, visit);
                    }
                    if let Some(children) = &await_block.catch_children {
                        walk_svelte_nodes(children, visit);
                    }
                }
                BlockKind::Key(key) => walk_svelte_nodes(&key.children, visit),
                BlockKind::Snippet(snippet) => walk_svelte_nodes(&snippet.children, visit),
                BlockKind::Unknown(unknown) => walk_svelte_nodes(&unknown.children, visit),
            },
            _ => {}
        }
    }
}

/// Depth-first walk visiting only elements (through block branches too).
pub fn walk_svelte_elements<'a>(nodes: &[Node<'a>], visit: &mut impl FnMut(&Element<'a>)) {
    walk_svelte_nodes(nodes, &mut |node| {
        if let Node::Element(element) = node {
            visit(element);
        }
    });
}

/// The first plain attribute named `name`, together with its value.
pub fn get_plain_attribute<'e, 'a>(
    element: &'e Element<'a>,
    name: &str,
) -> Option<(&'e Attribute<'a>, Option<&'e AttributeValue<'a>>)> {
    element.attributes.iter().find_map(|attribute| match &attribute.kind {
        AttributeKind::Plain { name: n, value, .. } if *n == name => {
            Some((attribute, value.as_ref()))
        }
        _ => None,
    })
}

/// Whether the element carries a `{...spread}` attribute — after which the
/// presence/absence of any specific attribute can no longer be decided
/// statically.
pub fn has_spread_attribute(element: &Element<'_>) -> bool {
    element
        .attributes
        .iter()
        .any(|attribute| matches!(attribute.kind, AttributeKind::Spread { .. }))
}

/// The span of an element's opening tag: from `<` through the closing `>`
/// (`open_tag_end`). The usual anchor for element-level reports.
pub fn svelte_start_tag_span(element: &Element<'_>) -> Span {
    Span::new(element.span.start, element.open_tag_end)
}
