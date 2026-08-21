//! Shared helpers for Vue `<template>` rules.
//!
//! Extracted from `rules/vue/require_v_for_key.rs` and generalized so every
//! template rule shares the same element/attribute/directive lookups instead
//! of re-implementing them.

use oxc_allocator::Allocator;
use oxc_ast::{
    AstKind,
    ast::{BindingPattern, Expression, ForStatementLeft, Program, Statement},
};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashSet;
use vue_sfc_parser::ast::{Attribute, Element, Node};

use super::{
    VUE_RESERVED_DEPRECATED_HTML_ELEMENTS, VUE_RESERVED_HTML_ELEMENTS,
    VUE_RESERVED_KEBAB_CASE_ELEMENTS, VUE_RESERVED_SVG_ELEMENTS,
};

/// Whether `element` carries a directive named `name` (e.g. `for` for
/// `v-for`), optionally with a specific static `argument` (e.g. `key` for
/// `v-bind:key` / `:key`). A dynamic argument (`:[key]`) never matches a
/// requested static argument.
pub fn has_directive(element: &Element<'_>, name: &str, argument: Option<&str>) -> bool {
    get_directive(element, name, argument).is_some()
}

/// Like [`has_directive`], but returns the matching [`Attribute`] so callers
/// can reach its span (e.g. for [`directive_key_span`]).
pub fn get_directive<'e, 'a>(
    element: &'e Element<'a>,
    name: &str,
    argument: Option<&str>,
) -> Option<&'e Attribute<'a>> {
    element.attributes.iter().find(|attribute| {
        attribute.directive.as_ref().is_some_and(|directive| {
            directive.name == name
                && match argument {
                    None => true,
                    Some(expected) => directive
                        .argument
                        .as_ref()
                        .is_some_and(|arg| !arg.dynamic && arg.text == expected),
                }
        })
    })
}

/// A plain (non-directive) attribute by name, matched ASCII-case-insensitively
/// like eslint-plugin-vue's `getAttribute` (vue-eslint-parser lowercases
/// attribute names before comparison).
pub fn get_attribute<'e, 'a>(element: &'e Element<'a>, name: &str) -> Option<&'e Attribute<'a>> {
    element.attributes.iter().find(|attribute| {
        attribute.directive.is_none() && attribute.name.eq_ignore_ascii_case(name)
    })
}

/// Whether `element`'s tag name equals `lowercase_name` the way
/// eslint-plugin-vue's `VElement[name='…']` selectors compare it.
///
/// vue-eslint-parser exposes two names per element: `rawName` (as written)
/// and `name` (ASCII-lowercased, per the HTML tag-name matching rules). Every
/// upstream rule that keys off a *native* tag — `<template>`, `<component>`,
/// `<slot>`, … — matches against the lowercased `name`, so `<Template>` and
/// `<TEMPLATE>` are the same element to them. This fork's
/// [`Element::name`](vue_sfc_parser::ast::Element::name) is the *raw* name, so
/// rules mirroring those selectors must fold case here rather than compare
/// with `==`.
///
/// `lowercase_name` is expected to already be lowercase (it always is at the
/// call sites: a literal native tag name).
///
/// The exceptions — rules that must stay case-*sensitive* because upstream
/// reads `rawName` rather than `name` — are `no-textarea-mustache`
/// (`VElement[rawName='textarea']`) and `no-deprecated-scope-attribute`
/// (whose bare-`scope`-to-directive conversion runs through
/// vue-eslint-parser's SFC `getTagName`, which returns `rawName`); neither
/// uses this helper.
pub fn element_name_eq_lower(element: &Element<'_>, lowercase_name: &str) -> bool {
    element.name.eq_ignore_ascii_case(lowercase_name)
}

/// eslint-plugin-vue's `isCustomComponent`: an `is` attribute / `v-bind:is` /
/// `v-is` makes any element a component; otherwise an element is custom when
/// its name is not a well-known HTML/SVG/MathML element. SFC template names
/// are case-sensitive (`<DIV>` resolves as a component in an SFC).
///
/// ### Known deviation from eslint-plugin-vue
///
/// Upstream's `isCustomComponent` falls back to `!isHtmlElementName(name) &&
/// !isSvgElementName(name) && !isMathElementName(name)`, where those three
/// predicates consult only its vendored `html-elements.js` / `svg-elements.js`
/// / `math-elements.js` "well-known" lists — the exact same trio
/// [`is_html_svg_or_math_element_name`] reproduces. This function instead
/// delegates to [`is_reserved_element_name`], which is *wider*: it also
/// accepts `VUE_RESERVED_DEPRECATED_HTML_ELEMENTS` (`<marquee>`, `<param>`,
/// `<blink>`, …) and `VUE_RESERVED_KEBAB_CASE_ELEMENTS`. Those extra names are
/// therefore classified here as native elements while upstream classifies them
/// as *custom components*.
///
/// Consequence: for a handful of tags (`<marquee>`, `<param>`, and the other
/// deprecated/kebab-case reserved names) rules that need to distinguish
/// components from plain elements — see this function's call sites — can
/// disagree with upstream. This is an accepted deviation rather than a bug to
/// be fixed silently: switching to the narrow trio would change behavior for
/// every consumer at once and is left for a follow-up that can verify each
/// rule against real eslint individually.
pub fn is_custom_component(element: &Element<'_>) -> bool {
    let has_is = element.attributes.iter().any(|attribute| {
        if let Some(directive) = &attribute.directive {
            directive.name == "is"
                || (directive.name == "bind"
                    && directive
                        .argument
                        .as_ref()
                        .is_some_and(|arg| !arg.dynamic && arg.text == "is"))
        } else {
            attribute.name.eq_ignore_ascii_case("is")
        }
    });
    if has_is {
        return true;
    }

    !is_reserved_element_name(element.name)
}

/// The tag-name portion of eslint-plugin-vue's `isCustomComponent`: whether
/// `name` matches a reserved (native) HTML/SVG/MathML element name. Factored
/// out of [`is_custom_component`] so callers that need the *name-only*
/// classification — ignoring any `is`/`v-bind:is`/`v-is` attribute — can
/// reuse it. `valid-v-is` needs exactly this: every element it checks
/// necessarily carries a `v-is` attribute (that's what the rule visits), so
/// `is_custom_component` itself would always answer `true` and be useless
/// there.
pub fn is_reserved_element_name(name: &str) -> bool {
    VUE_RESERVED_HTML_ELEMENTS.contains(name)
        || VUE_RESERVED_DEPRECATED_HTML_ELEMENTS.contains(name)
        || VUE_RESERVED_SVG_ELEMENTS.contains(name)
        || VUE_RESERVED_KEBAB_CASE_ELEMENTS.contains(name)
        || MATHML_ELEMENTS.contains(&name)
}

/// eslint-plugin-vue's `isHtmlWellKnownElementName(name) ||
/// isSvgWellKnownElementName(name) || isMathWellKnownElementName(name)` —
/// the *narrower* native-element check used by `no-deprecated-html-element-is`.
///
/// Unlike [`is_reserved_element_name`], this deliberately excludes deprecated
/// HTML elements (`VUE_RESERVED_DEPRECATED_HTML_ELEMENTS`) and the
/// foreign/kebab-case names in `VUE_RESERVED_KEBAB_CASE_ELEMENTS`: verified by
/// diffing eslint-plugin-vue's own vendored `html-elements.js`/
/// `svg-elements.js`/`math-elements.js` lists (what
/// `isHtmlWellKnownElementName`/`isSvgWellKnownElementName`/
/// `isMathWellKnownElementName` actually check) against this crate's
/// `VUE_RESERVED_*` sets — neither the deprecated-HTML nor the kebab-case set
/// has a matching source list upstream, so `no-deprecated-html-element-is`
/// (which calls exactly these three predicates, nothing broader) never
/// considers `<marquee is="...">` or `<font-face is="...">` deprecated the
/// way [`is_reserved_element_name`]-based rules would.
///
/// Exact match, case-sensitive: native element names in a template are
/// conventionally lowercase, and an uppercase/PascalCase tag is a component
/// reference, never a match here — mirroring upstream's raw `Set.has` lookup,
/// which never case-folds.
pub fn is_html_svg_or_math_element_name(name: &str) -> bool {
    VUE_RESERVED_HTML_ELEMENTS.contains(name)
        || VUE_RESERVED_SVG_ELEMENTS.contains(name)
        || MATHML_ELEMENTS.contains(&name)
}

/// Whether `name` is a well-known MathML element — the third of the three
/// lists [`is_html_svg_or_math_element_name`] unions, exposed on its own for
/// the rules that have to tell the namespaces apart.
pub fn is_math_element_name(name: &str) -> bool {
    MATHML_ELEMENTS.contains(&name)
}

/// MathML element names (eslint-plugin-vue checks these alongside HTML/SVG).
const MATHML_ELEMENTS: &[&str] = &[
    "annotation",
    "annotation-xml",
    "maction",
    "math",
    "menclose",
    "merror",
    "mfenced",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "semantics",
];

/// The span of the element's start tag, `<name ...attributes>`; mirrors
/// eslint-plugin-vue reporting on `element.startTag`. Attribute values may
/// contain `>`, so the scan starts after the last attribute.
pub fn start_tag_span(element: &Element<'_>, source_text: &str) -> Span {
    let scan_from =
        element.attributes.last().map_or(element.name_span.end, |attribute| attribute.span.end);
    let bytes = source_text.as_bytes();
    let mut index = scan_from as usize;
    while index < bytes.len() && bytes[index] != b'>' {
        index += 1;
    }
    let end = u32::try_from((index + 1).min(bytes.len())).unwrap_or(element.span.end);
    Span::new(element.span.start, end.min(element.span.end))
}

/// Depth-first pre-order walk over every [`Element`] in `nodes`, descending
/// into every element's children regardless of its kind (callers that need
/// to stop at `<template>`/`<slot>` boundaries — e.g. `v-for`'s `:key`
/// requirement — recurse manually instead).
pub fn walk_elements<'a>(nodes: &[Node<'a>], visit: &mut impl FnMut(&Element<'a>)) {
    for node in nodes {
        if let Node::Element(element) = node {
            visit(element);
            walk_elements(&element.children, visit);
        }
    }
}

/// The span of an attribute's "key" part — its name, argument, and modifiers,
/// excluding `="value"`. eslint-plugin-vue frequently reports on `node.key`
/// rather than the whole attribute/directive node; `Attribute::name_span`
/// already stops before `=`, so this is exactly that span, named to match
/// call sites mirroring eslint's `node.key`.
pub fn directive_key_span(attribute: &Attribute<'_>) -> Span {
    attribute.name_span
}

/// Whether a directive's value is missing or empty, mirroring
/// eslint-plugin-vue's `!node.value || utils.isEmptyValueDirective(node,
/// context)` check used by `valid-v-if`/`valid-v-else-if`/`valid-v-show`/
/// `valid-v-html`/`valid-v-text`. `AttributeValue::text` already excludes the
/// surrounding quotes (see its doc comment), so an eslint value of `""` is
/// exactly an empty `text` here — no extra quote-stripping is needed.
///
/// eslint-plugin-vue's version additionally special-cases values that
/// vue-eslint-parser failed to parse as a JS expression (`value.expression ==
/// null`) by re-checking the raw (quote-stripped, not trimmed) text; since
/// this parser doesn't parse directive expressions at all, that distinction
/// isn't reproducible. Matching upstream's *observable* behavior for the
/// common cases is what this does: `v-if=""` is empty, `v-if="cond"` is not,
/// and (like upstream, whose emptiness check doesn't trim) `v-if="   "` is
/// treated as present rather than empty.
pub fn directive_value_missing(attribute: &Attribute<'_>) -> bool {
    match &attribute.value {
        None => true,
        Some(value) => value.text.is_empty(),
    }
}

/// Like [`walk_elements`], but also gives the visitor the element's sibling
/// list (its parent's children, or the root nodes) and its index within it —
/// needed by rules that inspect "the previous element sibling", e.g.
/// `valid-v-else`/`valid-v-else-if` walking back past `Text`/`Comment`/
/// `Interpolation` nodes to find the nearest preceding `Element`, mirroring
/// eslint-plugin-vue's `utils.prevSibling`.
pub fn walk_elements_with_siblings<'e, 'a>(
    nodes: &'e [Node<'a>],
    visit: &mut impl FnMut(&'e Element<'a>, &'e [Node<'a>], usize),
) {
    for (index, node) in nodes.iter().enumerate() {
        if let Node::Element(element) = node {
            visit(element, nodes, index);
            walk_elements_with_siblings(&element.children, visit);
        }
    }
}

/// The span of the `index`-th modifier (0-based, in [`Directive::modifiers`]
/// source order) within an attribute's raw name text — e.g. the `native` in
/// `v-on:keyup.native`, or the `13` in `@keyup.13.stop`.
///
/// [`Directive::modifiers`] keeps only the modifier text, not its span, so
/// rules that need to report on the modifier's own node (eslint-plugin-vue's
/// `VIdentifier`) rather than the whole directive key — e.g.
/// `no-deprecated-v-on-native-modifier`, `no-deprecated-v-on-number-modifiers`
/// — recompute it here.
///
/// The scan for `.`-delimited segments starts *after* the directive argument,
/// because neither the shorthand prefix nor the argument is modifier
/// territory: `.foo.bogus` (the `.prop` shorthand, argument `foo`) has exactly
/// one modifier, `bogus`, and `:[a.b].sync` (dynamic argument `[a.b]`, dots
/// included) has exactly one, `sync`. Starting at the raw name's beginning
/// instead would count the shorthand's own leading `.` and every dot inside a
/// dynamic argument as a modifier separator. Directives without an argument
/// (`v-for.foo`, `v-model.aaa`, `v-bind.prop`) scan from the beginning, where
/// the first dot is the real separator.
///
/// This indexes raw `.` positions in the post-argument text directly, unlike
/// [`vue_sfc_parser::ast::Directive::modifiers`] itself, which drops empty
/// segments (so `..` in the source doesn't produce an empty modifier). The
/// two agree on well-formed input, but for a name with consecutive dots
/// (e.g. `:foo..bar`) `index`-to-modifier alignment between this function and
/// `Directive::modifiers` can diverge; callers rely on real-world attribute
/// names not doing that.
///
/// `index` is assumed in range (callers get it from iterating
/// `Directive::modifiers` itself).
pub fn directive_modifier_span(attribute: &Attribute<'_>, source_text: &str, index: usize) -> Span {
    let name_span = attribute.name_span;
    let text = &source_text[name_span.start as usize..name_span.end as usize];

    // Everything up to and including the argument is skipped; what remains is
    // exactly `.mod1.mod2…`.
    let scan_start = attribute
        .directive
        .as_ref()
        .and_then(|directive| directive.argument.as_ref())
        .map_or(0, |argument| {
            usize::try_from(argument.span.end.saturating_sub(name_span.start))
                .unwrap_or(0)
                .min(text.len())
        });

    let dot_positions: Vec<usize> = text[scan_start..]
        .char_indices()
        .filter_map(|(byte_index, ch)| (ch == '.').then_some(scan_start + byte_index))
        .collect();

    let start = dot_positions.get(index).map_or(text.len(), |&position| position + 1);
    let end = dot_positions.get(index + 1).copied().unwrap_or(text.len());
    let start = u32::try_from(start).unwrap_or(name_span.end - name_span.start);
    let end = u32::try_from(end).unwrap_or(start);
    Span::new(name_span.start + start, name_span.start + end)
}

/// The span covering *all* of an attribute's modifiers at once — from the
/// first modifier's start through the last one's end, dots in between
/// included.
///
/// This is the `loc` the `valid-v-*` family's single `unexpectedModifier`
/// report uses upstream:
/// `loc: { start: node.key.modifiers[0].loc.start, end: node.key.modifiers.at(-1).loc.end }`
/// (`valid-v-if`/`-else`/`-else-if`/`-show`/`-cloak`/`-once`/`-pre`/`-html`/
/// `-text`/`-for`/`-memo`/`-is`/`-slot`) — as opposed to `valid-v-bind`/
/// `-on`/`-model`, which report once *per* offending modifier and so use
/// [`directive_modifier_span`] directly.
///
/// Returns [`Attribute::name_span`] when there are no modifiers at all; every
/// caller guards on a non-empty modifier list first, matching upstream's
/// `if (lastModifier)` guard.
pub fn directive_modifiers_span(attribute: &Attribute<'_>, source_text: &str) -> Span {
    let count = attribute.directive.as_ref().map_or(0, |directive| directive.modifiers.len());
    if count == 0 {
        return attribute.name_span;
    }
    let first = directive_modifier_span(attribute, source_text, 0);
    let last = directive_modifier_span(attribute, source_text, count - 1);
    Span::new(first.start, last.end)
}

/// The nearest preceding `Element` sibling of `nodes[index]` within `nodes`,
/// skipping over any `Text`/`Interpolation`/`Comment`/`Raw` nodes in between —
/// eslint-plugin-vue's `utils.prevSibling`, which only ever tracks `VElement`
/// siblings and otherwise ignores every other node type outright.
pub fn prev_element_sibling<'e, 'a>(
    nodes: &'e [Node<'a>],
    index: usize,
) -> Option<&'e Element<'a>> {
    nodes[..index]
        .iter()
        .rev()
        .find_map(|node| if let Node::Element(element) = node { Some(element) } else { None })
}

/// Mirrors vue-eslint-parser's `ALIAS_ITERATOR` regex — the first (leftmost)
/// whole-word `in`/`of` immediately preceded by whitespace or `)`. Copied
/// from `valid-v-for`'s `find_for_separator` (see there for the full
/// rationale) — this is the one shared copy, used by rules that only need
/// the split itself (to build a `v-for`'s scope-variable names, or to skip
/// its alias list when checking its *iterator* expression), as opposed to
/// `valid-v-for`/`no-use-v-if-with-v-for`/`no-v-for-template-key-on-child`,
/// which additionally need this parse's own diagnostics or collected names
/// for their own purposes and so keep their own copies (this fork's
/// established convention of duplicating small per-rule helpers).
fn find_for_separator(text: &str) -> Option<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (index, &(byte_pos, _)) in chars.iter().enumerate() {
        let preceded_ok = index > 0 && {
            let previous = chars[index - 1].1;
            previous.is_whitespace() || previous == ')'
        };
        if !preceded_ok {
            continue;
        }
        for keyword in ["in", "of"] {
            if !text[byte_pos..].starts_with(keyword) {
                continue;
            }
            let after = byte_pos + keyword.len();
            let word_boundary_ok = match text[after..].chars().next() {
                None => true,
                Some(next) => !(next.is_alphanumeric() || next == '_' || next == '$'),
            };
            if word_boundary_ok {
                return Some((byte_pos, after));
            }
        }
    }
    None
}

/// The `for (let […] in/of …);` snippet a `v-for` value desugars to — the
/// "reuse real JS grammar" mechanism shared by [`v_for_alias_names`] (which
/// wants the parsed aliases) and [`template_expression_parse_error`] (which
/// wants the parse *errors*), so the alias/iterator split lives in exactly one
/// place.
///
/// `Err` carries vue-eslint-parser's own message for the two cases it rejects
/// before parsing anything: a blank value, and a value with no alias list in
/// front of the `in`/`of` (including one with no `in`/`of` at all, e.g.
/// `v-for="items"` — upstream's `ALIAS_ITERATOR` simply fails to match and it
/// reports the missing alias).
///
/// `Ok` also carries the byte offset, within `raw`, of the text the snippet's
/// [`V_FOR_ALIASES_PREFIX`] is followed by, which is what lets a span parsed
/// out of the snippet be mapped back onto the file.
fn v_for_snippet(raw: &str) -> Result<(String, usize), &'static str> {
    if raw.trim().is_empty() {
        return Err("Expected to be '<alias> in <expression>', but got empty");
    }
    let Some((sep_start, sep_end)) = find_for_separator(raw) else {
        return Err("Expected to be an alias, but got empty");
    };
    let aliases_raw = &raw[..sep_start];
    if aliases_raw.trim().is_empty() {
        return Err("Expected to be an alias, but got empty");
    }
    let delimiter = &raw[sep_start..sep_end];
    let iterator_raw = &raw[sep_end..];

    let trimmed = aliases_raw.trim();
    // `inner` is always a subslice of `raw`, so its offset within `raw` is
    // what maps a parsed alias's span back onto the file — see
    // [`snippet_bindings`].
    let (inner, inner_offset) =
        if trimmed.len() >= 2 && trimmed.starts_with('(') && trimmed.ends_with(')') {
            let leading = aliases_raw.len() - aliases_raw.trim_start().len();
            (&trimmed[1..trimmed.len() - 1], leading + 1)
        } else {
            (aliases_raw, 0)
        };

    // The `\n` puts the closing `)` on its own line so a trailing `//` line
    // comment in the iterator (`v-for="x in xs // note"`) can't comment it out.
    // This is not purely cosmetic for [`v_for_alias_bindings`]: such a value
    // used to fail to parse and yield *no* aliases, and now yields them, so
    // [`walk_nodes_with_scope`] sees a wider (correct) scope for the rules
    // built on it — strictly fewer false positives there.
    let snippet = format!("{V_FOR_ALIASES_PREFIX}{inner}]{delimiter}{iterator_raw}\n);");
    Ok((snippet, inner_offset))
}

/// What [`v_for_snippet`] puts in front of the alias list. Shared with the
/// span mapping so the two can't drift apart.
const V_FOR_ALIASES_PREFIX: &str = "for(let [";

/// Which JavaScript grammar a `<template>` value has to be parsed with, for
/// [`template_expression_parse_error`] — mirroring vue-eslint-parser's
/// `getStandardDirectiveKind` dispatch in `parseAttributeValue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateExpressionKind {
    /// A plain expression: an interpolation's contents, and every directive
    /// value that isn't one of the three below — `v-if`, `v-show`, `v-model`,
    /// `v-html`, `v-text`, `v-memo`, `:bind`, argument-less `v-on="{ … }"`,
    /// custom directives, … (upstream's fall-through to `parseExpression`).
    Expression,
    /// `v-on:x` / `@x` — an inline *statement list*, not an expression
    /// (`@click="a(); b()"` is valid), parsed as a function body the way
    /// upstream's `parseVOnExpressionBody` does.
    OnStatements,
    /// `v-for` — `<alias(es)> in/of <iterator>`.
    For,
    /// `v-slot` / `#` / `slot-scope` / `scope` — a destructuring *pattern*,
    /// parsed as a function parameter list.
    SlotScope,
}

/// The first parse error from parsing `text` as `kind`, or `None` when it
/// parses cleanly — the error channel `vue_sfc_parser` doesn't have.
///
/// Every other expression helper here (and in the individual rules) bails
/// *silently* on a parse failure, which is safe but leaves a broken expression
/// completely unreported; `vue/no-parsing-error` calls this to be the one
/// place that surfaces it, mirroring vue-eslint-parser pushing a `ParseError`
/// into `templateBody.errors`.
///
/// The returned message is the raw parser message with any trailing `.`
/// stripped, matching how `no-parsing-error` interpolates it.
///
/// ### Deviations from vue-eslint-parser
///
/// - Each wrapper puts `text` on its own line, so a trailing `//` line comment
///   inside a template expression can't comment out the wrapper's own closing
///   token. Upstream inlines the code and *does* report those; erring toward
///   fewer false positives is the deliberate choice here.
/// - Upstream's expression wrapper is `0(<text>)`, which additionally rejects
///   a spread (`...a`, via `throwUnexpectedTokenError`) and a top-level comma
///   (`a, b`, likewise). Of those two only the comma is missed here: the
///   parenthesised wrapper parses `a, b` clean as a sequence expression,
///   whereas a parenthesised spread is a parse error in its own right (it can
///   only be an arrow-function parameter list, so the parser demands a `=>`)
///   and is therefore still reported, just with a different message. The
///   comma case is under-reporting, not a false positive.
/// - `v-on` is always parsed as a statement list. Upstream first regex-tests
///   for a function expression / simple path and parses those as an
///   expression instead; every such value is also a valid statement, so the
///   only observable difference is which message a broken one gets.
pub fn template_expression_parse_error(text: &str, kind: TemplateExpressionKind) -> Option<String> {
    match kind {
        TemplateExpressionKind::Expression => snippet_parse_error(&format!("(\n{text}\n);")),
        TemplateExpressionKind::OnStatements => {
            snippet_parse_error(&format!("void function($event) {{\n{text}\n}};"))
        }
        TemplateExpressionKind::SlotScope => snippet_parse_error(&format!("(\n{text}\n) => 0;")),
        TemplateExpressionKind::For => match v_for_snippet(text) {
            Ok((snippet, _)) => snippet_parse_error(&snippet),
            Err(message) => Some(message.to_string()),
        },
    }
}

/// The first `oxc_parser` diagnostic message from parsing `snippet`, or `None`
/// when it parses cleanly.
fn snippet_parse_error(snippet: &str) -> Option<String> {
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, snippet, SourceType::ts()).parse();
    match parser_ret.diagnostics.first() {
        Some(diagnostic) => {
            let message = diagnostic.message.as_ref();
            Some(message.strip_suffix('.').unwrap_or(message).to_string())
        }
        // A panic without a diagnostic shouldn't happen, but it is still a
        // parse failure and must not be reported as success.
        None => parser_ret.panicked.then(|| "Unexpected token".to_string()),
    }
}

/// One scope variable a `<template>` element declares — vue-eslint-parser's
/// `VElement.variables` — as the declared name plus the **file-absolute** span
/// of the binding identifier that introduces it.
///
/// [`walk_nodes_with_scope`] needs only the names, and derives them from
/// [`element_own_scope_bindings`] so there is one implementation;
/// `no-template-shadow` reports *at* the declaration, which is what the span
/// is for.
#[derive(Debug, Clone)]
pub struct ScopeBinding {
    pub name: String,
    pub span: Span,
    /// Which declaration introduced it — vue-eslint-parser's
    /// `VVariable.kind`, which `no-unused-vars` groups by.
    pub kind: ScopeBindingKind,
    /// Whether the name sits inside an object or array pattern rather than
    /// being an alias in its own right — upstream's `isDestructuringVar`. A
    /// destructured name can be removed on its own, so it is reported even
    /// when a later sibling is used.
    pub destructured: bool,
}

/// The two ways a `<template>` element can declare a scope variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeBindingKind {
    /// A `v-for` alias.
    VFor,
    /// A `v-slot` / `slot-scope` / `scope` parameter.
    Scope,
}

/// The `v-for` aliases declared by a `v-for="<aliases> in/of <expr>"` value,
/// via the same parse-as-a-real-`for`-statement mechanism as `valid-v-for`'s
/// `check_for_value` (see there for the full rationale) — see
/// [`find_for_separator`]'s doc comment for why this particular copy is shared
/// rather than duplicated. `base` is the file offset `raw` starts at. Silently
/// returns nothing on any parse failure, matching this fork's established
/// silent-on-parse-failure discipline for template expression parsing.
fn v_for_alias_bindings(raw: &str, base: u32) -> Vec<ScopeBinding> {
    let Ok((snippet, alias_offset)) = v_for_snippet(raw) else { return Vec::new() };
    snippet_bindings(
        &snippet,
        V_FOR_ALIASES_PREFIX.len(),
        base,
        alias_offset,
        ScopeBindingKind::VFor,
        |program| {
            let left = match program.body.first() {
                Some(Statement::ForInStatement(statement)) => &statement.left,
                Some(Statement::ForOfStatement(statement)) => &statement.left,
                _ => return Vec::new(),
            };
            let ForStatementLeft::VariableDeclaration(declaration) = left else {
                return Vec::new();
            };
            let Some(declarator) = declaration.declarations.first() else { return Vec::new() };
            let BindingPattern::ArrayPattern(array_pattern) = &declarator.id else {
                return Vec::new();
            };
            let mut identifiers = Vec::new();
            for pattern in array_pattern.elements.iter().flatten() {
                collect_binding_identifiers(pattern, &mut identifiers);
            }
            identifiers
        },
    )
}

/// Parse `snippet`, hand the program to `bindings_of`, and map every span it
/// returns back onto the file.
///
/// Each of these snippets is `<prefix><some text from the file><suffix>`, so a
/// span inside that text is at `base + text_offset + (span - prefix_len)` in
/// the file, where `text_offset` is where the text begins within the attribute
/// value (a `v-for`'s alias list begins after its `(`, a slot-scope pattern
/// after the whitespace trimmed off its front).
fn snippet_bindings(
    snippet: &str,
    prefix_len: usize,
    base: u32,
    text_offset: usize,
    kind: ScopeBindingKind,
    bindings_of: impl for<'p> FnOnce(&Program<'p>) -> Vec<(String, Span, bool)>,
) -> Vec<ScopeBinding> {
    let (Ok(prefix_len), Ok(text_offset)) = (u32::try_from(prefix_len), u32::try_from(text_offset))
    else {
        return Vec::new();
    };
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let identifiers = bindings_of(&parser_ret.program);
    // One value binding the same name twice (`v-for="(a, a) in xs"`,
    // `v-slot="{ a, b: a }"`) is an early error in the grammar each snippet
    // borrows — a duplicate lexical declaration, and a duplicate parameter
    // name in strict mode — so espree rejects the value outright and
    // vue-eslint-parser gives that element NO variables at all (the error
    // surfaces through `no-parsing-error` instead). `oxc_parser` reports early
    // errors from the semantic pass rather than the parse, which these
    // snippets don't run, so the duplicate is caught here instead.
    if identifiers
        .iter()
        .enumerate()
        .any(|(index, (name, _, _))| identifiers[..index].iter().any(|(seen, _, _)| seen == name))
    {
        return Vec::new();
    }
    let start = base + text_offset;
    identifiers
        .into_iter()
        .map(|(name, span, destructured)| ScopeBinding {
            name,
            span: Span::new(
                start + span.start.saturating_sub(prefix_len),
                start + span.end.saturating_sub(prefix_len),
            ),
            kind,
            destructured,
        })
        .collect()
}

/// Every identifier a binding pattern *binds*, in source order, spanned within
/// the snippet it was parsed from. Only bound names are collected: a default
/// value's own expression (e.g. `someGlobal` in `{ b: c = someGlobal }`) is a
/// reference rather than a declaration, so only an assignment pattern's left
/// (binding) side is recursed into.
fn collect_binding_identifiers(pattern: &BindingPattern<'_>, out: &mut Vec<(String, Span, bool)>) {
    collect_binding_identifiers_inner(pattern, false, out);
}

/// `destructured` becomes true as soon as the walk descends through an object
/// or array pattern, which is upstream's `isDestructuringVar` seen from the
/// other end: it walks *up* from the identifier and asks whether a `Property`
/// or `ArrayPattern` comes before the `v-for` / slot-scope expression.
fn collect_binding_identifiers_inner(
    pattern: &BindingPattern<'_>,
    destructured: bool,
    out: &mut Vec<(String, Span, bool)>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            out.push((ident.name.as_str().to_string(), ident.span, destructured));
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_binding_identifiers_inner(&property.value, true, out);
            }
            if let Some(rest) = &object.rest {
                collect_binding_identifiers_inner(&rest.argument, true, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for pattern in array.elements.iter().flatten() {
                collect_binding_identifiers_inner(pattern, true, out);
            }
            if let Some(rest) = &array.rest {
                collect_binding_identifiers_inner(&rest.argument, true, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_binding_identifiers_inner(&assignment.left, destructured, out);
        }
    }
}

/// Depth-first walk over every [`Node`] in `nodes` — not just [`Element`]s,
/// since callers that also need [`vue_sfc_parser::ast::Interpolation`]s (every
/// expression-inspecting rule this was built for: `this-in-template`,
/// `no-deprecated-dollar-listeners-api`, `no-deprecated-dollar-scopedslots-api`)
/// match on the node kind themselves — together with the set of scope-variable
/// names visible at that node: every name declared by the node's own element
/// (its `v-for` alias(es), and/or its `v-slot`/shorthand `#`/deprecated
/// `slot-scope`/`scope` destructured parameter(s)) or any ancestor element's
/// same.
///
/// Mirrors vue-eslint-parser's per-`VElement` scope stack (`node.variables`):
/// both `v-for` aliases and slot-scope destructured parameters are tracked,
/// the latter via [`slot_scope_names`] — parsing the directive/attribute
/// value as a real function-parameter pattern (`({ <value> }) => 0`), the
/// same "reuse real JS grammar" mechanism `v_for_alias_names` uses for
/// `v-for`, rather than reimplementing destructuring-pattern parsing by hand.
pub fn walk_nodes_with_scope<'e, 'a>(
    nodes: &'e [Node<'a>],
    scope: &FxHashSet<String>,
    visit: &mut impl FnMut(&'e Node<'a>, &FxHashSet<String>),
) {
    for node in nodes {
        let Node::Element(element) = node else {
            visit(node, scope);
            continue;
        };
        let own_names = element_own_scope_names(element);
        if own_names.is_empty() {
            visit(node, scope);
            walk_nodes_with_scope(&element.children, scope, visit);
        } else {
            let mut child_scope = scope.clone();
            child_scope.extend(own_names);
            visit(node, &child_scope);
            walk_nodes_with_scope(&element.children, &child_scope, visit);
        }
    }
}

/// Every scope-variable name declared by `element` itself (not inherited
/// from an ancestor): its `v-for` alias(es), plus its slot-scope destructured
/// parameter(s) — see [`walk_nodes_with_scope`]'s doc comment.
fn element_own_scope_names(element: &Element<'_>) -> FxHashSet<String> {
    element_own_scope_bindings(element).into_iter().map(|binding| binding.name).collect()
}

/// Every scope variable declared by `element` itself (not inherited from an
/// ancestor), in source order and with spans: its `v-for` alias(es) followed
/// by its slot-scope destructured parameter(s).
///
/// This is vue-eslint-parser's `VElement.variables` for one element, which is
/// what a rule walking the scope stack itself (`no-template-shadow`) reports
/// on; [`walk_nodes_with_scope`] is the same information collapsed to the set
/// of names visible at a node, for the rules that only need to know what is
/// shadowed.
///
/// A name declared twice by the same element (`v-for="(a, a) in xs"`) is
/// listed twice — the duplicate is exactly what `no-template-shadow` reports,
/// so it must not be collapsed here.
pub fn element_own_scope_bindings(element: &Element<'_>) -> Vec<ScopeBinding> {
    let mut bindings = Vec::new();
    if let Some(value) =
        get_directive(element, "for", None).and_then(|attribute| attribute.value.as_ref())
    {
        bindings.extend(v_for_alias_bindings(value.text, value.span.start));
    }
    if let Some((pattern_text, base)) = slot_scope_pattern(element) {
        bindings.extend(slot_scope_bindings(pattern_text, base));
    }
    bindings
}

/// The raw pattern text — and the file offset it starts at — of whichever
/// slot-scope-establishing directive or
/// (deprecated bare) attribute `element` carries, if any: `v-slot`/its
/// shorthand `#` (any argument, static or dynamic — the *pattern* lives in
/// the value, not the argument), the deprecated `slot-scope` attribute (any
/// element — vue-eslint-parser's bare-attribute-to-directive conversion for
/// `slot-scope` isn't restricted to `<template>`, unlike `scope`; see
/// `no_deprecated_slot_scope_attribute.rs`), or the deprecated `scope`
/// attribute (`<template>` only; see `no_deprecated_scope_attribute.rs`).
/// Both deprecated attribute names are matched case-SENSITIVELY, exactly
/// like those two rules — vue-eslint-parser's SFC `getTagName` never
/// case-folds either the tag name or the attribute name for this
/// conversion.
fn slot_scope_pattern<'a>(element: &Element<'a>) -> Option<(&'a str, u32)> {
    let pattern = |attribute: &Attribute<'a>| {
        attribute.value.as_ref().map(|value| (value.text, value.span.start))
    };
    if let Some(attribute) = get_directive(element, "slot", None) {
        return pattern(attribute);
    }
    let is_bare = |attribute: &&Attribute<'a>, name: &str| {
        attribute.directive.is_none() && attribute.name == name
    };
    if let Some(attribute) =
        element.attributes.iter().find(|attribute| is_bare(attribute, "slot-scope"))
    {
        return pattern(attribute);
    }
    if element.name == "template"
        && let Some(attribute) =
            element.attributes.iter().find(|attribute| is_bare(attribute, "scope"))
    {
        return pattern(attribute);
    }
    None
}

/// The variables bound by a slot-scope pattern — `v-slot`/`#`/`slot-scope`/
/// `scope`'s value, e.g. `slotProps` or `{ a, b: c = 1, ...rest }` — via the
/// same "parse as real JS grammar" mechanism as [`v_for_alias_bindings`]:
/// wrapped as a single arrow-function parameter (`(<pattern>) => 0`) and
/// parsed with `oxc_parser`, so identifier binding (a bare identifier, an
/// object/array pattern with defaults, aliases (`b: c`), and rest elements)
/// is handled by real `BindingPattern` grammar rather than reimplemented.
/// `base` is the file offset `pattern_text` starts at. Silently empty on any
/// parse failure or a blank pattern, matching this fork's established
/// silent-on-parse-failure discipline — this also covers a value-less
/// `v-slot`/`slot-scope` (nothing to parse, no names declared).
fn slot_scope_bindings(pattern_text: &str, base: u32) -> Vec<ScopeBinding> {
    let trimmed = pattern_text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let leading = pattern_text.len() - pattern_text.trim_start().len();
    let snippet = format!("{SLOT_SCOPE_PREFIX}{trimmed}) => 0;");
    snippet_bindings(
        &snippet,
        SLOT_SCOPE_PREFIX.len(),
        base,
        leading,
        ScopeBindingKind::Scope,
        |program| {
            let Some(Statement::ExpressionStatement(statement)) = program.body.first() else {
                return Vec::new();
            };
            let Expression::ArrowFunctionExpression(arrow) = &statement.expression else {
                return Vec::new();
            };
            let mut identifiers = Vec::new();
            for parameter in &arrow.params.items {
                collect_binding_identifiers(&parameter.pattern, &mut identifiers);
            }
            identifiers
        },
    )
}

/// What [`slot_scope_bindings`] puts in front of the pattern. Shared with the
/// span mapping so the two can't drift apart.
const SLOT_SCOPE_PREFIX: &str = "(";

/// The `(text, absolute span)` of the JS expression a directive's value
/// represents, for the expression-inspecting template rules
/// (`this-in-template`, `no-deprecated-dollar-listeners-api`,
/// `no-deprecated-dollar-scopedslots-api`) that need to parse it.
///
/// `None` for: a plain (non-directive) attribute (never an expression —
/// this already excludes the deprecated bare `slot-scope`/`scope`
/// attributes, which never carry `Attribute::directive`), a directive with
/// no value, and `v-slot`/its shorthand `#` — those values are destructuring
/// *patterns*, not expressions (e.g. `v-slot="{ msg }"` parsed as an
/// expression would be an `ObjectExpression` with a shorthand property whose
/// value is a *reference* to `msg`, not a declaration of it). Their bound
/// names ARE extracted into scope, just via [`slot_scope_names`] (real
/// parameter-pattern parsing) rather than as a plain expression here — see
/// [`walk_nodes_with_scope`]'s doc comment.
///
/// `v-for`'s value is only ever its *iterator* expression (the part after
/// `in`/`of`) — its alias list is a set of declarations, not a reference,
/// already folded into scope by [`walk_nodes_with_scope`] instead.
pub fn directive_expression<'a>(attribute: &Attribute<'a>) -> Option<(&'a str, Span)> {
    let directive = attribute.directive.as_ref()?;
    if directive.name == "slot" {
        return None;
    }
    let value = attribute.value.as_ref()?;
    if directive.name == "for" {
        let (_, sep_end) = find_for_separator(value.text)?;
        let sep_end = u32::try_from(sep_end).ok()?;
        return Some((
            &value.text[sep_end as usize..],
            Span::new(value.span.start + sep_end, value.span.end),
        ));
    }
    Some((value.text, value.span))
}

/// Every span, within `text`, of a *free* (unresolved — not locally bound
/// within `text` itself, e.g. by a nested function's own parameter or a
/// `let`/`const`) identifier reference named exactly `name`. Built for
/// `no-deprecated-dollar-listeners-api`/`no-deprecated-dollar-scopedslots-api`,
/// whose upstream `VExpressionContainer` handler is exactly "every
/// `$listeners`/`$scopedSlots` *reference* (`reference.variable == null`,
/// i.e. not resolved to a locally-declared variable) in this expression" —
/// full scope resolution via `oxc_semantic` is what makes that distinction
/// (a plain identifier-name walk can't tell a free reference from one bound
/// by e.g. `function click($listeners) { fn($listeners) }`'s own parameter).
/// `v-for` alias / (documented gap: `v-slot`) shadowing from the
/// *template*'s own scope — as opposed to shadowing from within `text`
/// itself — is the caller's job, via [`walk_nodes_with_scope`]'s scope set.
///
/// Parses `text` wrapped in `(<text>);` to dodge the object-literal/
/// block-statement ambiguity at statement position (same trick as
/// `no-use-v-if-with-v-for`'s `expression_reference_names`); returned spans
/// already have that wrapper's leading `(` subtracted back out, so they're
/// relative to `text` itself. Silently empty on any parse failure, matching
/// this fork's established silent-on-parse-failure discipline.
/// Every emit call in a template expression, as the span of the event-name
/// argument (which is what the message points at) and the name. Only a
/// statically known name is returned, matching upstream's `getNameParamNode`.
///
/// `$emit` is always an emit; `binding` additionally names the `<script setup>`
/// binding a `defineEmits()` produced, which the template can call directly.
pub fn template_emit_calls(text: &str, binding: Option<&str>) -> Vec<(Span, String)> {
    let snippet = format!("({text});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let program = allocator.alloc(parser_ret.program);
    let semantic = SemanticBuilder::new_linter().build(program).semantic;
    let mut out = Vec::new();
    for node in semantic.nodes() {
        let AstKind::CallExpression(call) = node.kind() else { continue };
        let is_emit = match call.callee.get_inner_expression() {
            Expression::Identifier(identifier) => {
                identifier.name == "$emit"
                    || binding.is_some_and(|binding| identifier.name == binding)
            }
            Expression::StaticMemberExpression(member) => member.property.name == "$emit",
            _ => continue,
        };
        if !is_emit {
            continue;
        }
        let Some(argument) = call.arguments.first().and_then(|argument| argument.as_expression())
        else {
            continue;
        };
        let Some(name) = crate::utils::literal_element_name(argument.without_parentheses()) else {
            continue;
        };
        let span = argument.span();
        out.push((
            Span::new(span.start.saturating_sub(1), span.end.saturating_sub(1)),
            name.into_owned(),
        ));
    }
    out
}

/// Every span, within `text`, of a *call* whose callee is a free reference
/// named exactly `name` — `foo()` but not `foo`, `a.foo()` or a locally-bound
/// `foo`. The span covers the whole call expression, which is what
/// `no-use-computed-property-like-method` reports.
///
/// Same parse-and-resolve mechanism as [`free_reference_spans`], and the same
/// silent-on-parse-failure discipline; see there for why the wrapper is
/// `(<text>);` and why the returned spans are relative to `text`.
pub fn free_call_spans(text: &str, name: &str) -> Vec<Span> {
    let snippet = format!("({text});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let program = allocator.alloc(parser_ret.program);
    let semantic = SemanticBuilder::new_linter().build(program).semantic;
    let Some(reference_ids) = semantic.scoping().root_unresolved_references().get(name) else {
        return Vec::new();
    };
    let nodes = semantic.nodes();
    reference_ids
        .iter()
        .filter_map(|&reference_id| {
            let node = nodes.get_node(semantic.scoping().get_reference(reference_id).node_id());
            let AstKind::CallExpression(call) = nodes.parent_kind(node.id()) else { return None };
            if call.callee.span() != node.span() {
                return None;
            }
            Some(Span::new(call.span.start.saturating_sub(1), call.span.end.saturating_sub(1)))
        })
        .collect()
}

pub fn free_reference_spans(text: &str, name: &str) -> Vec<Span> {
    let snippet = format!("({text});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let program = allocator.alloc(parser_ret.program);
    let semantic = SemanticBuilder::new_linter().build(program).semantic;
    let Some(reference_ids) = semantic.scoping().root_unresolved_references().get(name) else {
        return Vec::new();
    };
    reference_ids
        .iter()
        .map(|&reference_id| {
            let reference = semantic.scoping().get_reference(reference_id);
            let span = semantic.reference_span(reference);
            Span::new(span.start.saturating_sub(1), span.end.saturating_sub(1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use vue_sfc_parser::{ast::Node, parse_template};

    use super::{directive_modifier_span, directive_modifiers_span};

    /// The source text each of `<div …>`'s attributes' modifier spans covers,
    /// one `Vec` entry per attribute, one inner entry per modifier.
    fn modifier_texts(source: &str) -> Vec<Vec<&str>> {
        let nodes = parse_template(source, 0);
        let Some(Node::Element(element)) = nodes.first() else { panic!("expected an element") };
        element
            .attributes
            .iter()
            .map(|attribute| {
                let count =
                    attribute.directive.as_ref().map_or(0, |directive| directive.modifiers.len());
                (0..count)
                    .map(|index| {
                        let span = directive_modifier_span(attribute, source, index);
                        &source[span.start as usize..span.end as usize]
                    })
                    .collect()
            })
            .collect()
    }

    fn modifiers_text(source: &str) -> &str {
        let nodes = parse_template(source, 0);
        let Some(Node::Element(element)) = nodes.first() else { panic!("expected an element") };
        let attribute = element.attributes.first().expect("expected an attribute");
        let span = directive_modifiers_span(attribute, source);
        &source[span.start as usize..span.end as usize]
    }

    #[test]
    fn modifier_span_skips_the_shorthand_prefix_and_argument() {
        // `.prop` shorthand: the leading `.` introduces the *argument*, not a
        // modifier — verified against eslint-plugin-vue 10.10.0, whose
        // `vue/valid-v-bind` underlines `bogus` here.
        assert_eq!(modifier_texts(r#"<div .foo.bogus="x"/>"#), vec![vec!["bogus"]]);
        assert_eq!(modifier_texts(r#"<div .foo.bogus.baz="x"/>"#), vec![vec!["bogus", "baz"]]);
        // `:` shorthand (regression).
        assert_eq!(modifier_texts(r#"<div :foo.bar="x"/>"#), vec![vec!["bar"]]);
        // Dots *inside* a dynamic argument are part of the argument.
        assert_eq!(modifier_texts(r#"<div .[a.b].sync="x"/>"#), vec![vec!["sync"]]);
        assert_eq!(modifier_texts(r#"<div :[a.b].sync="x"/>"#), vec![vec!["sync"]]);
        // No argument at all: the first dot is the real separator.
        assert_eq!(modifier_texts(r#"<div v-model.aaa="x"/>"#), vec![vec!["aaa"]]);
        assert_eq!(modifier_texts(r#"<div v-for.foo.bar="a in b"/>"#), vec![vec!["foo", "bar"]]);
        // Long forms with an argument.
        assert_eq!(modifier_texts(r#"<div v-on:keyup.native="x"/>"#), vec![vec!["native"]]);
        assert_eq!(modifier_texts(r#"<div @keyup.13.stop="x"/>"#), vec![vec!["13", "stop"]]);
    }

    #[test]
    fn modifiers_span_covers_first_through_last() {
        assert_eq!(modifiers_text(r#"<div v-for.foo.bar="a in b"/>"#), "foo.bar");
        assert_eq!(modifiers_text(r#"<div .foo.bogus.baz="x"/>"#), "bogus.baz");
        assert_eq!(modifiers_text(r#"<div .[a.b].sync="x"/>"#), "sync");
    }
}
