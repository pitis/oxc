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
use oxc_allocator::ArenaVec;
use oxc_formatter_core::{
    Buffer, BufferExtensions, DispatchRequest, DispatchResponse, ExpressionHugsDelimiters,
    FormatElement, InputKind,
    builders::{expand_parent, indent, soft_line_break_or_space, space, text},
    write,
};

use super::{
    VueFormatter, format_with,
    tag::{
        needs_to_borrow_prev_closing_tag_end_marker, write_closing_tag_suffix,
        write_opening_tag_prefix,
    },
    text::write_text,
    tree::{NodeId, Tree},
};

/// The language a `<script>` or `<style>` body is written in, or `None` when
/// nothing here owns it — an unrecognised `lang` or `type`, or a `src`
/// attribute, which means the body is elsewhere.
///
/// A `<script>` only defaults to JavaScript when it declares *neither* `lang`
/// nor `type`. One that declares either and names something unrecognised is
/// deliberately left alone: `<script type="text/x-template">` holds markup,
/// not a program, and formatting it as JavaScript would destroy it.
pub fn element_language(tree: &Tree<'_, '_>, id: NodeId) -> Option<&'static str> {
    let node = tree.node(id);
    if node.attribute_value("src").is_some() {
        return None;
    }
    let lang = node.declared_attribute_value("lang");
    let kind = node.declared_attribute_value("type");
    match node.name() {
        "script" => {
            if lang.is_none() && kind.is_none() {
                return Some("js");
            }
            lang.and_then(language_of_lang).or_else(|| kind.and_then(language_of_type))
        }
        // A `<style>` reads only its `lang`, and defaults to CSS without one.
        "style" => match lang {
            Some(lang) => language_of_lang(lang),
            None => Some("css"),
        },
        _ => None,
    }
}

/// The language a `lang` attribute names, of those something here formats.
pub fn language_of_lang(lang: &str) -> Option<&'static str> {
    Some(match lang.cow_to_ascii_lowercase().as_ref() {
        "js" | "javascript" | "babel" => "js",
        "ts" | "typescript" => "ts",
        "jsx" => "jsx",
        "tsx" => "tsx",
        "css" | "postcss" | "pcss" => "css",
        "scss" => "scss",
        "less" => "less",
        "json" => "json",
        "json5" => "json5",
        "jsonc" => "jsonc",
        "yaml" | "yml" => "yaml",
        "graphql" | "gql" => "graphql",
        "md" | "markdown" => "markdown",
        "html" => "html",
        "handlebars" | "glimmer" | "hbs" => "glimmer",
        _ => return None,
    })
}

/// The language a `type` attribute names. Ported from Prettier's
/// `inferParserByTypeAttribute`, including its suffix rules — an importmap and
/// anything ending in `json` are JSON whatever the vendor prefix.
pub fn language_of_type(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "module" | "text/javascript" | "text/babel" | "text/jsx" | "application/javascript" => "js",
        "application/x-typescript" => "ts",
        "text/markdown" => "markdown",
        "text/html" => "html",
        "text/x-handlebars-template" => "glimmer",
        _ => {
            if kind.ends_with("json") || kind.ends_with("importmap") || kind == "speculationrules" {
                "json"
            } else {
                return None;
            }
        }
    })
}

/// A `<script>` or `<style>` body, formatted by whoever owns that language.
///
/// The element around it is printed as any other, so only the body itself is
/// replaced. The forced break is what keeps `<script>` from ever collapsing
/// onto one line with its content.
///
/// When nothing formats it — an unrecognised `lang`, or a body that does not
/// parse — it is not spliced back raw: it goes through the ordinary text path,
/// which is where the block's own leading newline gets trimmed. Splicing it
/// raw leaves that newline next to the one the layout writes after the open
/// tag, and the block gains a blank line it never had.
pub fn write_script_like_text<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    language: Option<&'static str>,
    f: &mut VueFormatter<'_, 'a>,
) {
    let value = tree.node(id).value;
    let formatted = language
        .and_then(|language| dispatch(language, value, value, f))
        .map(|fragment| fragment.ir)
        .filter(|ir| !ir.is_empty());
    let Some(mut ir) = formatted else {
        write_text(tree, id, f);
        return;
    };
    // A whole-program IR ends with the newline a *file* wants. Here the
    // element's own layout supplies it.
    while matches!(ir.last(), Some(FormatElement::Line(_))) {
        ir.pop();
    }
    write!(f, expand_parent());
    write_opening_tag_prefix(tree, id, f);
    f.write_elements(ir);
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
    let Some(fragment) = dispatch(language, source, source, f) else { return false };
    let mut ir = fragment.ir;
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

/// Hand `source` to the formatter that owns `language` and return its IR.
///
/// `snippet` is what the *author* wrote, which is not always `source`: a
/// binding list is wrapped as `function _(…) {}` before it can be parsed, and
/// a warning has to quote the value, not the wrapper.
///
/// A fragment that does not parse is recorded rather than passed over: the
/// printer keeps it as written either way, but silently keeping a syntax
/// error is how a broken `v-if` survives a format run unnoticed.
pub fn dispatch<'a>(
    language: &'static str,
    source: &str,
    snippet: &str,
    f: &mut VueFormatter<'_, 'a>,
) -> Option<Fragment<'a>> {
    if source.trim().is_empty() {
        return None;
    }
    let response = f.session().dispatch(DispatchRequest {
        language,
        text: source,
        input_kind: InputKind::Fragment,
        parent_context: None,
    });
    let Ok(DispatchResponse::Formatted(payload)) = response else {
        if let Some(context) = warning_context(language) {
            f.context_mut().report_fragment_failure(snippet, context);
        }
        return None;
    };
    f.context_mut().report_fragment_success(snippet);
    // Whether the fragment's own brackets can hold the host's indentation:
    // only the language that parsed it can say, so it travels back with the IR.
    let hugs = payload
        .child_context
        .as_ref()
        .and_then(|context| context.downcast_ref::<ExpressionHugsDelimiters>())
        .is_none_or(|hugs| hugs.0);
    Some(Fragment { ir: payload.into_doc(), hugs })
}

/// A formatted fragment, with the layout answer its language gave.
pub struct Fragment<'a> {
    pub ir: ArenaVec<'a, FormatElement<'a>>,
    pub hugs: bool,
}

/// What a fragment of this language is called in a warning, in the vocabulary
/// the Prettier-side channel already uses, or `None` for the languages that
/// channel never covered — CSS is reported by nobody.
fn warning_context(language: &str) -> Option<&'static str> {
    Some(match language {
        "ts-attribute-expression" | "js-attribute-expression" => "expression-attribute",
        "vue-attribute-expression" => "vue-expression-attribute",
        "vue-expression" => "vue-expression-interpolation",
        "vue-event-handler" => "event-handler",
        "vue-v-for-left" => "vue-for-binding-left",
        "vue-binding-params" => "vue-bindings",
        "vue-generic" => "vue-script-generic",
        "js" | "ts" | "jsx" | "tsx" => "vue-script",
        _ => return None,
    })
}
