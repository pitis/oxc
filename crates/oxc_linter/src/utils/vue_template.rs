//! Shared helpers for Vue `<template>` rules.
//!
//! Extracted from `rules/vue/require_v_for_key.rs` and generalized so every
//! template rule shares the same element/attribute/directive lookups instead
//! of re-implementing them.

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, ForStatementLeft, Statement};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use oxc_vue_parser::ast::{Attribute, Element, Node};
use rustc_hash::FxHashSet;

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
/// — recompute it here. This scans `attribute.name_span`'s source text for
/// `.`-delimited segments, tracking `[`/`]` depth so a literal `.` inside a
/// dynamic argument (e.g. `:[a.b].sync`) isn't mistaken for a modifier
/// boundary. `index` is assumed in range (callers get it from iterating
/// `Directive::modifiers` itself).
pub fn directive_modifier_span(attribute: &Attribute<'_>, source_text: &str, index: usize) -> Span {
    let name_span = attribute.name_span;
    let text = &source_text[name_span.start as usize..name_span.end as usize];

    let mut depth = 0i32;
    let mut dot_positions = Vec::new();
    for (byte_index, ch) in text.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth -= 1,
            '.' if depth == 0 => dot_positions.push(byte_index),
            _ => {}
        }
    }

    let start = dot_positions.get(index).map_or(text.len(), |&position| position + 1);
    let end = dot_positions.get(index + 1).copied().unwrap_or(text.len());
    let start = u32::try_from(start).unwrap_or(name_span.end - name_span.start);
    let end = u32::try_from(end).unwrap_or(start);
    Span::new(name_span.start + start, name_span.start + end)
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

/// The `v-for` alias *names* declared by a `v-for="<aliases> in/of <expr>"`
/// value, via the same parse-as-a-real-`for`-statement mechanism as
/// `valid-v-for`'s `check_for_value` (see there for the full rationale) —
/// see [`find_for_separator`]'s doc comment for why this particular copy is
/// shared rather than duplicated. Silently returns nothing on any parse
/// failure, matching this fork's established silent-on-parse-failure
/// discipline for template expression parsing.
fn v_for_alias_names(raw: &str) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let Some((sep_start, sep_end)) = find_for_separator(raw) else { return names };
    let aliases_raw = &raw[..sep_start];
    if aliases_raw.trim().is_empty() {
        return names;
    }
    let delimiter = &raw[sep_start..sep_end];
    let iterator_raw = &raw[sep_end..];

    let trimmed = aliases_raw.trim();
    let inner = if trimmed.len() >= 2 && trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        aliases_raw
    };

    let snippet = format!("for(let [{inner}]{delimiter}{iterator_raw});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return names;
    }
    let left = match parser_ret.program.body.first() {
        Some(Statement::ForInStatement(statement)) => &statement.left,
        Some(Statement::ForOfStatement(statement)) => &statement.left,
        _ => return names,
    };
    let ForStatementLeft::VariableDeclaration(declaration) = left else { return names };
    let Some(declarator) = declaration.declarations.first() else { return names };
    let BindingPattern::ArrayPattern(array_pattern) = &declarator.id else { return names };

    for pattern in array_pattern.elements.iter().flatten() {
        collect_binding_names(pattern, &mut names);
    }
    names
}

fn collect_binding_names(pattern: &BindingPattern<'_>, out: &mut FxHashSet<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            out.insert(ident.name.as_str().to_string());
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_binding_names(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for pattern in array.elements.iter().flatten() {
                collect_binding_names(pattern, out);
            }
            if let Some(rest) = &array.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_binding_names(&assignment.left, out);
        }
    }
}

/// Depth-first walk over every [`Node`] in `nodes` — not just [`Element`]s,
/// since callers that also need [`oxc_vue_parser::ast::Interpolation`]s (every
/// expression-inspecting rule this was built for: `this-in-template`,
/// `no-deprecated-dollar-listeners-api`, `no-deprecated-dollar-scopedslots-api`)
/// match on the node kind themselves — together with the set of `v-for` alias
/// names visible at that node: every alias declared by the node's own
/// element (if it carries `v-for`) or any ancestor element's `v-for`.
///
/// Mirrors a deliberately narrowed subset of vue-eslint-parser's per-`VElement`
/// scope stack (`node.variables`): only `v-for` aliases are tracked.
/// `v-slot`/its shorthand `#`/deprecated `slot-scope` destructured parameters
/// (e.g. `<template v-slot="{ item }">`) are NOT folded into scope — see
/// `this-in-template`'s doc comment for the full rationale. A property name
/// that collides with such a slot-scope variable is a rare, documented false
/// positive in the rules built on this helper, not a silent miss.
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
        if let Some(value) =
            get_directive(element, "for", None).and_then(|attribute| attribute.value.as_ref())
        {
            let mut child_scope = scope.clone();
            child_scope.extend(v_for_alias_names(value.text));
            visit(node, &child_scope);
            walk_nodes_with_scope(&element.children, &child_scope, visit);
        } else {
            visit(node, scope);
            walk_nodes_with_scope(&element.children, scope, visit);
        }
    }
}

/// The `(text, absolute span)` of the JS expression a directive's value
/// represents, for the expression-inspecting template rules
/// (`this-in-template`, `no-deprecated-dollar-listeners-api`,
/// `no-deprecated-dollar-scopedslots-api`) that need to parse it.
///
/// `None` for: a plain (non-directive) attribute (never an expression),
/// a directive with no value, and `v-slot`/its shorthand `#`/deprecated
/// `slot-scope` — those values are destructuring *patterns*, not
/// expressions, and (per [`walk_nodes_with_scope`]'s doc comment) this fork
/// doesn't extract their bound names into scope either, so treating the
/// pattern text as a plain expression would be actively wrong (e.g.
/// `v-slot="{ msg }"` parsed as an expression is an `ObjectExpression` with
/// a shorthand property whose value is a *reference* to `msg`, not a
/// declaration of it) rather than just incomplete.
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
