//! Handing a body to the formatter that owns its language.
//!
//! A component is several languages in one file: JavaScript in `<script>`,
//! CSS in `<style>`, JavaScript again in every `{{ … }}`. None of them is
//! this printer's to lay out — each goes to the formatter that owns it
//! through the session's dispatcher, and comes back as IR this document
//! splices in.
//!
//! When there is no dispatcher, or the body does not parse, the source is
//! kept exactly as written. A component must never come back with its script
//! rewritten into something that does not mean the same thing.

use cow_utils::CowUtils;
use oxc_formatter_core::{
    Buffer, BufferExtensions, DispatchRequest, DispatchResponse, FormatElement, InputKind,
    builders::{expand_parent, indent, soft_line_break_or_space, space, text},
    write,
};

use super::{
    VueFormatter, format_with,
    tag::{
        needs_to_borrow_prev_closing_tag_end_marker, write_closing_tag_suffix,
        write_opening_tag_prefix,
    },
    tree::{NodeId, Tree},
};

/// The language a `<script>` or `<style>` body is written in, or `None` when
/// nothing here owns it — an unrecognised `lang`, or a `src` attribute, which
/// means the body is elsewhere.
pub fn element_language(tree: &Tree<'_, '_>, id: NodeId) -> Option<&'static str> {
    let node = tree.node(id);
    if node.attribute_value("src").is_some() {
        return None;
    }
    let lang = node.attribute_value("lang").unwrap_or("").cow_to_ascii_lowercase();
    match node.name() {
        "script" => match lang.as_ref() {
            "" | "js" | "javascript" => Some("js"),
            "ts" | "typescript" => Some("ts"),
            "jsx" => Some("jsx"),
            "tsx" => Some("tsx"),
            _ => None,
        },
        "style" => match lang.as_ref() {
            "" | "css" | "postcss" | "pcss" => Some("css"),
            "scss" => Some("scss"),
            "less" => Some("less"),
            _ => None,
        },
        _ => None,
    }
}

/// A `<script>` or `<style>` body, formatted by whoever owns that language.
///
/// The element around it is printed as any other, so only the body itself is
/// replaced. The forced break is what keeps `<script>` from ever collapsing
/// onto one line with its content.
pub fn write_script_like_text<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    language: Option<&'static str>,
    f: &mut VueFormatter<'_, 'a>,
) {
    write!(f, expand_parent());
    write_opening_tag_prefix(tree, id, f);
    let value = tree.node(id).value;
    if !language.is_some_and(|language| write_formatted(language, value, f)) {
        write!(f, text(value));
    }
    write_closing_tag_suffix(tree, id, f);
}

/// The expression inside `{{ … }}`.
///
/// The braces are the interpolation's own; what this adds is the break
/// between them and the expression, which is where a long interpolation
/// wraps. The trailing break becomes a plain space when the node after the
/// interpolation has borrowed a delimiter — there is nowhere to break there.
pub fn write_interpolation_text<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let value = tree.node(id).value;
    write!(
        f,
        indent(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            write!(f, soft_line_break_or_space());
            if !write_formatted("vue-expression", value, f) {
                write!(f, text(value.trim()));
            }
        }))
    );

    let hugs_next = tree
        .node(id)
        .parent
        .and_then(|parent| tree.next(parent))
        .is_some_and(|next| needs_to_borrow_prev_closing_tag_end_marker(tree, next));
    if hugs_next {
        write!(f, space());
    } else {
        write!(f, soft_line_break_or_space());
    }
}

/// Dispatch `source` and splice the result in. Returns whether it worked; a
/// `false` leaves nothing written, so the caller can fall back.
pub fn write_formatted<'a>(
    language: &'static str,
    source: &'a str,
    f: &mut VueFormatter<'_, 'a>,
) -> bool {
    if source.trim().is_empty() {
        return false;
    }
    let response = f.session().dispatch(DispatchRequest {
        language,
        text: source,
        input_kind: InputKind::Fragment,
        parent_context: None,
    });
    let Ok(DispatchResponse::Formatted(payload)) = response else {
        return false;
    };
    let mut ir = payload.into_doc(f.context_mut());
    // A whole-program IR ends with the newline a *file* wants. Here the
    // element's own layout supplies it.
    while matches!(ir.last(), Some(FormatElement::Line(_))) {
        ir.pop();
    }
    if ir.is_empty() {
        return false;
    }
    f.write_elements(ir);
    true
}
