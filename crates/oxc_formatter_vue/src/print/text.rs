//! Printing a run of text.
//!
//! Outside a `<pre>`, HTML collapses every run of whitespace to one space, so
//! the printer is free to put the break wherever the line runs out: the text
//! becomes words separated by breaks, and a `fill` packs as many onto each
//! line as will fit. Inside a `<pre>` the opposite holds — every byte renders,
//! so the line breaks the author wrote are the only ones there may be.

use oxc_formatter_core::{
    Buffer, Format,
    builders::{hard_line_break, literal_line_break, soft_line_break_or_space, text},
    write,
};

use crate::context::VueFormatContext;

use super::{
    VueFormatter,
    classify::is_collapsible_whitespace,
    tag::{closing_tag_suffix, opening_tag_prefix},
    tree::{NodeId, Tree},
};

/// One element of the flat sequence a text run becomes. A `fill` alternates
/// content and separator, so the two are one type.
#[derive(Clone, Copy)]
enum Piece<'a> {
    Word(&'a str),
    /// A break the printer may take, rendered as a space when it does not.
    Line,
    /// A break that is always taken, and starts a new line of output.
    HardLine,
    /// A break that is always taken and reproduces the source's own line,
    /// without the indentation the enclosing layout would add.
    LiteralLine,
}

/// Print a text node, together with whatever its neighbours borrowed from it.
pub fn write_text<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let node = tree.node(id);
    let parent = node.parent.expect("a text node always has a parent");
    let parent_node = tree.node(parent);

    let pieces = if parent_node.is_whitespace_sensitive {
        if parent_node.is_indentation_sensitive {
            // Inside a `<pre>`: the indentation is content too, so the lines
            // are reproduced exactly where the source put them.
            split_lines(node.value, 0, Piece::LiteralLine)
        } else {
            let value = trim_preserving_indentation(node.value);
            split_lines(value, min_indentation(value), Piece::HardLine)
        }
    } else {
        split_words(node.value)
    };

    let prefix = opening_tag_prefix(tree, id);
    let suffix = closing_tag_suffix(tree, id);
    // The borrowed delimiters ride along with the first and last piece rather
    // than becoming entries of their own: a `fill`'s odd entries must be the
    // breaks, and a delimiter is not one.
    if pieces.is_empty() {
        return;
    }
    let last = pieces.len() - 1;
    let entry = |index: usize| Entry {
        prefix: (index == 0).then_some(prefix),
        piece: pieces[index],
        suffix: (index == last).then_some(suffix),
    };

    let mut filler = f.fill();
    filler.entry(&Empty, &entry(0));
    let mut index = 1;
    while index <= last {
        if index < last {
            filler.entry(&entry(index), &entry(index + 1));
            index += 2;
        } else {
            filler.entry(&Empty, &entry(index));
            index += 1;
        }
    }
    filler.finish();
}

/// The words of a text run, separated by breaks.
fn split_words(value: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    for (index, word) in value.split(is_whitespace_char).filter(|word| !word.is_empty()).enumerate()
    {
        if index > 0 {
            pieces.push(Piece::Line);
        }
        pieces.push(Piece::Word(word));
    }
    pieces
}

/// The lines of a text run, separated by breaks that are always taken, with
/// `indent` bytes of shared indentation removed from each.
///
/// Dedenting by re-slicing each line is what keeps every piece a borrow of
/// the source: the indentation that comes off belongs to the surrounding
/// markup, and what is left is already contiguous.
fn split_lines<'a>(value: &'a str, indent: usize, separator: Piece<'static>) -> Vec<Piece<'a>> {
    let mut pieces = Vec::new();
    for (index, line) in value.split('\n').enumerate() {
        if index > 0 {
            pieces.push(separator);
        }
        pieces.push(Piece::Word(&line[indent.min(line.len())..]));
    }
    pieces
}

fn is_whitespace_char(c: char) -> bool {
    u8::try_from(c).is_ok_and(is_collapsible_whitespace)
}

/// Drop the trailing whitespace and the one blank line the author may have
/// left at the start, keeping the indentation of everything else.
fn trim_preserving_indentation(value: &str) -> &str {
    let end = value
        .bytes()
        .rposition(|byte| !is_collapsible_whitespace(byte))
        .map_or(0, |index| index + 1);
    let value = &value[..end];
    // Only the first such line, matching Prettier's non-multiline anchor.
    let leading = value.bytes().position(|byte| !matches!(byte, b'\t' | 0x0c | b'\r' | b' '));
    match leading {
        Some(index) if value.as_bytes()[index] == b'\n' => &value[index + 1..],
        _ => value,
    }
}

/// The indentation shared by every non-blank line, which belongs to the
/// surrounding markup rather than to the text.
fn min_indentation(value: &str) -> usize {
    let mut minimum = usize::MAX;
    for line in value.split('\n') {
        if line.is_empty() {
            continue;
        }
        let indent =
            line.bytes().position(|byte| !is_collapsible_whitespace(byte)).unwrap_or(line.len());
        if indent == 0 {
            return 0;
        }
        if indent == line.len() {
            continue;
        }
        minimum = minimum.min(indent);
    }
    if minimum == usize::MAX { 0 } else { minimum }
}

struct Empty;

impl<'a> Format<'a, VueFormatContext<'a>> for Empty {
    fn fmt(&self, _: &mut VueFormatter<'_, 'a>) {}
}

/// One `fill` entry: a piece, plus any delimiter that rides on it.
struct Entry<'a> {
    prefix: Option<super::tag::Marker<'a>>,
    piece: Piece<'a>,
    suffix: Option<super::tag::Marker<'a>>,
}

impl<'a> Format<'a, VueFormatContext<'a>> for Entry<'a> {
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        if let Some(prefix) = self.prefix {
            write!(f, prefix);
        }
        match self.piece {
            Piece::Word(word) => {
                if !word.is_empty() {
                    write!(f, text(word));
                }
            }
            Piece::Line => write!(f, soft_line_break_or_space()),
            Piece::HardLine => write!(f, hard_line_break()),
            Piece::LiteralLine => write!(f, literal_line_break()),
        }
        if let Some(suffix) = self.suffix {
            write!(f, suffix);
        }
    }
}
