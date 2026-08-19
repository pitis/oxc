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

/// The span of an element's closing tag, `</name>` included, or `None` when
/// it has none — a self-closing or void element, or one the parser had to
/// recover because the tag was missing.
///
/// The element's span ends at the closing tag's `>`, and the only thing
/// between that and the tag's `<` is the name and whitespace, so scanning
/// back for `<` finds the tag exactly.
pub fn svelte_end_tag_span(element: &Element<'_>, source_text: &str) -> Option<Span> {
    if element.self_closing || element.is_void || element.unclosed {
        return None;
    }
    let end = element.span.end as usize;
    let start = source_text.get(..end)?.rfind('<')?;
    let start = u32::try_from(start).ok()?;
    // A tag that starts before the element's own content is not a closing tag.
    (start >= element.open_tag_end).then(|| Span::new(start, element.span.end))
}

/// One `<script>` block of a `.svelte` file, located for on-demand parsing
/// by hybrid markup rules (rules that need both template and script).
///
/// Pure-script rules should NOT use this: they run as ordinary rules on the
/// extracted script sub-hosts with full semantic analysis. This exists for
/// markup-pass rules that additionally need to look into the script (e.g.
/// `bind:this` targets, `$props()` declarations).
#[derive(Debug, Clone, Copy)]
pub struct SvelteScript<'a> {
    /// The script body text, exactly the element's raw-text span.
    pub content: &'a str,
    /// File offset of `content`'s first byte: add to any span produced by
    /// parsing `content` to get a file-absolute span.
    pub offset: u32,
    /// `lang="ts"` (or `lang="tsx"`).
    pub typescript: bool,
}

/// The `<script>` blocks of a parsed `.svelte` file, in source order.
pub fn svelte_scripts<'a>(nodes: &[Node<'a>], source_text: &'a str) -> Vec<SvelteScript<'a>> {
    let mut scripts = Vec::new();
    walk_svelte_elements(nodes, &mut |element| {
        if !element.name.eq_ignore_ascii_case("script") {
            return;
        }
        let Some(raw) = element.raw_text else { return };
        let typescript = get_plain_attribute(element, "lang")
            .and_then(|(_, value)| value.and_then(AttributeValue::as_static_text))
            .is_some_and(|lang| {
                lang.eq_ignore_ascii_case("ts") || lang.eq_ignore_ascii_case("tsx")
            });
        scripts.push(SvelteScript {
            content: &source_text[raw.start as usize..raw.end as usize],
            offset: raw.start,
            typescript,
        });
    });
    scripts
}
