//! Printing `{#if}`, `{#each}`, `{#await}`, `{#key}`, `{#snippet}` and the
//! `{@…}` tags.
//!
//! A block's markers are printed here and its branches go back through the
//! ordinary children layout, so an element inside a block is laid out the
//! same way it would be anywhere else.

// `{:else}` is a Svelte block marker; the `{…}` in these literals is markup,
// not a format argument.
#![expect(clippy::literal_string_with_formatting_args)]

use oxc_formatter_core::{
    Buffer,
    builders::{
        empty_line, expand_parent, group, hard_line_break, indent, soft_line_break_or_space, text,
        token,
    },
    write,
};
use svelte_markup_parser::ast::{
    AwaitBlock, Block, BlockKind, EachBlock, ExpressionSlot, IfBlock, KeyBlock, Node, SnippetBlock,
    Tag, TagKind,
};

use super::{
    SvelteFormatter,
    children::Trim,
    classify::{
        ends_with_collapsible_whitespace, ends_with_line_breaks, is_empty_text,
        starts_with_collapsible_whitespace, starts_with_line_breaks, trimmed,
    },
    expression::{ExpressionPosition, write_expression},
    format_with, write_children, write_source,
};

pub fn write_block<'a>(block: &Block<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    // A block the parser had to recover has no closing marker to print, and
    // its branches are a guess; keep it exactly as written.
    if block.unclosed {
        write_source(block.span, f);
        return;
    }
    match &block.kind {
        BlockKind::If(if_block) => write_if_block(if_block, f),
        BlockKind::Each(each) => write_each_block(each, f),
        BlockKind::Await(await_block) => write_await_block(await_block, f),
        BlockKind::Key(key) => write_key_block(key, f),
        BlockKind::Snippet(snippet) => write_snippet_block(snippet, f),
        // A `{#word …}` this crate does not know: its header is not ours to
        // reshape, so the whole block keeps its spelling.
        BlockKind::Unknown(_) => write_source(block.span, f),
    }
}

/// `{#if}` with its whole `{:else if}` / `{:else}` chain.
fn write_if_block<'a>(if_block: &IfBlock<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    write!(
        f,
        group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
            for (index, branch) in if_block.branches.iter().enumerate() {
                match (&branch.expression, index) {
                    (Some(expression), 0) => {
                        write!(f, token("{#if "));
                        write_slot(expression, f);
                        write!(f, token("}"));
                    }
                    (Some(expression), _) => {
                        write!(f, token("{:else if "));
                        write_slot(expression, f);
                        write!(f, token("}"));
                    }
                    (None, _) => write!(f, token("{:else}")),
                }
                write_branch(&branch.children, f);
            }
            write!(f, token("{/if}"));
            // A block always breaks: it is control flow, and reading it on
            // one line is not what anyone wants from it.
            write!(f, expand_parent());
        }))
    );
}

fn write_each_block<'a>(each: &EachBlock<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    write!(
        f,
        group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
            write!(f, token("{#each "));
            write_slot(&each.expression, f);
            // The `as` pattern and the index name are bindings, not
            // expressions: they keep the spelling the author gave them.
            if let Some(context) = &each.context {
                write!(f, [token(" as "), text(context.text.trim())]);
            }
            if let Some(index) = &each.index {
                write!(f, [token(", "), text(index.text.trim())]);
            }
            if let Some(key) = &each.key {
                write!(f, token(" ("));
                write_slot(key, f);
                write!(f, token(")"));
            }
            write!(f, token("}"));
            write_branch(&each.children, f);
            if let Some(fallback) = &each.fallback {
                write!(f, token("{:else}"));
                write_branch(fallback, f);
            }
            write!(f, token("{/each}"));
            write!(f, expand_parent());
        }))
    );
}

fn write_await_block<'a>(await_block: &AwaitBlock<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    let has_pending = await_block.pending.iter().any(|node| !is_empty_text(node));
    let has_then =
        await_block.then_children.as_ref().is_some_and(|c| c.iter().any(|n| !is_empty_text(n)));
    let has_catch =
        await_block.catch_children.as_ref().is_some_and(|c| c.iter().any(|n| !is_empty_text(n)));

    write!(
        f,
        group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
            // With nothing to show while pending, the whole thing collapses
            // into the one-line `{#await expr then value}` form.
            if !has_pending && has_then {
                write!(f, token("{#await "));
                write_slot(&await_block.expression, f);
                write!(f, token(" then"));
                write_binding(await_block.then_pattern.as_ref(), f);
                write!(f, token("}"));
                write_branch(await_block.then_children.as_deref().unwrap_or(&[]), f);
            } else if !has_pending && has_catch {
                write!(f, token("{#await "));
                write_slot(&await_block.expression, f);
                write!(f, token(" catch"));
                write_binding(await_block.catch_pattern.as_ref(), f);
                write!(f, token("}"));
                write_branch(await_block.catch_children.as_deref().unwrap_or(&[]), f);
            } else {
                write!(f, token("{#await "));
                write_slot(&await_block.expression, f);
                write!(f, token("}"));
                if has_pending {
                    write_branch(&await_block.pending, f);
                }
                if has_then {
                    write!(f, token("{:then"));
                    write_binding(await_block.then_pattern.as_ref(), f);
                    write!(f, token("}"));
                    write_branch(await_block.then_children.as_deref().unwrap_or(&[]), f);
                }
            }

            if (has_pending || has_then) && has_catch {
                write!(f, token("{:catch"));
                write_binding(await_block.catch_pattern.as_ref(), f);
                write!(f, token("}"));
                write_branch(await_block.catch_children.as_deref().unwrap_or(&[]), f);
            }
            write!(f, token("{/await}"));
        }))
    );
}

fn write_key_block<'a>(key: &KeyBlock<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    write!(
        f,
        group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
            write!(f, token("{#key "));
            write_slot(&key.expression, f);
            write!(f, token("}"));
            write_branch(&key.children, f);
            write!(f, [token("{/key}"), expand_parent()]);
        }))
    );
}

fn write_snippet_block<'a>(snippet: &SnippetBlock<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    write!(f, token("{#snippet "));
    // The header is one JavaScript expression: `name(params)` reads as a
    // call, and that is what gives the parameter list a function signature's
    // layout instead of an argument list's. No parens at all is not valid
    // Svelte, but the parser recovered it, and then the name is all there is.
    if let Some(params) = &snippet.params {
        let source = f.context().source_text().as_str();
        let start = snippet.name_span.start as usize;
        let end = (params.span.end as usize + 1).min(source.len());
        write_expression(&source[start..end], ExpressionPosition::Braces, f);
    } else {
        write!(f, text(snippet.name));
    }
    write!(f, token("}"));
    write_branch(&snippet.children, f);
    write!(f, token("{/snippet}"));
}

/// One `{@…}` tag.
pub fn write_tag<'a>(tag: &Tag<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    if tag.unterminated {
        write_source(tag.span, f);
        return;
    }
    let keyword = tag.keyword;
    let expression = tag.expression.trim();
    match tag.kind {
        // `{@debug}` takes a comma-separated identifier list, not an
        // expression, and with none at all it is just `{@debug}`.
        TagKind::Debug if expression.is_empty() => write!(f, token("{@debug}")),
        TagKind::Unknown => write_source(tag.span, f),
        _ => {
            write!(f, [token("{@"), text(keyword), token(" ")]);
            write_expression(tag.expression, ExpressionPosition::Braces, f);
            write!(f, token("}"));
        }
    }
}

/// An expression slot of a block header, which stays on one line however
/// long it is: it reads as part of the marker rather than as content laid out
/// beside it.
fn write_slot<'a>(slot: &ExpressionSlot<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    write_expression(slot.text, ExpressionPosition::BlockHeader, f);
}

/// A `{:then value}` / `{:catch error}` binding, which keeps its spelling and
/// brings its own leading space when there is one.
fn write_binding<'a>(slot: Option<&ExpressionSlot<'a>>, f: &mut SvelteFormatter<'_, 'a>) {
    let Some(slot) = slot else { return };
    let text_value = slot.text.trim();
    if text_value.is_empty() {
        return;
    }
    write!(f, [token(" "), text(text_value)]);
}

/// A block branch's children, indented, with the whitespace the author left
/// around them decided once for the whole branch.
fn write_branch<'a>(children: &[Node<'a>], f: &mut SvelteFormatter<'_, 'a>) {
    if children.is_empty() {
        return;
    }
    let start = whitespace_at_start(children);
    let end = whitespace_at_end(children);
    // One `line` anywhere in the branch means both ends break, so the
    // markers keep their own lines.
    let broken = start == Whitespace::Line || end == Whitespace::Line;

    let mut trims = vec![Trim::default(); children.len()];
    if let Some(Node::Text(text)) = children.first()
        && starts_with_collapsible_whitespace(text.value)
    {
        trims[0].left = true;
    }
    if let Some(Node::Text(text)) = children.last()
        && ends_with_collapsible_whitespace(text.value)
    {
        let last = children.len() - 1;
        trims[last].right = true;
    }

    // Nothing survives the trim, so the two edges meet. Written as one blank
    // line rather than two breaks: the printer only starts a new line once
    // per line of output, so two breaks in a row would collapse into one.
    let nothing_left = children.iter().zip(&trims).all(|(child, trim)| {
        matches!(child, Node::Text(text) if trimmed(text.value, trim.left, trim.right).is_empty())
    });
    if nothing_left && start != Whitespace::None && end != Whitespace::None {
        if broken {
            write!(f, empty_line());
        } else {
            write!(f, [soft_line_break_or_space(), soft_line_break_or_space()]);
        }
        return;
    }

    write!(
        f,
        indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
            write_edge(start, broken, f);
            write!(
                f,
                group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                    write_children(children, &trims, false, false, f);
                }))
            );
        }))
    );
    write_edge(end, broken, f);
}

fn write_edge(whitespace: Whitespace, broken: bool, f: &mut SvelteFormatter<'_, '_>) {
    match whitespace {
        Whitespace::None => {}
        _ if broken => write!(f, hard_line_break()),
        _ => write!(f, soft_line_break_or_space()),
    }
}

/// How a branch's content is separated from its markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whitespace {
    /// Nothing between the marker and the content: they must stay together.
    None,
    /// A space, which may become a break.
    Space,
    /// A line the author wrote, which stays a line.
    Line,
}

fn whitespace_at_start(children: &[Node<'_>]) -> Whitespace {
    let Some(Node::Text(text)) = children.first() else { return Whitespace::None };
    if starts_with_line_breaks(text.value, 1) {
        Whitespace::Line
    } else if starts_with_collapsible_whitespace(text.value) {
        Whitespace::Space
    } else {
        Whitespace::None
    }
}

fn whitespace_at_end(children: &[Node<'_>]) -> Whitespace {
    let Some(Node::Text(text)) = children.last() else { return Whitespace::None };
    if ends_with_line_breaks(text.value, 1) {
        Whitespace::Line
    } else if ends_with_collapsible_whitespace(text.value) {
        Whitespace::Space
    } else {
        Whitespace::None
    }
}
