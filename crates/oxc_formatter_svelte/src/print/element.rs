//! Printing an element: its open tag, its content, and how the two meet.
//!
//! The shape of a tag follows from whether its content "hugs" the tag —
//! whether there is whitespace between `>` and the first child, and between
//! the last child and `</`. Where there is none, inserting a break would add
//! a space the page would render, so the printer must not.

use oxc_formatter_core::{
    Buffer,
    builders::{
        dedent, group, hard_line_break, indent, soft_line_break, soft_line_break_or_space, space,
        text, token,
    },
    write,
};
use svelte_markup_parser::ast::{Element, Node};

use crate::options::WhitespaceSensitivity;

use super::{
    SvelteFormatter,
    attribute::{AttributeContext, write_attribute},
    children::{ChildLayout, Trim},
    classify::{
        ends_with_collapsible_whitespace, is_block_tag, is_boundary, is_collapsible_whitespace,
        is_empty, is_inline_tag, is_pre_content, is_raw_text_element, is_regular_element,
        starts_with_collapsible_whitespace, starts_with_line_breaks,
    },
    format_with,
    raw_text::write_raw_text_element,
    write_children, write_source,
};

pub fn write_element<'a>(
    element: &Element<'a>,
    last_in_block_parent: bool,
    in_pre: bool,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    let source = f.context().source_text().as_str();
    let options = *f.options();
    let name = element.name;
    let children = &element.children;
    let empty = is_empty(children);

    // A `<script>` or `<style>` body is another language; it goes to the
    // formatter that owns it.
    if is_raw_text_element(element) {
        write_raw_text_element(element, f);
        return;
    }

    let self_closing = empty && (element.self_closing || element.is_void);
    let attributes: Vec<&_> = element.attributes.iter().collect();
    let attribute_context = AttributeContext {
        allow_shorthand: options.allow_shorthand.is_enabled(),
        regular_element: is_regular_element(element),
    };
    // `<pre>` and `<textarea>` render their own whitespace, so their content
    // is not the printer's to lay out. The tag around it still is.
    // Being inside one counts: a `<span>` in a `<pre>` shows its own
    // whitespace too, however deeply it is nested.
    let pre = in_pre || is_pre_content(element);
    let sensitivity = options.whitespace_sensitivity;
    let bracket_same_line = options.bracket_same_line.is_enabled();
    let block = is_block_tag(element, sensitivity);
    // Inside one of those, nothing is an inline element: there is no layout
    // decision left to make about the whitespace.
    let inline = is_inline_tag(element, sensitivity) && !pre;

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
                            write_attribute(attribute, source, attribute_context, f);
                        }
                        if !bracket_same_line {
                            write!(f, dedent(&soft_line_break_or_space()));
                        }
                    })))
                );
                // Every self-closing tag is normalized to `<x />`, including a
                // void element the author wrote as `<br>`.
                if bracket_same_line {
                    write!(f, space());
                }
                write!(f, token("/>"));
            }))
        );
        return;
    }

    let hug_start = should_hug_start(element, sensitivity);
    let hug_end = should_hug_end(element, sensitivity);
    let omit_softline_before_close =
        bracket_same_line && (!hugs_next_node(element, source) || last_in_block_parent);

    // The element's own layout owns the whitespace at the very start and end
    // of its content, so the first and last child must not print it again.
    let mut trims = vec![Trim::default(); children.len()];
    let mut separator_start = Separator::Soft;
    let mut separator_end = Separator::Soft;
    if pre {
        separator_start = Separator::None;
        separator_end = Separator::None;
    } else if !empty {
        let first = children.first();
        let last = children.last();
        let mut set_end = false;
        if !hug_start
            && let Some((index, Node::Text(text))) = children.iter().enumerate().next()
            && matches!(first, Some(Node::Text(_)))
        {
            if starts_with_line_breaks(text.value, 1)
                && children.len() > 1
                && (!inline
                    || matches!(last, Some(Node::Text(last)) if ends_with_collapsible_whitespace(last.value)))
            {
                separator_start = Separator::Hard;
                separator_end = Separator::Hard;
                set_end = true;
            } else if inline {
                separator_start = Separator::Line;
            }
            trims[index].left = true;
        }
        if !hug_end && let Some(Node::Text(_)) = last {
            if inline && !set_end {
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
                    write_attribute(attribute, source, attribute_context, f);
                }
                if (!hug_start || empty) && !pre && !bracket_same_line {
                    write!(f, dedent(&soft_line_break()));
                }
            })))
        );
    });

    let body = format_with(move |f: &mut SvelteFormatter<'_, 'a>| {
        if empty {
            return;
        }
        if pre {
            write_pre_children(children, f);
            return;
        }
        write_children(children, &trims, block, pre, f);
    });

    if empty {
        // An inline element whose only content is whitespace still renders
        // that whitespace, so it keeps a break that can become a space.
        // Otherwise there is nothing between the tags, and `bracketSameLine`
        // is what decides whether the close tag starts a line of its own.
        let inline_whitespace = inline
            && matches!(children.first(), Some(Node::Text(text)) if starts_with_collapsible_whitespace(text.value));
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, &open_tag);
                if inline_whitespace {
                    write!(f, [token(">"), soft_line_break_or_space()]);
                    write_close_tag(name, f);
                } else if bracket_same_line {
                    write!(f, [token(">"), soft_line_break()]);
                    write_close_tag(name, f);
                } else if hug_start && hug_end {
                    // An element that hugs its content on both sides renders
                    // the whitespace between its tags, so the closing tag
                    // borrows the opening `>` rather than let a break
                    // introduce any — and the two then travel together in a
                    // group of their own behind the break.
                    //
                    // That group is also what lets the attributes stay on the
                    // tag's line: once the element has broken, measuring the
                    // attribute list stops at this break instead of running on
                    // into `></span>` and counting eight characters that were
                    // never going to share the line. A block element hugs
                    // nothing, so its `>` follows the attributes directly and
                    // the measurement is right without any of this. Prettier
                    // draws exactly this distinction — `group([softline,
                    // group([">", "</span"])])` for `<span>`, a plain `">",
                    // "</div>"` for `<div>`.
                    //
                    // Hugging, not inlineness, is the test: a component is
                    // neither inline nor block, and it borrows. So does a
                    // `<textarea>`, and so does every element once
                    // `htmlWhitespaceSensitivity` is `strict` and nothing is
                    // a block any more.
                    write!(
                        f,
                        group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                            write!(f, soft_line_break());
                            write!(
                                f,
                                group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                                    write!(f, [token(">"), token("</"), text(name)]);
                                }))
                            );
                        }))
                    );
                    write!(f, token(">"));
                } else {
                    write!(f, token(">"));
                    write_close_tag(name, f);
                }
            }))
        );
        return;
    }

    if hug_start && hug_end {
        write!(
            f,
            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                write!(f, &open_tag);
                // A group of its own, so that content which still fits keeps
                // `>` on the last attribute's line even when the attribute
                // list itself had to break.
                write!(
                    f,
                    group(&indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                        write!(f, soft_line_break());
                        write!(
                            f,
                            group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                                write!(f, [token(">"), &body, token("</"), text(name)]);
                            }))
                        );
                    })))
                );
                write_close_bracket(omit_softline_before_close, f);
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
                write_close_bracket(omit_softline_before_close, f);
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

/// The `>` of a close tag whose name hugged the content, on its own line
/// unless `bracketSameLine` says it may stay where it is.
fn write_close_bracket(omit_softline: bool, f: &mut SvelteFormatter<'_, '_>) {
    if !omit_softline {
        write!(f, soft_line_break());
    }
    write!(f, token(">"));
}

/// Whether what follows the element begins right against it, with no
/// whitespace to absorb a break. The end of the component counts as a gap.
fn hugs_next_node(element: &Element<'_>, source: &str) -> bool {
    let after = &source[(element.span.end as usize).min(source.len())..];
    match after.bytes().next() {
        None => false,
        Some(byte) => !is_collapsible_whitespace(byte),
    }
}

/// The content of a `<pre>` or `<textarea>`: its text exactly as written,
/// and everything else printed as usual.
///
/// Only text renders as written. A tag, a `{…}` or a block marker is markup
/// the browser never shows, so reshaping one changes nothing on the page —
/// which is why Prettier lays them out here as it does anywhere else.
fn write_pre_children<'a>(children: &[Node<'a>], f: &mut SvelteFormatter<'_, 'a>) {
    let layout = ChildLayout { in_pre: true, ..ChildLayout::default() };
    for child in children {
        match child {
            Node::Text(text) => write_source(text.span, f),
            other => super::write_node(other, layout, f),
        }
    }
}

/// What goes between a tag and its content when the content does not hug it.
#[derive(Clone, Copy)]
enum Separator {
    /// Nothing at all: the content owns its own whitespace.
    None,
    Soft,
    Line,
    Hard,
}

impl Separator {
    fn write(self, f: &mut SvelteFormatter<'_, '_>) {
        match self {
            Self::None => {}
            Self::Soft => write!(f, soft_line_break()),
            Self::Line => write!(f, soft_line_break_or_space()),
            Self::Hard => write!(f, hard_line_break()),
        }
    }
}

/// Whether the open tag hugs its first child: there is no whitespace after
/// `>`, so a break there would add a space that renders.
fn should_hug_start(element: &Element<'_>, sensitivity: WhitespaceSensitivity) -> bool {
    if is_boundary(element) || is_block_tag(element, sensitivity) {
        return false;
    }
    let Some(first) = element.children.first() else { return true };
    // With the whitespace declared insignificant there is nothing to hug: the
    // content goes on its own line whatever the author wrote. The check comes
    // after the empty case, which Prettier still hugs.
    if sensitivity == WhitespaceSensitivity::Ignore {
        return false;
    }
    !matches!(first, Node::Text(text) if starts_with_collapsible_whitespace(text.value))
}

/// The mirror of [`should_hug_start`], for the closing tag.
fn should_hug_end(element: &Element<'_>, sensitivity: WhitespaceSensitivity) -> bool {
    if is_boundary(element) || is_block_tag(element, sensitivity) {
        return false;
    }
    let Some(last) = element.children.last() else { return true };
    if sensitivity == WhitespaceSensitivity::Ignore {
        return false;
    }
    !matches!(last, Node::Text(text) if ends_with_collapsible_whitespace(text.value))
}
