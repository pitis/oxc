//! Printing the JavaScript inside `{…}`.
//!
//! The expression is handed to `oxc_formatter` through the session's
//! dispatcher, exactly as a `<script>` body is. When there is no dispatcher,
//! or the expression does not parse, the source text is kept: a component
//! must never come back with an expression rewritten into something that
//! does not mean the same thing.

use oxc_formatter_core::{
    Buffer, BufferExtensions, DispatchRequest, DispatchResponse, Format, InputKind,
    builders::{dedent, group, indent, soft_line_break, text, token},
    remove_lines, write,
};
use svelte_markup_parser::ast::ExpressionTag;

use super::{SvelteFormatter, format_with, write_source};

/// Where an expression sits, which decides its preferred quote style.
///
/// A Svelte expression is delimited by its own braces, so it normally keeps
/// the configured quote style — unlike Vue, where an attribute value really
/// is inside quotes. The one exception is an expression that is *part* of a
/// quoted value (`class="a {x} b"`), where a `"` of its own would end the
/// attribute early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionPosition {
    /// Delimited by its own braces: text position, a lone attribute value, a
    /// spread, a `{@…}` tag's argument.
    Braces,
    /// One part of a quoted attribute value made of text and expressions.
    QuotedAttribute,
    /// A `{#if}` / `{#each}` / `{#await}` / `{#key}` header, which stays on
    /// one line however long it is (Prettier's `forceSingleLine`).
    BlockHeader,
    /// A `bind:` directive's value, which gets a break of its own to move to
    /// when the tag wraps (Prettier's `surroundWithSoftline`). Svelte 5 spells
    /// a function binding `bind:x={get, set}`, so a comma here is its grammar.
    BindDirective,
    /// What a `{#each}` iterates. Svelte 5 spells the index `{#each expr, i}`,
    /// so a comma here is its grammar too — the rest of a block header is one
    /// expression and takes [`ExpressionPosition::BlockHeader`].
    EachSubject,
    /// A `{#snippet name(params)}` header, already wrapped by the caller as
    /// `function name(params) {}` (Prettier's `asFunction`).
    SnippetSignature,
    /// A `{const x = 1}` / `{let a = 1, b = 2}` tag's contents, keyword
    /// included — a declaration, laid out as one.
    DeclarationTag,
}

/// Print a `{…}` in a text position, which may be a declaration tag.
pub fn write_mustache<'a>(tag: &ExpressionTag<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    if !tag.unterminated
        && let Some((keyword, declarator)) = declaration_tag(tag.expression)
    {
        // `{const x = 1}` declares a binding for the rest of the fragment. The
        // declarators go to the JavaScript formatter as the declarators they
        // are, so `{let a = 1, b = 2}` lays out as one declaration rather than
        // coming back as the sequence expression `(a = 1), (b = 2)`.
        write!(f, [token("{"), text(keyword), token(" ")]);
        write_expression(declarator, ExpressionPosition::DeclarationTag, f);
        write!(f, token("}"));
        return;
    }
    write_expression_tag(tag, ExpressionPosition::Braces, f);
}

/// Split a declaration tag's text into its keyword and its declarators.
///
/// `type` is left alone: what follows it is a type, not an expression, and
/// this printer has no way to lay one out on its own.
fn declaration_tag(expression: &str) -> Option<(&'static str, &str)> {
    let trimmed = expression.trim_start();
    for keyword in ["const", "let"] {
        if let Some(rest) = trimmed.strip_prefix(keyword)
            && rest.starts_with(|c: char| c.is_whitespace())
        {
            let declarator = rest.trim_start();
            if declarator.is_empty() {
                return None;
            }
            return Some((keyword, declarator));
        }
    }
    None
}

/// Print `{expr}`, with the expression formatted by whoever owns JavaScript.
pub fn write_expression_tag<'a>(
    tag: &ExpressionTag<'a>,
    position: ExpressionPosition,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    if tag.unterminated {
        write_source(tag.span, f);
        return;
    }
    write!(f, token("{"));
    write_expression(tag.expression, position, f);
    write!(f, token("}"));
}

/// Print one expression's text, formatted if it can be.
pub fn write_expression<'a>(
    expression: &'a str,
    position: ExpressionPosition,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    if expression.trim().is_empty() {
        // Nothing at all rather than the source's own padding: a header that
        // brings its own space would otherwise grow by one every run. The
        // parser already reports such a construct as recovery, so this is
        // only reached through an embedder that formats anyway.
        return;
    }

    let language = match position {
        // TypeScript is the safe reading either way: a `.svelte` component's
        // markup can carry `as` casts and non-null assertions whether or not
        // its script declares `lang="ts"`, and TS is a superset of what plain
        // JavaScript allows here.
        // A `{…}` in content is spliced bare, so a broken expression indents
        // itself. So does a block header: flattening it leaves the breaks the
        // author cannot get rid of — a member chain long enough to break does
        // so through hard lines — and the chain's continuation then sits under
        // the indent the binaryish chain around it supplies. Only a `bind:`
        // value carries an indent from the embed site, so only it keeps the
        // ordinary fragment route.
        ExpressionPosition::Braces | ExpressionPosition::BlockHeader => "svelte-expression",
        ExpressionPosition::EachSubject => "svelte-each-subject",
        ExpressionPosition::BindDirective => "svelte-bind-value",
        ExpressionPosition::QuotedAttribute => "svelte-attribute-expression",
        ExpressionPosition::SnippetSignature => "svelte-snippet-signature",
        ExpressionPosition::DeclarationTag => "svelte-declaration-tag",
    };
    // A snippet header is a function signature with the keyword left out, so
    // it is put back before the fragment is handed over — and left out of the
    // fallback below, which is the header as the author wrote it.
    let wrapped = matches!(position, ExpressionPosition::SnippetSignature)
        .then(|| f.allocator().alloc_str(&format!("function {} {{}}", expression.trim())));
    let response = f.session().dispatch(DispatchRequest {
        language,
        text: wrapped.unwrap_or(expression),
        input_kind: InputKind::Fragment,
        parent_context: None,
    });
    let Ok(DispatchResponse::Formatted(payload)) = response else {
        write!(f, text(expression.trim()));
        return;
    };

    // Spliced as-is, with no group or indent of its own. Where Vue and
    // Angular decide hug-vs-expand at the embed site — Prettier's
    // `__onHtmlBindingRoot` hook — `prettier-plugin-svelte` does not: its
    // `{…}` is the expression's doc between two braces, and the expression's
    // own groups decide where it breaks.
    let mut ir = payload.into_doc();
    match position {
        // A block header is one line however long it gets: the expression
        // that decides what a `{#each}` iterates reads as part of the marker,
        // not as content laid out beside it.
        ExpressionPosition::BlockHeader | ExpressionPosition::EachSubject => {
            remove_lines(&mut ir, f.allocator());
            f.write_elements(ir);
        }
        // A `bind:` value carries a break of its own, so a tag that has to
        // wrap can put the value on the next line rather than only inside it.
        ExpressionPosition::BindDirective => {
            let content = WriteElements(std::cell::RefCell::new(Some(ir)));
            write!(
                f,
                group(&indent(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                    write!(f, [soft_line_break(), group(&content), dedent(&soft_line_break())]);
                })))
            );
        }
        _ => f.write_elements(ir),
    }
}

/// The child's IR, written once into whatever position the layout puts it.
///
/// The builders take their content by reference and may format it more than
/// once while measuring, so the elements are moved out on the first write.
struct WriteElements<'a>(
    std::cell::RefCell<Option<oxc_allocator::ArenaVec<'a, oxc_formatter_core::FormatElement<'a>>>>,
);

impl<'a> Format<'a, crate::context::SvelteFormatContext<'a>> for WriteElements<'a> {
    fn fmt(&self, f: &mut SvelteFormatter<'_, 'a>) {
        if let Some(elements) = self.0.borrow_mut().take() {
            f.write_elements(elements);
        }
    }
}
