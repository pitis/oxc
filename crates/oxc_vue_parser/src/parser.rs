//! Template source → [`Node`] tree.
//!
//! Error-tolerant by construction: anything unrecognised becomes
//! [`Node::Raw`] and unclosed elements recover at their parent's boundary,
//! so malformed markup degrades to pass-through rather than an error.

use oxc_span::Span;

use crate::ast::{
    Attribute, AttributeValue, Comment, Directive, DirectiveArgument, DirectiveShorthand, Element,
    Interpolation, Node, Text, is_raw_text_element, is_void_element,
};

/// Parse Vue template source into a node tree.
///
/// `source` is the template contents (what sits between `<template>` and
/// `</template>`, or a whole `.vue` file when parsing top-level SFC
/// structure). `base_offset` is added to every span in the returned tree, so
/// spans are file-relative *by construction* rather than by convention:
///
/// - Parsing `source` standalone (it already starts at byte 0 of whatever
///   you consider "the file")? Pass `0`.
/// - Parsing a block's content extracted from a larger `.vue` file (e.g.
///   `SfcBlock::content`)? Pass `block.content_span.start` so the returned
///   spans point back into the original file, not into the substring.
///
/// This is a single `O(n)` pass over the produced tree after parsing — it
/// does not re-scan `source`, so there is no per-node or per-query cost
/// beyond the one-time shift.
pub fn parse_template(source: &str, base_offset: u32) -> Vec<Node<'_>> {
    let mut parser = Parser { source: source.as_bytes(), text: source, position: 0, depth: 0 };
    let mut nodes = parser.children(&Ancestors::Root);
    shift_nodes(&mut nodes, base_offset);
    nodes
}

/// Shift every span in `nodes` (recursively, including attributes and
/// directives) by `offset`. `u32` addition, no overflow checks: callers are
/// bound by the same "source length fits in `u32`" assumption the rest of
/// the crate already makes for span offsets.
fn shift_nodes(nodes: &mut [Node<'_>], offset: u32) {
    if offset == 0 {
        return;
    }
    for node in nodes {
        shift_node(node, offset);
    }
}

fn shift_span(span: Span, offset: u32) -> Span {
    Span::new(span.start + offset, span.end + offset)
}

fn shift_node(node: &mut Node<'_>, offset: u32) {
    match node {
        Node::Element(element) => {
            element.span = shift_span(element.span, offset);
            element.name_span = shift_span(element.name_span, offset);
            element.open_tag_end += offset;
            if let Some(raw_text) = &mut element.raw_text {
                *raw_text = shift_span(*raw_text, offset);
            }
            for attribute in &mut element.attributes {
                shift_attribute(attribute, offset);
            }
            shift_nodes(&mut element.children, offset);
        }
        Node::Text(text) => text.span = shift_span(text.span, offset),
        Node::Interpolation(interpolation) => {
            interpolation.span = shift_span(interpolation.span, offset);
            interpolation.expression_span = shift_span(interpolation.expression_span, offset);
        }
        Node::Comment(comment) => {
            comment.span = shift_span(comment.span, offset);
            comment.content_span = shift_span(comment.content_span, offset);
        }
        Node::Raw(span) => *span = shift_span(*span, offset),
    }
}

fn shift_attribute(attribute: &mut Attribute<'_>, offset: u32) {
    attribute.span = shift_span(attribute.span, offset);
    attribute.name_span = shift_span(attribute.name_span, offset);
    if let Some(value) = &mut attribute.value {
        value.span = shift_span(value.span, offset);
    }
    if let Some(directive) = &mut attribute.directive
        && let Some(argument) = &mut directive.argument
    {
        argument.span = shift_span(argument.span, offset);
    }
}

/// Recursion guard: beyond this depth elements are captured as raw text.
const MAX_DEPTH: u32 = 256;

/// The stack of names of currently-open ancestor elements, threaded through
/// the recursive descent without heap allocation: each frame borrows its
/// caller's, mirroring the parser's own call stack.
enum Ancestors<'p, 'a> {
    Root,
    Open { name: &'a str, parent: &'p Ancestors<'p, 'a> },
}

impl<'a> Ancestors<'_, 'a> {
    /// The name of the innermost open element, if any.
    fn name(&self) -> Option<&'a str> {
        match self {
            Ancestors::Root => None,
            Ancestors::Open { name, .. } => Some(name),
        }
    }
}

struct Parser<'a> {
    source: &'a [u8],
    text: &'a str,
    position: u32,
    depth: u32,
}

impl<'a> Parser<'a> {
    #[inline]
    fn at(&self, offset: u32) -> u8 {
        *self.source.get((self.position + offset) as usize).unwrap_or(&0)
    }

    #[inline]
    fn eof(&self) -> bool {
        self.position as usize >= self.source.len()
    }

    #[inline]
    fn len(&self) -> u32 {
        u32::try_from(self.source.len()).unwrap_or(u32::MAX)
    }

    fn starts_with(&self, pattern: &str) -> bool {
        self.source[(self.position as usize).min(self.source.len())..]
            .starts_with(pattern.as_bytes())
    }

    fn slice(&self, span: Span) -> &'a str {
        &self.text[span.start as usize..span.end as usize]
    }

    /// Parse children until a closing tag that belongs to `ancestors`, or end
    /// of input. The caller consumes the closing tag.
    fn children(&mut self, ancestors: &Ancestors<'_, 'a>) -> Vec<Node<'a>> {
        let mut nodes = Vec::new();
        if self.depth > MAX_DEPTH {
            let start = self.position;
            self.position = self.len();
            if start < self.position {
                nodes.push(Node::Raw(Span::new(start, self.position)));
            }
            return nodes;
        }
        loop {
            if self.eof() {
                return nodes;
            }
            let byte = self.at(0);

            if byte == b'<' {
                // HTML implied end tags: `<li>a<li>b` are siblings — a new
                // `<li>` closes the previous one. Leave the tag for the
                // grandparent loop.
                if let Some(parent) = ancestors.name()
                    && self.at(1) != b'/'
                    && self.at(1) != b'!'
                    && self.opening_tag_closes(parent)
                {
                    return nodes;
                }
                if self.at(1) == b'/' {
                    // A closing tag belongs to whoever opened it: if it
                    // matches this element or an outer ancestor, leave it
                    // unconsumed for that level to close — bubbling up marks
                    // intervening elements `unclosed` (this is what makes
                    // `<div><p></div>` recover correctly). If it matches no
                    // open ancestor at all (a browser would just drop it —
                    // e.g. `</br>` after a void `<br>`, `</span>` never
                    // opened, an orphan `</template>`), it doesn't close
                    // anything: consume it as raw markup and keep parsing
                    // siblings at this level so nothing here is lost.
                    if self.closing_tag_matches_any(ancestors) {
                        return nodes;
                    }
                    let start = self.position;
                    self.consume_closing_tag();
                    nodes.push(Node::Raw(Span::new(start, self.position)));
                    continue;
                }
                if self.starts_with("<!--") {
                    nodes.push(self.comment());
                    continue;
                }
                if self.at(1) == b'!' || self.at(1) == b'?' {
                    let start = self.position;
                    while !self.eof() && self.at(0) != b'>' {
                        self.position += 1;
                    }
                    self.position = (self.position + 1).min(self.len());
                    nodes.push(Node::Raw(Span::new(start, self.position)));
                    continue;
                }
                if let Some(node) = self.element(ancestors) {
                    nodes.push(node);
                    continue;
                }
                // A bare `<` that does not begin a tag: fall through as text.
            }

            if byte == b'{' && self.at(1) == b'{' {
                nodes.push(self.interpolation());
                continue;
            }

            // Plain text up to the next construct.
            let start = self.position;
            loop {
                if self.eof() {
                    break;
                }
                let current = self.at(0);
                if current == b'<' || (current == b'{' && self.at(1) == b'{') {
                    break;
                }
                self.position += 1;
            }
            if self.position == start {
                // Guarantee progress on a stray delimiter.
                self.position += 1;
            }
            let span = Span::new(start, self.position);
            nodes.push(Node::Text(Text { span, value: self.slice(span) }));
        }
    }

    fn comment(&mut self) -> Node<'a> {
        let start = self.position;
        self.position += 4; // `<!--`
        let content_start = self.position;
        while !self.eof() && !self.starts_with("-->") {
            self.position += 1;
        }
        let content_end = self.position;
        let unterminated = self.eof();
        self.position = (self.position + 3).min(self.len());
        let content_span = Span::new(content_start, content_end);
        Node::Comment(Comment {
            span: Span::new(start, self.position),
            content_span,
            content: self.slice(content_span),
            unterminated,
        })
    }

    fn interpolation(&mut self) -> Node<'a> {
        let start = self.position;
        self.position += 2; // `{{`
        let expression_start = self.position;
        while !self.eof() && (self.at(0) != b'}' || self.at(1) != b'}') {
            self.position += 1;
        }
        let expression_end = self.position;
        let unterminated = self.eof();
        self.position = (self.position + 2).min(self.len());
        let expression_span = Span::new(expression_start, expression_end);
        Node::Interpolation(Interpolation {
            span: Span::new(start, self.position),
            expression_span,
            expression: self.slice(expression_span),
            unterminated,
        })
    }

    fn element(&mut self, ancestors: &Ancestors<'_, 'a>) -> Option<Node<'a>> {
        let start = self.position;
        self.position += 1; // `<`
        let name_start = self.position;
        while !self.eof() {
            let current = self.at(0);
            if current.is_ascii_whitespace() || current == b'>' || current == b'/' {
                break;
            }
            self.position += 1;
        }
        let name_span = Span::new(name_start, self.position);
        if name_span.start == name_span.end {
            self.position = start;
            return None;
        }
        let name = self.slice(name_span);

        let attributes = self.attributes();
        let mut self_closing = false;
        if self.at(0) == b'/' {
            self_closing = true;
            self.position += 1;
        }
        if self.at(0) == b'>' {
            self.position += 1;
        }

        let is_void = is_void_element(name);
        if self_closing || is_void {
            return Some(Node::Element(Element {
                span: Span::new(start, self.position),
                name,
                name_span,
                attributes,
                children: Vec::new(),
                self_closing,
                is_void,
                raw_text: None,
                unclosed: false,
                open_tag_end: self.position,
            }));
        }

        if is_raw_text_element(name) {
            let body_start = self.position;
            while !self.eof() {
                if self.at(0) == b'<' && self.closing_tag_matches(name) {
                    break;
                }
                self.position += 1;
            }
            let raw_text = Span::new(body_start, self.position);
            let unclosed = self.eof();
            self.consume_closing_tag();
            return Some(Node::Element(Element {
                span: Span::new(start, self.position),
                name,
                name_span,
                attributes,
                children: Vec::new(),
                self_closing: false,
                is_void: false,
                raw_text: Some(raw_text),
                unclosed,
                open_tag_end: body_start,
            }));
        }

        let open_tag_end = self.position;
        self.depth += 1;
        let element_ancestors = Ancestors::Open { name, parent: ancestors };
        let children = self.children(&element_ancestors);
        self.depth -= 1;
        // Recover from a mismatched closing tag: only consume it when it
        // matches this element; otherwise the element is unclosed and the
        // tag is left for an ancestor.
        let unclosed =
            !(self.at(0) == b'<' && self.at(1) == b'/' && self.closing_tag_matches(name));
        if !unclosed {
            self.consume_closing_tag();
        }
        Some(Node::Element(Element {
            span: Span::new(start, self.position),
            name,
            name_span,
            attributes,
            children,
            self_closing: false,
            is_void: false,
            raw_text: None,
            unclosed,
            open_tag_end,
        }))
    }

    /// Whether the opening tag at the current `<` position implicitly closes
    /// an open `parent` element (HTML implied end tags).
    fn opening_tag_closes(&self, parent: &str) -> bool {
        let rest = &self.text[(self.position as usize + 1).min(self.text.len())..];
        let name_end = rest
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        implicitly_closes(parent, &rest[..name_end])
    }

    /// Whether the source at the current `<` position is `</name` (ASCII
    /// case-insensitive), followed by whitespace, `>`, `/`, or EOF.
    fn closing_tag_matches(&self, name: &str) -> bool {
        let rest = &self.source[(self.position as usize).min(self.source.len())..];
        let Some(rest) = rest.strip_prefix(b"</") else {
            return false;
        };
        if rest.len() < name.len() || !rest[..name.len()].eq_ignore_ascii_case(name.as_bytes()) {
            return false;
        }
        matches!(rest.get(name.len()), None | Some(b'>' | b'/'))
            || rest[name.len()].is_ascii_whitespace()
    }

    /// Whether the closing tag at the current position matches this element
    /// or any enclosing open ancestor.
    fn closing_tag_matches_any(&self, ancestors: &Ancestors) -> bool {
        match ancestors {
            Ancestors::Root => false,
            Ancestors::Open { name, parent } => {
                self.closing_tag_matches(name) || self.closing_tag_matches_any(parent)
            }
        }
    }

    fn consume_closing_tag(&mut self) {
        if self.at(0) == b'<' && self.at(1) == b'/' {
            while !self.eof() && self.at(0) != b'>' {
                self.position += 1;
            }
            self.position = (self.position + 1).min(self.len());
        }
    }

    fn attributes(&mut self) -> Vec<Attribute<'a>> {
        let mut attributes = Vec::new();
        loop {
            while !self.eof() && self.at(0).is_ascii_whitespace() {
                self.position += 1;
            }
            if self.eof() || self.at(0) == b'>' || (self.at(0) == b'/' && self.at(1) == b'>') {
                return attributes;
            }

            let name_start = self.position;
            while !self.eof() {
                let current = self.at(0);
                if current.is_ascii_whitespace()
                    || current == b'='
                    || current == b'>'
                    || (current == b'/' && self.at(1) == b'>')
                {
                    break;
                }
                self.position += 1;
            }
            let name_span = Span::new(name_start, self.position);
            if name_span.start == name_span.end {
                // Guarantee progress on a stray character.
                self.position += 1;
                continue;
            }
            let name = self.slice(name_span);

            let mut value = None;
            if self.at(0) == b'=' {
                self.position += 1;
                let quote = self.at(0);
                if quote == b'"' || quote == b'\'' {
                    self.position += 1;
                    let value_start = self.position;
                    while !self.eof() && self.at(0) != quote {
                        self.position += 1;
                    }
                    let span = Span::new(value_start, self.position);
                    let unterminated = self.eof();
                    value =
                        Some(AttributeValue { span, text: self.slice(span), quote, unterminated });
                    self.position = (self.position + 1).min(self.len());
                } else {
                    let value_start = self.position;
                    while !self.eof() && !self.at(0).is_ascii_whitespace() && self.at(0) != b'>' {
                        self.position += 1;
                    }
                    let span = Span::new(value_start, self.position);
                    value = Some(AttributeValue {
                        span,
                        text: self.slice(span),
                        quote: 0,
                        unterminated: false,
                    });
                }
            }

            let directive = parse_directive(name, name_span);
            attributes.push(Attribute {
                span: Span::new(name_span.start, self.position),
                name,
                name_span,
                value,
                directive,
            });
        }
    }
}

/// HTML implied end tags (WHATWG "generate implied end tags" subset):
/// an opening `next` tag ends an unclosed `open` element.
fn implicitly_closes(open: &str, next: &str) -> bool {
    const P_CLOSERS: &[&str] = &[
        "address",
        "article",
        "aside",
        "blockquote",
        "details",
        "div",
        "dl",
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
        "main",
        "menu",
        "nav",
        "ol",
        "p",
        "pre",
        "section",
        "table",
        "ul",
    ];
    let eq = |candidate: &str| next.eq_ignore_ascii_case(candidate);
    let open_is = |candidate: &str| open.eq_ignore_ascii_case(candidate);
    if open_is("li") {
        eq("li")
    } else if open_is("dt") || open_is("dd") {
        eq("dt") || eq("dd")
    } else if open_is("option") {
        eq("option") || eq("optgroup")
    } else if open_is("optgroup") {
        eq("optgroup")
    } else if open_is("tr") {
        eq("tr")
    } else if open_is("td") || open_is("th") {
        eq("td") || eq("th") || eq("tr")
    } else if open_is("thead") || open_is("tbody") {
        eq("tbody") || eq("tfoot")
    } else if open_is("rt") || open_is("rp") {
        eq("rt") || eq("rp")
    } else if open_is("p") {
        P_CLOSERS.iter().any(|closer| eq(closer))
    } else {
        false
    }
}

/// Decompose a raw attribute name into its directive parts, if it is one.
fn parse_directive(name: &str, name_span: Span) -> Option<Directive<'_>> {
    let (directive_name, shorthand, rest, rest_offset) = match name.as_bytes().first()? {
        b':' => ("bind", Some(DirectiveShorthand::Bind), &name[1..], 1u32),
        b'.' => ("bind", Some(DirectiveShorthand::BindProp), &name[1..], 1),
        b'@' => ("on", Some(DirectiveShorthand::On), &name[1..], 1),
        b'#' => ("slot", Some(DirectiveShorthand::Slot), &name[1..], 1),
        _ => {
            let rest = name.strip_prefix("v-")?;
            // `v-if`, `v-else-if`, `v-on:click`, `v-bind:x`, `v-my-directive`…
            // The directive name runs to the first `:` or `.`.
            let name_end = rest.find([':', '.']).unwrap_or(rest.len());
            let directive_name = &rest[..name_end];
            let after = &rest[name_end..];
            let (after, offset) = if let Some(stripped) = after.strip_prefix(':') {
                (stripped, 2 + u32::try_from(name_end).unwrap_or(u32::MAX).saturating_add(1))
            } else {
                (after, 2 + u32::try_from(name_end).unwrap_or(u32::MAX))
            };
            (directive_name, None, after, offset)
        }
    };

    // `rest` is `argument.mod1.mod2` (argument may be empty, e.g. `v-if`,
    // or bracketed for dynamic arguments: `[key].mod`).
    let mut modifiers = Vec::new();
    let argument_len = if rest.starts_with('[') {
        rest.find(']').map_or(rest.len(), |index| index + 1)
    } else {
        rest.find('.').unwrap_or(rest.len())
    };
    let (argument_text, modifier_text) = rest.split_at(argument_len);
    for modifier in modifier_text.split('.') {
        if !modifier.is_empty() {
            modifiers.push(modifier);
        }
    }
    let argument = if argument_text.is_empty() {
        None
    } else {
        let argument_start = name_span.start + rest_offset;
        Some(DirectiveArgument {
            span: Span::new(
                argument_start,
                argument_start + u32::try_from(argument_text.len()).unwrap_or(u32::MAX),
            ),
            text: argument_text,
            dynamic: argument_text.starts_with('['),
        })
    };

    // `.prop` shorthand implies the `prop` modifier in Vue's model; keep the
    // written modifiers only — consumers can special-case `BindProp`.
    Some(Directive { name: directive_name, argument, modifiers, shorthand })
}
