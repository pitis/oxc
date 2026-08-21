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
    FormatElement, InputKind, ScriptInComponentFile,
    builders::{
        empty_line, expand_parent, hard_line_break, indent, literal_line_break,
        soft_line_break_or_space, space, text,
    },
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
        .and_then(|language| dispatch_script(language, value, value, f))
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
    let language = tree.script_flavour().interpolation();
    let Some(fragment) = dispatch(language, value, value, f) else {
        // Nothing parsed it, so it is not an expression — it is text, and the
        // author's own spacing is all the meaning it has left. Reflowing it
        // would be inventing a layout for something this printer does not
        // understand.
        write_unparsed_interpolation(value, f);
        return;
    };

    let expression = Doc(std::cell::RefCell::new(Some(fragment.ir)));
    write!(
        f,
        indent(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            write!(f, [soft_line_break_or_space(), &expression]);
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

/// An interpolation whose contents are not an expression, kept exactly.
///
/// The lines are literal, so the indentation the author wrote survives
/// whatever the surrounding markup is indented to. The one newline that ends
/// the value is dropped and re-supplied as an ordinary break, which is what
/// puts the closing `}}` at the element's indent rather than at column zero.
fn write_unparsed_interpolation<'a>(value: &'a str, f: &mut VueFormatter<'_, 'a>) {
    let (body, ended_with_line) = without_trailing_line(value);
    let mut lines: Vec<&str> = body.split('\n').collect();
    // A value ending in a blank line would otherwise emit a break of its own
    // and then the closing one, and the printer starts a new line only once
    // per line of output — the two would collapse and the blank line vanish.
    // One `empty_line` says what both were for.
    let ends_blank = ended_with_line && lines.last() == Some(&"");
    if ends_blank {
        lines.pop();
    }

    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            write!(f, literal_line_break());
        }
        if !line.is_empty() {
            write!(f, text(line));
        }
    }

    if ends_blank {
        write!(f, empty_line());
    } else if ended_with_line {
        write!(f, hard_line_break());
    }
}

/// Split off a trailing newline and the horizontal whitespace after it.
fn without_trailing_line(value: &str) -> (&str, bool) {
    let head = value.trim_end_matches([' ', '\t', '\r', '\u{c}']);
    match head.strip_suffix('\n') {
        Some(rest) => (rest, true),
        None => (value, false),
    }
}

/// A child document's IR, written once into whatever position the layout puts
/// it. The builders take their content by reference and may format it more
/// than once while measuring, so the elements move out on the first write.
struct Doc<'a>(std::cell::RefCell<Option<ArenaVec<'a, FormatElement<'a>>>>);

impl<'a> oxc_formatter_core::Format<'a, crate::context::VueFormatContext<'a>> for Doc<'a> {
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        if let Some(elements) = self.0.borrow_mut().take() {
            f.write_elements(elements);
        }
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
    dispatch_with(language, source, snippet, None, f)
}

/// Like [`dispatch`], for a `<script>` block: the same request, plus the one
/// thing a block knows that a fragment does not — that it lives in a component
/// file rather than a file of its own. See [`ScriptInComponentFile`].
pub fn dispatch_script<'a>(
    language: &'static str,
    source: &str,
    snippet: &str,
    f: &mut VueFormatter<'_, 'a>,
) -> Option<Fragment<'a>> {
    dispatch_with(language, source, snippet, Some(&ScriptInComponentFile), f)
}

fn dispatch_with<'a>(
    language: &'static str,
    source: &str,
    snippet: &str,
    parent_context: Option<&dyn std::any::Any>,
    f: &mut VueFormatter<'_, 'a>,
) -> Option<Fragment<'a>> {
    if source.trim().is_empty() {
        return None;
    }
    let response = f.session().dispatch(DispatchRequest {
        language,
        text: source,
        input_kind: InputKind::Fragment,
        parent_context,
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
