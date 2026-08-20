//! Printing an element: its open tag, its content, and how the two meet.
//!
//! The shape of a tag follows from whether its content "hugs" the tag —
//! whether there is whitespace between `>` and the first child, and between
//! the last child and `</`. Where there is none, inserting a break would add
//! a space the page would render, so the printer must not.

use oxc_formatter_core::{
    Buffer, Format,
    builders::{
        dedent, group, hard_line_break, indent, soft_line_break, soft_line_break_or_space, text,
        token,
    },
    write,
};
use svelte_markup_parser::ast::{Element, Node};

use super::{
    SvelteFormatter,
    attribute::write_attribute,
    children::Trim,
    classify::{
        ends_with_collapsible_whitespace, is_block_tag, is_empty, is_pre_tag, is_raw_text_element,
        starts_with_collapsible_whitespace, starts_with_line_breaks,
    },
    raw_text::write_raw_text_element,
    write_children, write_source,
};

pub fn write_element<'a>(element: &Element<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    let source = f.context().source_text().as_str();
    let options = *f.options();
    let name = element.name;
    let children = &element.children;
    let empty = is_empty(children);

    // `<pre>` and `<textarea>` render their own whitespace, so their content
    // is not the printer's to lay out.
    if is_pre_tag(element) {
        write_source(element.span, f);
        return;
    }
    // A `<script>` or `<style>` body is another language; it goes to the
    // formatter that owns it.
    if is_raw_text_element(element) {
        write_raw_text_element(element, f);
        return;
    }

    let self_closing = empty && (element.self_closing || element.is_void);
    let attributes: Vec<&_> = element.attributes.iter().collect();
    let allow_shorthand = options.allow_shorthand.is_enabled();

    if self_closing {
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, [token("<"), text(name)]);
                write!(
                    f,
                    indent(&group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                        for attribute in &attributes {
                            write!(f, soft_line_break_or_space());
                            write_attribute(attribute, source, allow_shorthand, f);
                        }
                        write!(f, dedent(&soft_line_break_or_space()));
                    })))
                );
                // Every self-closing tag is normalized to `<x />`, including a
                // void element the author wrote as `<br>`.
                write!(f, token("/>"));
            }))
        );
        return;
    }

    let hug_start = should_hug_start(element);
    let hug_end = should_hug_end(element);

    // The element's own layout owns the whitespace at the very start and end
    // of its content, so the first and last child must not print it again.
    let mut trims = vec![Trim::default(); children.len()];
    let mut separator_start = Separator::Soft;
    let mut separator_end = Separator::Soft;
    if !empty {
        let first = children.first();
        let last = children.last();
        let mut set_end = false;
        if !hug_start
            && let Some((index, Node::Text(text))) = children.iter().enumerate().next()
            && matches!(first, Some(Node::Text(_)))
        {
            if starts_with_line_breaks(text.value, 1)
                && children.len() > 1
                && (is_block_tag(element)
                    || matches!(last, Some(Node::Text(last)) if ends_with_collapsible_whitespace(last.value)))
            {
                separator_start = Separator::Hard;
                separator_end = Separator::Hard;
                set_end = true;
            } else if !is_block_tag(element) {
                separator_start = Separator::Line;
            }
            trims[index].left = true;
        }
        if !hug_end && let Some(Node::Text(_)) = last {
            if !is_block_tag(element) && !set_end {
                separator_end = Separator::Line;
            }
            let last_index = children.len() - 1;
            trims[last_index].right = true;
        }
    }

    let open_tag = format_with(move |f: &mut SvelteFormatter<'_, 'a>| {
        write!(f, [token("<"), text(name)]);
        write!(
            f,
            indent(&group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                for attribute in &attributes {
                    write!(f, soft_line_break_or_space());
                    write_attribute(attribute, source, allow_shorthand, f);
                }
                if !hug_start || empty {
                    write!(f, dedent(&soft_line_break()));
                }
            })))
        );
    });

    let body = format_with(|f: &mut SvelteFormatter<'_, 'a>| {
        if empty {
            return;
        }
        write_children(children, &trims, f);
    });

    if empty {
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, [&open_tag, token(">")]);
                write_close_tag(name, f);
            }))
        );
        return;
    }

    if hug_start && hug_end {
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, &open_tag);
                write!(
                    f,
                    indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                        write!(f, soft_line_break());
                        write!(
                            f,
                            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                                write!(f, [token(">"), &body, token("</"), text(name)]);
                            }))
                        );
                    }))
                );
                write!(f, [soft_line_break(), token(">")]);
            }))
        );
        return;
    }

    if hug_start {
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, &open_tag);
                write!(
                    f,
                    indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                        write!(f, soft_line_break());
                        write!(
                            f,
                            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                                write!(f, [token(">"), &body]);
                            }))
                        );
                    }))
                );
                separator_end.write(f);
                write_close_tag(name, f);
            }))
        );
        return;
    }

    if hug_end {
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, [&open_tag, token(">")]);
                write!(
                    f,
                    indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                        separator_start.write(f);
                        write!(
                            f,
                            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                                write!(f, [&body, token("</"), text(name)]);
                            }))
                        );
                    }))
                );
                write!(f, [soft_line_break(), token(">")]);
            }))
        );
        return;
    }

    write!(
        f,
        group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
            write!(f, [&open_tag, token(">")]);
            write!(
                f,
                indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                    separator_start.write(f);
                    write!(f, &body);
                }))
            );
            separator_end.write(f);
            write_close_tag(name, f);
        }))
    );
}

fn write_close_tag<'a>(name: &'a str, f: &mut SvelteFormatter<'_, 'a>) {
    write!(f, [token("</"), text(name), token(">")]);
}

/// What goes between a tag and its content when the content does not hug it.
#[derive(Clone, Copy)]
enum Separator {
    Soft,
    Line,
    Hard,
}

impl Separator {
    fn write(self, f: &mut SvelteFormatter<'_, '_>) {
        match self {
            Self::Soft => write!(f, soft_line_break()),
            Self::Line => write!(f, soft_line_break_or_space()),
            Self::Hard => write!(f, hard_line_break()),
        }
    }
}

/// Whether the open tag hugs its first child: there is no whitespace after
/// `>`, so a break there would add a space that renders.
fn should_hug_start(element: &Element<'_>) -> bool {
    if is_block_tag(element) {
        return false;
    }
    let Some(first) = element.children.first() else { return true };
    !matches!(first, Node::Text(text) if starts_with_collapsible_whitespace(text.value))
}

/// The mirror of [`should_hug_start`], for the closing tag.
fn should_hug_end(element: &Element<'_>) -> bool {
    if is_block_tag(element) {
        return false;
    }
    let Some(last) = element.children.last() else { return true };
    !matches!(last, Node::Text(text) if ends_with_collapsible_whitespace(text.value))
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
