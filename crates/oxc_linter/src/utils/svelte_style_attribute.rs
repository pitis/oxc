//! Parsing the value of a Svelte `style="…"` attribute.
//!
//! Stands in for eslint-plugin-svelte's `css-utils` (a PostCSS parse of the
//! attribute value that tracks `{expr}` interpolations). This is a
//! declaration-level scanner rather than a full CSS parser: enough to read
//! property names, values, `!important`, comments, and which parts came from
//! an interpolation.

use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeValue, ValuePart};

/// One `prop: value` declaration of a `style` attribute.
#[derive(Debug)]
pub struct SvelteStyleDecl<'a> {
    /// The property name, exactly as written.
    pub property: &'a str,
    pub property_span: Span,
    /// The text after the colon, `!important` included.
    pub value_span: Span,
    /// Whether the declaration ends in `!important`.
    pub important: bool,
}

/// One `;`-separated piece of a `style` attribute value.
#[derive(Debug)]
pub enum SvelteStyleSegment<'a> {
    Decl(SvelteStyleDecl<'a>),
    /// A `/* … */` comment.
    Comment(Span),
    /// A segment that is not a statically readable declaration: no colon, a
    /// property name that is (or contains) an interpolation, or a bare
    /// `{expr}` standing in for whole declarations.
    Unknown(Span),
}

impl<'a> SvelteStyleSegment<'a> {
    pub fn as_decl(&self) -> Option<&SvelteStyleDecl<'a>> {
        match self {
            Self::Decl(decl) => Some(decl),
            _ => None,
        }
    }
}

/// Split a `style` attribute value into its segments.
///
/// `{expr}` parts are masked byte-for-byte before splitting, so an
/// interpolation can never introduce a phantom `;`, `:` or quote, while every
/// span still points into the real source.
#[expect(clippy::cast_possible_truncation)] // offsets into a source file, capped at `u32` by `Span`
pub fn parse_svelte_style_attribute<'a>(
    value: &AttributeValue<'a>,
    source: &'a str,
) -> Vec<SvelteStyleSegment<'a>> {
    let value_start = value.span.start as usize;
    let value_end = (value.span.end as usize).min(source.len());
    if value_start >= value_end {
        return Vec::new();
    }
    let raw = &source[value_start..value_end];

    let mut masked = raw.as_bytes().to_vec();
    let mut interpolations: Vec<(usize, usize)> = Vec::new();
    for part in &value.parts {
        if let ValuePart::Expression(expression) = part {
            let start =
                (expression.span.start as usize).saturating_sub(value_start).min(masked.len());
            let end = (expression.span.end as usize).saturating_sub(value_start).min(masked.len());
            masked[start..end].fill(b'0');
            interpolations.push((start, end));
        }
    }

    let overlaps_interpolation = |start: usize, end: usize| {
        interpolations.iter().any(|&(from, to)| from < end && to > start)
    };
    let span_of = |start: usize, end: usize| {
        Span::new((value_start + start) as u32, (value_start + end) as u32)
    };

    let mut segments = Vec::new();
    for (start, end) in split_declarations(&masked) {
        // Trim the segment.
        let mut from = start;
        let mut to = end;
        while from < to && masked[from].is_ascii_whitespace() {
            from += 1;
        }
        while to > from && masked[to - 1].is_ascii_whitespace() {
            to -= 1;
        }
        if from == to {
            continue;
        }
        let span = span_of(from, to);

        if masked[from..to].starts_with(b"/*") {
            segments.push(SvelteStyleSegment::Comment(span));
            continue;
        }
        let Some(colon) = masked[from..to].iter().position(|&byte| byte == b':') else {
            segments.push(SvelteStyleSegment::Unknown(span));
            continue;
        };

        let property_limit = from + colon;
        let mut property_end = property_limit;
        while property_end > from && masked[property_end - 1].is_ascii_whitespace() {
            property_end -= 1;
        }
        if from == property_end || overlaps_interpolation(from, property_end) {
            segments.push(SvelteStyleSegment::Unknown(span));
            continue;
        }

        let mut value_from = property_limit + 1;
        while value_from < to && masked[value_from].is_ascii_whitespace() {
            value_from += 1;
        }
        let important = masked[value_from..to]
            .windows(b"!important".len())
            .any(|window| window.eq_ignore_ascii_case(b"!important"));

        segments.push(SvelteStyleSegment::Decl(SvelteStyleDecl {
            property: &raw[from..property_end],
            property_span: span_of(from, property_end),
            value_span: span_of(value_from, to),
            important,
        }));
    }
    segments
}

/// `;`-separated ranges, ignoring separators inside quotes, parentheses and
/// comments (`content: "a;b"`, `background: url(a;b)`, `/* a;b */`).
fn split_declarations(masked: &[u8]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0u32;
    let mut quote: Option<u8> = None;
    let mut index = 0;
    while index < masked.len() {
        let byte = masked[index];
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
        } else {
            if byte == b'/' && masked.get(index + 1) == Some(&b'*') {
                // Skip to the end of the comment so a `;` inside it does not
                // split the declaration.
                let mut end = index + 2;
                while end + 1 < masked.len() && !(masked[end] == b'*' && masked[end + 1] == b'/') {
                    end += 1;
                }
                index = (end + 2).min(masked.len());
                continue;
            }
            match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b';' if paren_depth == 0 => {
                    ranges.push((start, index));
                    start = index + 1;
                }
                _ => {}
            }
        }
        index += 1;
    }
    ranges.push((start, masked.len()));
    ranges
}
