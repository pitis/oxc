//! Printing `<script>` and `<style>`, whose bodies are other languages.
//!
//! The body is handed to the formatter that owns it through the session's
//! dispatcher, and its IR is spliced into this document. When there is no
//! dispatcher, or the child refuses (a language nothing formats, a parse
//! error), the body is kept exactly as written — a component must never come
//! back with its script rewritten into something that does not parse.

use cow_utils::CowUtils;
use oxc_formatter_core::{
    Buffer, BufferExtensions, DispatchRequest, DispatchResponse, Format, FormatElement, InputKind,
    ScriptInComponentFile,
    builders::{
        block_indent, dedent, group, hard_line_break, indent, soft_line_break,
        soft_line_break_or_space, text, token,
    },
    write,
};
use oxc_span::Span;
use svelte_markup_parser::ast::Element;

use super::{
    SvelteFormatter,
    attribute::{AttributeContext, write_attribute},
    format_with, write_source,
};

/// Print a `<script>` or `<style>` element: its tag as markup, its body by
/// whoever owns that language.
pub fn write_raw_text_element<'a>(element: &Element<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    let Some(body) = element.raw_text else {
        write_source(element.span, f);
        return;
    };
    let source = f.context().source_text().as_str();
    let options = *f.options();
    // A `<script>` or `<style>` is a regular element, so its attributes are
    // printed under the same rules as any other tag's.
    let attribute_context = AttributeContext {
        allow_shorthand: options.allow_shorthand.is_enabled(),
        regular_element: true,
    };
    let body_text = &source[body.start as usize..body.end as usize];

    // The open tag is markup like any other, so it is printed rather than
    // copied — attribute spacing and quoting get normalized with the rest,
    // and a list too long for the line wraps the same way.
    let bracket_same_line = options.bracket_same_line.is_enabled();
    let attributes: Vec<&_> = element.attributes.iter().collect();
    write!(f, [token("<"), text(element.name)]);
    // `indent` rejects content that produces nothing, so a bare `<script>`
    // under `bracketSameLine` skips the wrapper entirely.
    if !attributes.is_empty() || !bracket_same_line {
        write!(
            f,
            indent(&group(&format_with(|f: &mut SvelteFormatter<'_, 'a>| {
                for attribute in &attributes {
                    write!(f, soft_line_break_or_space());
                    write_attribute(attribute, source, attribute_context, f);
                }
                if !bracket_same_line {
                    write!(f, dedent(&soft_line_break()));
                }
            })))
        );
    }
    write!(f, token(">"));

    // Nothing between the tags at all, so they meet. Whitespace *is*
    // something: an author who left a line in an empty `<script>` keeps one.
    if body_text.is_empty() {
        write_close_tag(element, f);
        return;
    }
    if body_text.trim().is_empty() {
        write!(f, hard_line_break());
        write_close_tag(element, f);
        return;
    }

    let Some(language) = language_of(element) else {
        // A `lang` nothing here formats: the body is not ours to touch.
        write_verbatim_body(body, f);
        write_close_tag(element, f);
        return;
    };

    let response = f.session().dispatch(DispatchRequest {
        language,
        text: body_text,
        input_kind: InputKind::Fragment,
        // A `<script>` is not a file of its own — see [`ScriptInComponentFile`].
        parent_context: Some(&ScriptInComponentFile),
    });
    let Ok(DispatchResponse::Formatted(payload)) = response else {
        write_verbatim_body(body, f);
        write_close_tag(element, f);
        return;
    };

    let mut ir = payload.into_doc();
    // A whole-program IR ends with the newline a *file* wants. Here the tag
    // supplies it, and leaving both makes the break ambiguous: it sits between
    // the end of the indent and the close tag, and whoever resolves the pair
    // decides which indentation `</script>` lands on.
    while matches!(ir.last(), Some(FormatElement::Line(_))) {
        ir.pop();
    }
    let content = WriteElements(std::cell::RefCell::new(Some(ir)));
    if options.indent_script_and_style.is_enabled() {
        write!(f, block_indent(&content));
    } else {
        write!(f, [hard_line_break(), &content, hard_line_break()]);
    }
    write_close_tag(element, f);
}

/// The language a raw-text element's body is written in, or `None` when it
/// is one this formatter has no owner for.
fn language_of(element: &Element<'_>) -> Option<&'static str> {
    let lang = element
        .attributes
        .iter()
        .find_map(|attribute| match &attribute.kind {
            svelte_markup_parser::ast::AttributeKind::Plain {
                name: "lang",
                value: Some(value),
                ..
            } => value.as_static_text(),
            _ => None,
        })
        .unwrap_or("");

    let lang = lang.cow_to_ascii_lowercase();
    if element.name.eq_ignore_ascii_case("script") {
        return match lang.as_ref() {
            "" | "js" | "javascript" => Some("js"),
            "ts" | "typescript" => Some("ts"),
            _ => None,
        };
    }
    if element.name.eq_ignore_ascii_case("style") {
        return match lang.as_ref() {
            "" | "css" | "postcss" | "pcss" => Some("css"),
            "scss" => Some("scss"),
            "less" => Some("less"),
            _ => None,
        };
    }
    None
}

/// Keep the body byte for byte, including the line breaks around it.
fn write_verbatim_body(body: Span, f: &mut SvelteFormatter<'_, '_>) {
    write_source(body, f);
}

/// The child's IR, written once into whatever position the layout puts it.
///
/// `block_indent` takes its content by reference and may format it more than
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

fn write_close_tag<'a>(element: &Element<'a>, f: &mut SvelteFormatter<'_, 'a>) {
    write!(f, [token("</"), text(element.name), token(">")]);
}
