//! Printing the JavaScript inside `{…}`.
//!
//! The expression is handed to `oxc_formatter` through the session's
//! dispatcher, exactly as a `<script>` body is. When there is no dispatcher,
//! or the expression does not parse, the source text is kept: a component
//! must never come back with an expression rewritten into something that
//! does not mean the same thing.

use oxc_formatter_core::{
    Buffer, BufferExtensions, DispatchRequest, DispatchResponse, InputKind,
    builders::{text, token},
    write,
};
use svelte_markup_parser::ast::ExpressionTag;

use super::{SvelteFormatter, write_source};

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
}

/// Print a `{…}` in a text position, which may be a declaration tag.
pub fn write_mustache<'a>(tag: &ExpressionTag<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    if !tag.unterminated
        && let Some((keyword, declarator)) = declaration_tag(tag.expression)
    {
        // `{const x = 1}` declares a binding for the rest of the fragment.
        // The declarator is the same shape `{@const x = 1}` carries, so it
        // goes through the expression path: `x = 1` reads as an assignment
        // and `{ a, b } = obj` as one to a destructuring pattern, both of
        // which the JavaScript formatter lays out the way Prettier does.
        write!(f, [token("{"), text(keyword), token(" ")]);
        write_expression(declarator, ExpressionPosition::Braces, f);
        write!(f, token("}"));
        return;
    }
    write_expression_tag(tag, ExpressionPosition::Braces, f);
}

/// Split a declaration tag's text into its keyword and what follows.
///
/// `type` is left alone: what follows it is a type, not an expression, and
/// this printer has no way to lay one out on its own.
fn declaration_tag(expression: &str) -> Option<(&'static str, &str)> {
    let expression = expression.trim_start();
    for keyword in ["const", "let"] {
        if let Some(rest) = expression.strip_prefix(keyword)
            && rest.starts_with(|c: char| c.is_whitespace())
            && !rest.trim_start().is_empty()
        {
            return Some((keyword, rest.trim_start()));
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
        ExpressionPosition::Braces => "ts-expression",
        ExpressionPosition::QuotedAttribute => "ts-attribute-expression",
    };
    let response = f.session().dispatch(DispatchRequest {
        language,
        text: expression,
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
    let ir = payload.into_doc(f.context_mut());
    f.write_elements(ir);
}
