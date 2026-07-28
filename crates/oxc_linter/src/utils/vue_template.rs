//! Shared helpers for Vue `<template>` rules.
//!
//! Extracted from `rules/vue/require_v_for_key.rs` and generalized so every
//! template rule shares the same element/attribute/directive lookups instead
//! of re-implementing them.

use oxc_span::Span;
use oxc_vue_parser::ast::{Attribute, Element, Node};

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
// Not yet consumed by `require_v_for_key`/`no_duplicate_attributes`; part of
// the shared helper surface later template rules build on.
#[expect(dead_code)]
pub fn get_attribute<'e, 'a>(element: &'e Element<'a>, name: &str) -> Option<&'e Attribute<'a>> {
    element.attributes.iter().find(|attribute| {
        attribute.directive.is_none() && attribute.name.eq_ignore_ascii_case(name)
    })
}

/// eslint-plugin-vue's `isCustomComponent`: an `is` attribute / `v-bind:is` /
/// `v-is` makes any element a component; otherwise an element is custom when
/// its name is not a well-known HTML/SVG/MathML element. SFC template names
/// are case-sensitive (`<DIV>` resolves as a component in an SFC).
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
