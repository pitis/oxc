//! The document tree the printer lays out.
//!
//! The parser produces a plain tree of spans. Everything the layout needs
//! beyond that — which whitespace is real content, which element's edges are
//! sensitive to it, what each node's CSS display is — is derived here, once,
//! into a flat table indexed by [`NodeId`]. This mirrors Prettier's HTML
//! preprocess pipeline, and keeping it as one explicit pass is what makes the
//! order of those derivations reviewable: `cssDisplay` has to be known before
//! space sensitivity can be, and space sensitivity before any tag is printed.
//!
//! The flat table also buys the printer what a tree of owned nodes cannot: a
//! node's previous and next *siblings*, which nearly every layout decision in
//! `tag.rs` asks about.

use oxc_allocator::Allocator;
use oxc_span::Span;
use vue_sfc_parser::ast::{Attribute, Element, Node, is_void_element};

use crate::options::VueFormatOptions;

use super::classify::{
    Display, WhiteSpace, element_display, element_white_space, forced_display,
    has_collapsible_whitespace, ignores_first_line_feed, is_only_collapsible_whitespace,
    leading_whitespace, trailing_whitespace,
};

pub type NodeId = usize;

/// The document itself, which every other node descends from.
pub const ROOT: NodeId = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Root,
    Element,
    Text,
    /// `{{ … }}`, whose one child is the expression as text.
    Interpolation,
    Comment,
    /// Anything the parser passed through untouched — a doctype, a processing
    /// instruction, a stray closing tag. Printed exactly as written.
    Raw,
}

impl Kind {
    /// Whether the node has no tag of its own to break around, so a run of
    /// them lays out as prose rather than as markup. Prettier's
    /// `isTextLikeNode`.
    pub fn is_text_like(self) -> bool {
        matches!(self, Self::Text | Self::Comment)
    }

    /// Whether the node is one that can have children at all, which is what
    /// Prettier tests as `node.children` being present.
    fn has_children_slot(self) -> bool {
        matches!(self, Self::Root | Self::Element | Self::Interpolation)
    }
}

/// One node, with everything the printer asks of it.
///
/// `'n` borrows the parse tree, `'a` the source it points into.
pub struct Info<'n, 'a> {
    pub kind: Kind,
    /// The element this node came from, for its name and attributes.
    pub element: Option<&'n Element<'a>>,
    /// The whole node, narrowed to the surviving text once the insensitive
    /// whitespace at its ends has been accounted for.
    pub span: Span,
    /// A text node's value, an interpolation's expression, a comment's inner
    /// text. Empty for everything else.
    pub value: &'a str,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Position among the parent's children, which is what makes `prev` and
    /// `next` constant-time.
    index_in_parent: usize,

    pub css_display: Display,
    pub white_space: WhiteSpace,
    pub has_leading_spaces: bool,
    pub has_trailing_spaces: bool,
    /// Set on a parent whose only content was whitespace.
    pub has_dangling_spaces: bool,
    pub is_whitespace_sensitive: bool,
    pub is_indentation_sensitive: bool,
    pub is_leading_space_sensitive: bool,
    pub is_trailing_space_sensitive: bool,
    pub is_dangling_space_sensitive: bool,
    pub is_self_closing: bool,
}

impl<'n, 'a> Info<'n, 'a> {
    fn new(kind: Kind, span: Span) -> Self {
        Self {
            kind,
            element: None,
            span,
            value: "",
            parent: None,
            children: Vec::new(),
            index_in_parent: 0,
            css_display: Display::default(),
            white_space: WhiteSpace::default(),
            has_leading_spaces: false,
            has_trailing_spaces: false,
            has_dangling_spaces: false,
            is_whitespace_sensitive: false,
            is_indentation_sensitive: false,
            is_leading_space_sensitive: false,
            is_trailing_space_sensitive: false,
            is_dangling_space_sensitive: false,
            is_self_closing: false,
        }
    }

    /// The element's name as the author wrote it. A Vue template's tag names
    /// are case-sensitive, so this is also the name every lookup uses.
    pub fn name(&self) -> &'a str {
        self.element.map_or("", |element| element.name)
    }

    pub fn attributes(&self) -> &'n [Attribute<'a>] {
        self.element.map_or(&[][..], |element| &element.attributes)
    }

    /// The value of an attribute that actually declares something.
    ///
    /// An empty value counts as absent, because Prettier reads these through
    /// JavaScript truthiness: `lang=""` is no `lang` at all, so a
    /// `<script lang="">` is an ordinary script rather than one in a language
    /// nothing here knows.
    pub fn declared_attribute_value(&self, name: &str) -> Option<&'a str> {
        self.attribute_value(name).filter(|value| !value.is_empty())
    }

    pub fn attribute_value(&self, name: &str) -> Option<&'a str> {
        self.attributes()
            .iter()
            .find(|attribute| attribute.name == name)
            .and_then(|attribute| attribute.value.as_ref())
            .map(|value| value.text)
    }
}

pub struct Tree<'n, 'a> {
    nodes: Vec<Info<'n, 'a>>,
    source: &'a str,
    /// Byte offset each line starts at, for the "was there a line break
    /// between these two nodes" questions the layout asks.
    line_starts: Vec<u32>,
    parser_is_vue_sfc: bool,
}

impl<'n, 'a> Tree<'n, 'a> {
    /// Build the tree for a whole `.vue` file, whose root children are its
    /// top-level blocks.
    pub fn build_sfc(
        root_nodes: &'n [Node<'a>],
        source: &'a str,
        options: &VueFormatOptions,
        allocator: &'a Allocator,
    ) -> Self {
        let mut tree = Self {
            nodes: vec![Info::new(Kind::Root, Span::new(0, offset(source.len())))],
            source,
            line_starts: line_starts(source),
            parser_is_vue_sfc: true,
        };
        let children = tree.add_nodes(root_nodes, ROOT);
        tree.nodes[ROOT].children = children;

        // Prettier's preprocess pipeline, in its order. Each step depends on
        // what the ones above it established.
        tree.remove_ignorable_first_lf();
        tree.extract_whitespaces();
        tree.add_css_display(options);
        tree.add_is_self_closing();
        tree.add_is_space_sensitive();
        tree.merge_simple_element_into_text(allocator);
        tree
    }

    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn node(&self, id: NodeId) -> &Info<'n, 'a> {
        &self.nodes[id]
    }

    pub fn prev(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id];
        let parent = node.parent?;
        let index = node.index_in_parent;
        (index > 0).then(|| self.nodes[parent].children[index - 1])
    }

    pub fn next(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id];
        let parent = node.parent?;
        self.nodes[parent].children.get(node.index_in_parent + 1).copied()
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].children.first().copied()
    }

    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].children.last().copied()
    }

    /// The deepest last child, which is what decides whether a closing tag
    /// can be borrowed. Prettier's `getLastDescendant`.
    pub fn last_descendant(&self, id: NodeId) -> NodeId {
        match self.last_child(id) {
            Some(child) => self.last_descendant(child),
            None => id,
        }
    }

    /// How deep the node sits, with the document itself at zero. Only the
    /// closing tag's "is the content already indented to here" test needs it.
    pub fn depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut current = self.nodes[id].parent;
        while let Some(parent) = current {
            depth += 1;
            current = self.nodes[parent].parent;
        }
        depth
    }

    /// The source line `offset` falls on, counting from zero. Only ever
    /// compared against another, never shown.
    pub fn line_of(&self, offset: u32) -> usize {
        self.line_starts.partition_point(|start| *start <= offset).saturating_sub(1)
    }

    pub fn text(&self, span: Span) -> &'a str {
        &self.source[span.start as usize..span.end as usize]
    }

    /// The span of an element's opening tag, `<` through `>`.
    pub fn start_span(&self, id: NodeId) -> Span {
        let node = &self.nodes[id];
        match node.element {
            Some(element) => Span::new(element.span.start, element.open_tag_end),
            None => node.span,
        }
    }

    /// The span of an element's closing tag, or `None` when it has none —
    /// a void element, one written self-closing, or one the parser had to
    /// recover because the author never closed it.
    ///
    /// The parser does not record where the closing tag starts, but it can be
    /// recovered exactly: the element ends immediately after that tag's `>`,
    /// and a closing tag contains no `<` of its own, so the last `<` in the
    /// element's span opens it. This holds even for a raw-text element whose
    /// body contains `<`, because the scan runs backwards from the end.
    pub fn end_span(&self, id: NodeId) -> Option<Span> {
        let node = &self.nodes[id];
        let element = node.element?;
        if element.unclosed {
            return None;
        }
        if element.self_closing || element.is_void {
            // Prettier gives a self-closing element the same span for both
            // ends, which is what makes `isSelfClosing` true below.
            return Some(self.start_span(id));
        }
        let body = self.text(element.span);
        let start = offset(body.rfind('<')?) + element.span.start;
        Some(Span::new(start, element.span.end))
    }

    // -----------------------------------------------------------------
    // Predicates the printer shares with the preprocess steps
    // -----------------------------------------------------------------

    /// Whether the element's body is a foreign language — JavaScript or CSS —
    /// that another formatter owns. Prettier's `isScriptLikeTag`.
    pub fn is_script_like(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        node.kind == Kind::Element && matches!(node.name(), "script" | "style")
    }

    /// A top-level block of the component. Every one of them is laid out as a
    /// block whatever its tag name would otherwise say.
    pub fn is_vue_sfc_block(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        self.parser_is_vue_sfc
            && node.kind == Kind::Element
            && node.parent == Some(ROOT)
            && !node.name().eq_ignore_ascii_case("html")
    }

    /// A top-level block that is none of `<template>`, `<script>`, `<style>` —
    /// an `<i18n>` or `<docs>` block, whose body is not the printer's to
    /// reshape.
    pub fn is_vue_custom_block(&self, id: NodeId) -> bool {
        self.is_vue_sfc_block(id)
            && !matches!(self.nodes[id].name(), "template" | "script" | "style")
    }

    /// A top-level block whose body is not HTML: a custom block, or one whose
    /// `lang` names another language.
    pub fn is_vue_non_html_block(&self, id: NodeId) -> bool {
        if !self.is_vue_sfc_block(id) {
            return false;
        }
        self.is_vue_custom_block(id)
            || self.nodes[id].declared_attribute_value("lang").is_some_and(|lang| lang != "html")
    }

    /// Whether the element renders its own whitespace, so the printer must
    /// leave its content exactly as written.
    pub fn is_pre_like(&self, id: NodeId) -> bool {
        self.nodes[id].white_space.is_pre_like()
    }

    /// Whether the node's content is kept byte for byte rather than printed.
    /// Prettier's `shouldPreserveContent`.
    pub fn should_preserve_content(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        // A `<pre>` holding anything but text and interpolations: the printer
        // has no way to lay out markup without changing what it renders.
        if self.is_pre_like(id)
            && node
                .children
                .iter()
                .any(|child| !matches!(self.nodes[*child].kind, Kind::Text | Kind::Interpolation))
        {
            return true;
        }
        if self.is_vue_non_html_block(id)
            && !self.is_script_like(id)
            && node.kind != Kind::Interpolation
        {
            return true;
        }
        false
    }

    /// Whether the node's own whitespace is content. Prettier's
    /// `isWhitespaceSensitiveNode`.
    fn is_whitespace_sensitive(&self, id: NodeId) -> bool {
        self.is_script_like(id)
            || self.nodes[id].kind == Kind::Interpolation
            || self.nodes[id].white_space.is_pre_like()
    }

    /// Whether a `prettier-ignore` comment sits immediately before the node,
    /// which hands the author back every byte of it.
    pub fn has_prettier_ignore(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        if node.parent.is_none() {
            return false;
        }
        self.prev(id).is_some_and(|prev| self.is_prettier_ignore(prev))
    }

    pub fn is_prettier_ignore(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        node.kind == Kind::Comment && node.value.trim() == "prettier-ignore"
    }

    // -----------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------

    fn add_nodes(&mut self, nodes: &'n [Node<'a>], parent: NodeId) -> Vec<NodeId> {
        let mut ids = Vec::with_capacity(nodes.len());
        for node in nodes {
            ids.push(self.add_node(node, parent, ids.len()));
        }
        ids
    }

    fn add_node(&mut self, node: &'n Node<'a>, parent: NodeId, index: usize) -> NodeId {
        let id = self.nodes.len();
        match node {
            Node::Element(element) => {
                let mut info = Info::new(Kind::Element, element.span);
                info.element = Some(element);
                info.white_space = element_white_space(element.name);
                self.push(info, parent, index);
                // A raw-text element's body is one text node the parser did
                // not look inside. Modelling it as a child is what lets the
                // same code path hand it to the formatter that owns it.
                //
                // An *empty* body is no child at all, not a child of nothing:
                // `<textarea></textarea>` has nothing between its tags, and a
                // zero-length text node would make it look as though it had
                // whitespace that renders.
                let children = match element.raw_text {
                    Some(span) if !span.is_empty() => {
                        let mut text = Info::new(Kind::Text, span);
                        text.value = self.text(span);
                        vec![self.push(text, id, 0)]
                    }
                    Some(_) => Vec::new(),
                    None => self.add_nodes(&element.children, id),
                };
                self.nodes[id].children = children;
            }
            Node::Text(text) => {
                let mut info = Info::new(Kind::Text, text.span);
                info.value = text.value;
                self.push(info, parent, index);
            }
            Node::Interpolation(interpolation) => {
                let info = Info::new(Kind::Interpolation, interpolation.span);
                self.push(info, parent, index);
                // Prettier models the expression as a text child, which is
                // what the embedded formatter is handed.
                let mut expression = Info::new(Kind::Text, interpolation.expression_span);
                expression.value = interpolation.expression;
                let child = self.push(expression, id, 0);
                self.nodes[id].children = vec![child];
            }
            Node::Comment(comment) => {
                let mut info = Info::new(Kind::Comment, comment.span);
                info.value = comment.content;
                self.push(info, parent, index);
            }
            Node::Raw(span) => {
                self.push(Info::new(Kind::Raw, *span), parent, index);
            }
        }
        id
    }

    fn push(&mut self, mut info: Info<'n, 'a>, parent: NodeId, index: usize) -> NodeId {
        info.parent = Some(parent);
        info.index_in_parent = index;
        self.nodes.push(info);
        self.nodes.len() - 1
    }

    /// Every node in the tree, parents before their children.
    fn walk(&self) -> Vec<NodeId> {
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![ROOT];
        while let Some(id) = stack.pop() {
            order.push(id);
            stack.extend(self.nodes[id].children.iter().rev().copied());
        }
        order
    }

    fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        let children = &mut self.nodes[parent].children;
        let Some(position) = children.iter().position(|id| *id == child) else { return };
        children.remove(position);
        let rest: Vec<NodeId> = self.nodes[parent].children[position..].to_vec();
        for (offset, id) in rest.into_iter().enumerate() {
            self.nodes[id].index_in_parent = position + offset;
        }
    }

    // -----------------------------------------------------------------
    // Preprocess steps, in Prettier's order
    // -----------------------------------------------------------------

    /// Drop the line break that follows `<pre>` and friends: it is part of the
    /// spelling, not of the content, and the parser is required to ignore it.
    fn remove_ignorable_first_lf(&mut self) {
        for id in self.walk() {
            let node = &self.nodes[id];
            if node.kind != Kind::Element || !ignores_first_line_feed(node.name()) {
                continue;
            }
            let Some(first) = self.first_child(id) else { continue };
            let first_node = &self.nodes[first];
            if first_node.kind != Kind::Text || !first_node.value.starts_with('\n') {
                continue;
            }
            if first_node.value.len() == 1 {
                self.remove_child(id, first);
            } else {
                let node = &mut self.nodes[first];
                node.value = &node.value[1..];
                node.span = Span::new(node.span.start + 1, node.span.end);
            }
        }
    }

    /// Record which whitespace is content and remove the rest.
    ///
    /// This is where a text node stops being "the bytes between two tags" and
    /// becomes "the words, plus a note on each neighbour that there was space
    /// here". Every later decision reads those notes rather than the source.
    fn extract_whitespaces(&mut self) {
        for id in self.walk() {
            if !self.nodes[id].kind.has_children_slot() {
                continue;
            }
            let children = self.nodes[id].children.clone();

            // Nothing but whitespace is not content: it is recorded on the
            // parent and dropped, so an "empty" element really is empty.
            let only_whitespace = children.len() == 1 && {
                let child = &self.nodes[children[0]];
                child.kind == Kind::Text && is_only_collapsible_whitespace(child.value)
            };
            if children.is_empty() || only_whitespace {
                self.nodes[id].has_dangling_spaces = !children.is_empty();
                self.nodes[id].children.clear();
                continue;
            }

            let whitespace_sensitive = self.is_whitespace_sensitive(id);
            let indentation_sensitive = self.nodes[id].white_space.is_pre_like();

            if !whitespace_sensitive {
                for child in children {
                    if self.nodes[child].kind != Kind::Text {
                        continue;
                    }
                    let value = self.nodes[child].value;
                    let leading = leading_whitespace(value);
                    let trailing = trailing_whitespace(&value[leading.len()..]);
                    let text = &value[leading.len()..value.len() - trailing.len()];
                    let prev = self.prev(child);
                    let next = self.next(child);

                    if text.is_empty() {
                        self.remove_child(id, child);
                        if !leading.is_empty() || !trailing.is_empty() {
                            if let Some(prev) = prev {
                                self.nodes[prev].has_trailing_spaces = true;
                            }
                            if let Some(next) = next {
                                self.nodes[next].has_leading_spaces = true;
                            }
                        }
                        continue;
                    }

                    let span = self.nodes[child].span;
                    self.nodes[child].value = text;
                    self.nodes[child].span = Span::new(
                        span.start + offset(leading.len()),
                        span.end - offset(trailing.len()),
                    );
                    if !leading.is_empty() {
                        if let Some(prev) = prev {
                            self.nodes[prev].has_trailing_spaces = true;
                        }
                        self.nodes[child].has_leading_spaces = true;
                    }
                    if !trailing.is_empty() {
                        self.nodes[child].has_trailing_spaces = true;
                        if let Some(next) = next {
                            self.nodes[next].has_leading_spaces = true;
                        }
                    }
                }
            }

            self.nodes[id].is_whitespace_sensitive = whitespace_sensitive;
            self.nodes[id].is_indentation_sensitive = indentation_sensitive;
        }
    }

    fn add_css_display(&mut self, options: &VueFormatOptions) {
        for id in self.walk() {
            self.nodes[id].css_display = self.css_display_of(id, options);
        }
    }

    fn css_display_of(&self, id: NodeId, options: &VueFormatOptions) -> Display {
        // A component's top-level blocks are blocks, whatever they are named.
        if self.is_vue_sfc_block(id) {
            return Display::Block;
        }
        // `<!-- display: block -->` in front of an element overrides the
        // stylesheet, which is how an author tells the printer that a
        // component renders as something other than the default.
        if let Some(prev) = self.prev(id) {
            let prev_node = &self.nodes[prev];
            if prev_node.kind == Kind::Comment
                && let Some(display) = parse_display_comment(prev_node.value)
            {
                return Display::from_comment_name(display);
            }
        }
        // Inside an `<svg>` the HTML stylesheet does not apply: SVG lays its
        // own elements out as blocks, so none of their surrounding whitespace
        // renders. This is decided before the sensitivity setting, so even
        // `strict` does not make an SVG's internals space-sensitive.
        if let Some(display) = self.svg_display(id) {
            return display;
        }
        if let Some(forced) = forced_display(options.whitespace_sensitivity) {
            return forced;
        }
        let node = &self.nodes[id];
        if node.kind == Kind::Element
            && let Some(display) = element_display(node.name())
        {
            return display;
        }
        Display::default()
    }

    /// The `display` an element gets for being in the SVG namespace, or
    /// `None` when it is not — including an HTML element inside a
    /// `<foreignObject>`, which is where HTML resumes and the stylesheet
    /// applies again.
    fn svg_display(&self, id: NodeId) -> Option<Display> {
        let node = &self.nodes[id];
        if node.kind != Kind::Element || !self.is_svg_namespaced(id) {
            return None;
        }
        // A nested `<svg>` inside a `<foreignObject>` is namespaced but not
        // laid out by this rule; Prettier falls back to the table for it.
        if self.has_ancestor_or_self_named(id, "foreignObject") {
            return None;
        }
        Some(if node.name() == "svg" { Display::InlineBlock } else { Display::Block })
    }

    /// Whether the element is in the SVG namespace: it is an `<svg>`, or it
    /// descends from one without passing through a `<foreignObject>`, whose
    /// children are HTML again.
    fn is_svg_namespaced(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = &self.nodes[node_id];
            if node.kind != Kind::Element {
                return false;
            }
            match node.name() {
                "svg" => return true,
                "foreignObject" if node_id != id => return false,
                _ => {}
            }
            current = node.parent;
        }
        false
    }

    fn has_ancestor_or_self_named(&self, id: NodeId, name: &str) -> bool {
        let mut current = Some(id);
        while let Some(node_id) = current {
            if self.nodes[node_id].name() == name {
                return true;
            }
            current = self.nodes[node_id].parent;
        }
        false
    }

    fn add_is_self_closing(&mut self) {
        for id in self.walk() {
            let node = &self.nodes[id];
            self.nodes[id].is_self_closing = if node.kind.has_children_slot() {
                node.element
                    .is_some_and(|element| is_void_element(element.name) || element.self_closing)
            } else {
                // Nothing that can hold children closes anything.
                true
            };
        }
    }

    /// Decide, for every node, whether the whitespace on either side of it is
    /// rendered — and therefore whether the printer may put a break there.
    ///
    /// The second loop is what makes the answer symmetric: a gap only shows if
    /// *both* sides of it are sensitive to it, so each node's flag is reduced
    /// by its neighbour's.
    fn add_is_space_sensitive(&mut self) {
        for id in self.walk() {
            if !self.nodes[id].kind.has_children_slot() {
                continue;
            }
            let children = self.nodes[id].children.clone();
            if children.is_empty() {
                self.nodes[id].is_dangling_space_sensitive =
                    self.nodes[id].css_display.is_edge_space_sensitive()
                        && !self.is_script_like(id);
                continue;
            }
            for child in &children {
                self.nodes[*child].is_leading_space_sensitive =
                    self.is_leading_space_sensitive(*child);
                self.nodes[*child].is_trailing_space_sensitive =
                    self.is_trailing_space_sensitive(*child);
            }
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    let prev = children[index - 1];
                    self.nodes[*child].is_leading_space_sensitive = self.nodes[prev]
                        .is_trailing_space_sensitive
                        && self.nodes[*child].is_leading_space_sensitive;
                }
                if index + 1 < children.len() {
                    let next = children[index + 1];
                    self.nodes[*child].is_trailing_space_sensitive = self.nodes[next]
                        .is_leading_space_sensitive
                        && self.nodes[*child].is_trailing_space_sensitive;
                }
            }
        }
    }

    fn is_leading_space_sensitive(&self, id: NodeId) -> bool {
        let sensitive = self.is_leading_space_sensitive_inner(id);
        // The first thing inside a `<pre>` is preceded by the newline the
        // parser dropped, so only an interpolation can still be sensitive.
        if sensitive
            && self.prev(id).is_none()
            && self.nodes[id]
                .parent
                .is_some_and(|parent| ignores_first_line_feed(self.nodes[parent].name()))
        {
            return self.nodes[id].kind == Kind::Interpolation;
        }
        sensitive
    }

    fn is_leading_space_sensitive_inner(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        let prev = self.prev(id);
        // Two runs of prose in a row: the gap between them is a word break.
        if matches!(node.kind, Kind::Text | Kind::Interpolation)
            && prev.is_some_and(|prev| {
                matches!(self.nodes[prev].kind, Kind::Text | Kind::Interpolation)
            })
        {
            return true;
        }
        let Some(parent) = node.parent else { return false };
        if self.nodes[parent].css_display == Display::None {
            return false;
        }
        if self.is_pre_like(parent) {
            return true;
        }
        if prev.is_none()
            && (self.nodes[parent].kind == Kind::Root
                || self.is_pre_like(id)
                || self.is_script_like(parent)
                || self.is_vue_custom_block(parent)
                || !self.nodes[parent].css_display.is_edge_space_sensitive())
        {
            return false;
        }
        if let Some(prev) = prev
            && !self.nodes[prev].css_display.is_between_space_sensitive()
        {
            return false;
        }
        true
    }

    fn is_trailing_space_sensitive(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        let next = self.next(id);
        if matches!(node.kind, Kind::Text | Kind::Interpolation)
            && next.is_some_and(|next| {
                matches!(self.nodes[next].kind, Kind::Text | Kind::Interpolation)
            })
        {
            return true;
        }
        let Some(parent) = node.parent else { return false };
        if self.nodes[parent].css_display == Display::None {
            return false;
        }
        if self.is_pre_like(parent) {
            return true;
        }
        if next.is_none()
            && (self.nodes[parent].kind == Kind::Root
                || self.is_pre_like(id)
                || self.is_script_like(parent)
                || self.is_vue_custom_block(parent)
                || !self.nodes[parent].css_display.is_edge_space_sensitive())
        {
            return false;
        }
        if let Some(next) = next
            && !self.nodes[next].css_display.is_between_space_sensitive()
        {
            return false;
        }
        true
    }

    /// Fold `foo<b>bar</b>baz` into one run of text.
    ///
    /// The element carries no attributes, holds one word, and touches text on
    /// both sides, so nothing about it can break — treating it as a word keeps
    /// the surrounding prose filling as prose instead of splitting it into
    /// three pieces around an unbreakable tag.
    fn merge_simple_element_into_text(&mut self, allocator: &'a Allocator) {
        for id in self.walk() {
            if !self.nodes[id].kind.has_children_slot() {
                continue;
            }
            let children = self.nodes[id].children.clone();
            for child in children {
                if !self.is_simple_element(child) {
                    continue;
                }
                let (Some(prev), Some(next)) = (self.prev(child), self.next(child)) else {
                    continue;
                };
                let inner = self.nodes[child].children[0];
                let name = self.nodes[child].name();
                let merged = format!(
                    "{}<{name}>{}</{name}>{}",
                    self.nodes[prev].value, self.nodes[inner].value, self.nodes[next].value
                );
                self.nodes[prev].value = allocator.alloc_str(&merged);
                self.nodes[prev].span =
                    Span::new(self.nodes[prev].span.start, self.nodes[next].span.end);
                self.nodes[prev].is_trailing_space_sensitive =
                    self.nodes[next].is_trailing_space_sensitive;
                self.nodes[prev].has_trailing_spaces = self.nodes[next].has_trailing_spaces;
                self.remove_child(id, child);
                self.remove_child(id, next);
            }
        }
    }

    fn is_simple_element(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        if node.kind != Kind::Element || !node.attributes().is_empty() || node.children.len() != 1 {
            return false;
        }
        let inner = &self.nodes[node.children[0]];
        if inner.kind != Kind::Text
            || has_collapsible_whitespace(inner.value)
            || inner.has_leading_spaces
            || inner.has_trailing_spaces
        {
            return false;
        }
        if !node.is_leading_space_sensitive
            || node.has_leading_spaces
            || !node.is_trailing_space_sensitive
            || node.has_trailing_spaces
        {
            return false;
        }
        self.prev(id).is_some_and(|prev| self.nodes[prev].kind == Kind::Text)
            && self.next(id).is_some_and(|next| self.nodes[next].kind == Kind::Text)
    }
}

/// The `display` a `<!-- display: … -->` comment names.
///
/// Deliberately only lowercase letters, no hyphen: this mirrors Prettier's
/// `/^\s*display:\s*([a-z]+)\s*$/`, under which `display: inline-block` is
/// *not* an override. Widening it here would make the fork lay out a template
/// differently from Prettier for a comment Prettier ignores.
fn parse_display_comment(value: &str) -> Option<&str> {
    let rest = value.trim_start().strip_prefix("display:")?;
    let name = rest.trim();
    (!name.is_empty() && name.bytes().all(|byte| byte.is_ascii_lowercase())).then_some(name)
}

/// A byte offset as the spans carry it. Source longer than 4 GiB is not a
/// case this toolchain supports anywhere, and the parser has already produced
/// `u32` spans over the same text.
#[expect(clippy::cast_possible_truncation)]
fn offset(value: usize) -> u32 {
    value as u32
}

fn line_starts(source: &str) -> Vec<u32> {
    let mut starts = vec![0];
    starts.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(index, _)| offset(index) + 1),
    );
    starts
}
