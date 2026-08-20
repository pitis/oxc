//! Printing text between tags.
//!
//! HTML collapses a run of whitespace to a single space, so text is printed
//! as words joined by `line`s the printer may break at, with the exception
//! that line breaks the author wrote are kept — one, or two for a blank
//! line, and no more.

use oxc_formatter_core::{
    Buffer, Format,
    builders::{empty_line, hard_line_break, soft_line_break_or_space, text},
    write,
};

use super::{
    SvelteFormatter,
    children::Trim,
    classify::{
        ends_with_line_breaks, is_collapsible_whitespace, starts_with_line_breaks, trimmed,
    },
};

/// One element of the flat sequence a text run becomes.
#[derive(Debug, Clone, Copy)]
enum Piece<'a> {
    Word(&'a str),
    /// A break the printer may take, rendered as a space when it does not.
    Line,
    /// A break the author wrote, which is always taken.
    Hardline,
    /// Two breaks the author wrote: a blank line. One element rather than two
    /// `Hardline`s, because the printer only starts a new line once per line
    /// of output and two in a row would collapse into one.
    EmptyLine,
}

/// Print a run of text, with whatever whitespace the surrounding layout has
/// already claimed removed from its ends.
pub fn write_text<'a>(value: &'a str, trim: Trim, f: &mut SvelteFormatter<'_, 'a>) {
    let value = trimmed(value, trim.left, trim.right);
    if value.is_empty() {
        return;
    }
    if is_only_whitespace(value) {
        write_whitespace(value, f);
        return;
    }

    let pieces = split_into_pieces(value);
    // Prettier hands the whole flat sequence to `fill`, breaks at the edges
    // included, and the fill measures each item on its own — so an edge break
    // renders as a space when what follows it fits, and as a line when it
    // does not. Pulling those breaks out of the fill would make them the
    // enclosing group's, which breaks them whenever anything else does.
    let nothing = FormatPiece(Piece::Word(""));
    let mut filler = f.fill();
    let mut pieces = pieces.into_iter();
    if let Some(first) = pieces.next() {
        filler.entry(&nothing, &FormatPiece(first));
    }
    while let Some(separator) = pieces.next() {
        match pieces.next() {
            Some(content) => filler.entry(&FormatPiece(separator), &FormatPiece(content)),
            None => filler.entry(&nothing, &FormatPiece(separator)),
        };
    }
    filler.finish();
}

/// Print a text node that is nothing but whitespace: what the author wrote
/// decides whether it becomes a break, a blank line, or a plain space.
pub fn write_whitespace(value: &str, f: &mut SvelteFormatter<'_, '_>) {
    if has_blank_line(value) {
        write!(f, empty_line());
    } else if value.contains('\n') {
        write!(f, hard_line_break());
    } else if !value.is_empty() {
        write!(f, soft_line_break_or_space());
    }
}

struct FormatPiece<'a>(Piece<'a>);

impl<'a> Format<'a, crate::context::SvelteFormatContext<'a>> for FormatPiece<'a> {
    fn fmt(&self, f: &mut SvelteFormatter<'_, 'a>) {
        match self.0 {
            Piece::Word(word) => {
                if !word.is_empty() {
                    write!(f, text(word));
                }
            }
            Piece::Line => write!(f, soft_line_break_or_space()),
            Piece::Hardline => write!(f, hard_line_break()),
            Piece::EmptyLine => write!(f, empty_line()),
        }
    }
}

/// Split a text run into words and the breaks between them, promoting the
/// author's own line breaks at either end to hard ones.
fn split_into_pieces(value: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    for (index, word) in split_on_whitespace_runs(value).enumerate() {
        if index > 0 {
            pieces.push(Piece::Line);
        }
        if !word.is_empty() {
            pieces.push(Piece::Word(word));
        }
    }

    if starts_with_line_breaks(value, 1)
        && let Some(first) = pieces.first_mut()
    {
        *first = if starts_with_line_breaks(value, 2) { Piece::EmptyLine } else { Piece::Hardline };
    }
    if ends_with_line_breaks(value, 1)
        && let Some(last) = pieces.last_mut()
    {
        *last = if ends_with_line_breaks(value, 2) { Piece::EmptyLine } else { Piece::Hardline };
    }
    pieces
}

/// The words of a text run, split on whole *runs* of whitespace — one break
/// between two words however much whitespace the author left there.
///
/// A run at either end yields an empty word on that side, which is what marks
/// the text as starting or ending with a break.
fn split_on_whitespace_runs(value: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(value);
    std::iter::from_fn(move || {
        let current = rest?;
        let Some(start) = current.find(is_whitespace_char) else {
            rest = None;
            return Some(current);
        };
        let end = current[start..]
            .find(|c: char| !is_whitespace_char(c))
            .map_or(current.len(), |offset| start + offset);
        rest = Some(&current[end..]);
        Some(&current[..start])
    })
}

fn is_whitespace_char(c: char) -> bool {
    u8::try_from(c).is_ok_and(is_collapsible_whitespace)
}

fn is_only_whitespace(value: &str) -> bool {
    value.chars().all(is_whitespace_char)
}

/// Whether a whitespace-only run spans a blank line, which is preserved as
/// one blank line however many the author wrote.
fn has_blank_line(value: &str) -> bool {
    value.matches('\n').count() >= 2
}
