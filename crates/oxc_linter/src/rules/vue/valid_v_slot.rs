use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, ForStatementLeft, Statement};
use oxc_ast_visit::Visit;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Attribute, Directive, Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        directive_modifiers_span, directive_value_missing, element_name_eq_lower, get_directive,
        has_directive, is_custom_component,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn owner_must_be_custom_element_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "'v-slot' directive must be owned by a custom element, but '{name}' is not."
    ))
    .with_help("Move `v-slot` onto (or into) a component, not a native element.")
    .with_label(span)
}

fn named_slot_must_be_on_template_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Named slots must use '<template>' on a custom element.")
        .with_help("Wrap the content in `<template #name>...</template>`.")
        .with_label(span)
}

fn default_slot_must_be_on_template_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Default slot must use '<template>' on a custom element when there are other named slots.",
    )
    .with_help("Wrap the default content in `<template #default>...</template>` too.")
    .with_label(span)
}

fn disallow_duplicate_slots_on_element_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("An element cannot have multiple 'v-slot' directives.")
        .with_help("Keep only one `v-slot` per element.")
        .with_label(span)
}

fn disallow_duplicate_slots_on_children_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "An element cannot have multiple '<template>' elements which are distributed to the same slot.",
    )
    .with_help("Give each `<template>` a distinct slot name, or merge their content.")
    .with_label(span)
}

fn disallow_argument_use_slot_params_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Dynamic argument of 'v-slot' directive cannot use that slot parameter.")
        .with_help("The dynamic argument is evaluated in the outer scope, before the slot's own scope exists.")
        .with_label(span)
}

fn disallow_any_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-slot' directive doesn't support any modifier.")
        .with_help("Remove the modifier, or enable the `allowModifiers` option.")
        .with_label(span)
}

fn require_attribute_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-slot' directive on a custom element requires that attribute value.")
        .with_help("Give the default slot shorthand a value, e.g. `v-slot=\"props\"`, or move it onto a `<template>`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ValidVSlot {
    /// Whether a modifier is allowed on a *named* (has an argument) `v-slot`
    /// directive (e.g. `v-slot:foo.bar`). A modifier on a default slot
    /// (no argument) is always rejected. Default `false`.
    allow_modifiers: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-slot` directives in Vue `<template>` blocks:
    /// `v-slot` must be owned by a custom component (directly, or via a
    /// `<template>` child of one), a named slot must use `<template>`,
    /// sibling `<template>`s can't be distributed to the same slot,
    /// a dynamic argument can't reference the slot's own scope
    /// parameters, modifiers aren't supported (unless allowed via the
    /// `allowModifiers` option, and even then only on named slots), and a
    /// default slot placed directly on a component requires a value.
    ///
    /// ### Why is this bad?
    ///
    /// Each of these shapes either fails to compile, silently distributes
    /// content to the wrong (or no) slot, or creates a naming collision
    /// where only one of several intended slots actually renders.
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// The `v-for`-aware duplicate-slot check
    /// (`disallowDuplicateSlotsOnChildren`, when the competing `<template>`s
    /// use a *dynamic* argument that reads their own `v-for`'s iteration
    /// variables, e.g. `<template v-for="(key, value) in xxxx" #[key]>`)
    /// mirrors upstream's positional variable correspondence and iterator
    /// equality (which slot of the `v-for` pattern each *referenced*
    /// variable came from, and whether the two `v-for`s iterate the same
    /// source) via `oxc_parser`, but approximates upstream's structural
    /// `equalTokens` comparison of the iterator expression with a trimmed
    /// raw-text comparison instead of true token equality. This
    /// under-approximates (whitespace/formatting differences in an
    /// otherwise-identical iterator expression could be missed) rather than
    /// over-reports.
    ///
    /// `disallowArgumentUseSlotParams` (a dynamic argument using the slot's
    /// own scope parameter, e.g. `<template #[data]="{data}">`) is detected
    /// by parsing both the argument and the value pattern with `oxc_parser`
    /// and checking for a name in common, rather than upstream's precise
    /// scope-based reference resolution; a coincidental name collision with
    /// an unrelated outer-scope variable would be a false positive here,
    /// but this combination (`v-slot` with a *dynamic* argument at all) is
    /// itself rare.
    ///
    /// `requireAttributeValue` additionally treats a value that fails to
    /// parse as any expression at all (e.g. a comment-only `v-slot="/**/"`)
    /// as missing, mirroring upstream's `isEmptyExpressionValueDirective`
    /// fallback; it does not reproduce that fallback's own token-scanning
    /// mechanism, only its observable effect on this class of input.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-slot="{ data }">{{ data }}</div>
    ///   <MyComponent v-slot:named="{ data }">{{ data }}</MyComponent>
    ///   <MyComponent v-slot>content</MyComponent>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent v-slot="{ data }">{{ data }}</MyComponent>
    ///   <MyComponent>
    ///     <template #default>default</template>
    ///     <template #named>named</template>
    ///   </MyComponent>
    /// </template>
    /// ```
    ValidVSlot,
    vue,
    correctness,
    config = ValidVSlot,
    version = "1.77.0",
    short_description = "Enforce valid `v-slot` directives.",
);

impl Rule for ValidVSlot {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for ValidVSlot {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        check_children(self, nodes, None, ctx);
    }
}

/// Depth-first walk that threads each element's parent alongside it — needed
/// to compute a `v-slot`'s "owner" element (its parent, when it sits on a
/// `<template>`; itself otherwise).
fn check_children<'a>(
    rule: &ValidVSlot,
    nodes: &[Node<'a>],
    parent: Option<&Element<'a>>,
    ctx: &mut VueTemplateContext<'a>,
) {
    for node in nodes {
        if let Node::Element(element) = node {
            check_element(rule, element, parent, ctx);
            check_children(rule, &element.children, Some(element), ctx);
        }
    }
}

fn check_element<'a>(
    rule: &ValidVSlot,
    element: &Element<'a>,
    parent: Option<&Element<'a>>,
    ctx: &mut VueTemplateContext<'a>,
) {
    for attribute in &element.attributes {
        let Some(directive) = &attribute.directive else { continue };
        if directive.name != "slot" {
            continue;
        }

        let is_default_slot = directive
            .argument
            .as_ref()
            .is_none_or(|argument| !argument.dynamic && argument.text == "default");

        // eslint-plugin-vue's `VDocumentFragment` short-circuit, specialized:
        // a non-`<template>` element's owner is always itself (never
        // `None`); a `<template>` at the very root of the `<template>`
        // block has, in upstream's tree, the SFC's own `<template>` tag as
        // its real owner — a genuine `VElement` this fork's template pass
        // doesn't materialize (it parses only that tag's *content*, so
        // there's no `Element` to point at for it). That owner is
        // deterministically a native `<template>` (never a custom
        // component), so the primary `ownerMustBeCustomElement` check still
        // fires for it; checks that would need to inspect the (synthetic)
        // owner's *other* children (duplicate-slot detection) aren't
        // reproduced for this one root-level case.
        let owner_candidate =
            if element_name_eq_lower(element, "template") { parent } else { Some(element) };
        let Some(owner) = owner_candidate else {
            ctx.diagnostic(owner_must_be_custom_element_diagnostic("template", attribute.span));
            continue;
        };
        let owner_is_element = std::ptr::eq(owner, element);

        if !is_custom_component(owner) {
            ctx.diagnostic(owner_must_be_custom_element_diagnostic(owner.name, attribute.span));
        }
        if !is_default_slot && !element_name_eq_lower(element, "template") {
            ctx.diagnostic(named_slot_must_be_on_template_diagnostic(attribute.span));
        }

        let child_groups = slot_directive_groups(owner);
        if owner_is_element && !child_groups.is_empty() {
            ctx.diagnostic(default_slot_must_be_on_template_diagnostic(attribute.span));
        }

        let own_slot_directives = get_directives_named(element, "slot");
        if own_slot_directives.len() >= 2 && !std::ptr::eq(own_slot_directives[0], attribute) {
            ctx.diagnostic(disallow_duplicate_slots_on_element_diagnostic(attribute.span));
        }

        // `ownerElement === parentElement` is exactly the `element.name ==
        // "template"` case: `owner` is defined as `parent` there, and as
        // `element` itself (never equal to `parent`) otherwise.
        if element_name_eq_lower(element, "template") {
            let v_for = get_directive(element, "for", None);
            let current_vars = v_for_vars(attribute, v_for);
            let same_slot_groups = filter_same_slot(
                &child_groups,
                attribute,
                current_vars.as_ref(),
                ctx.source_text(),
            );
            let in_first_group = same_slot_groups.first().is_some_and(|group| {
                group.iter().any(|(_, candidate)| std::ptr::eq(*candidate, attribute))
            });
            if same_slot_groups.len() >= 2 && !in_first_group {
                ctx.diagnostic(disallow_duplicate_slots_on_children_diagnostic(attribute.span));
            }
            if v_for.is_some() && current_vars.is_none() {
                ctx.diagnostic(disallow_duplicate_slots_on_children_diagnostic(attribute.span));
            }
        }

        if is_using_scope_var(attribute, directive) {
            ctx.diagnostic(disallow_argument_use_slot_params_diagnostic(attribute.span));
        }

        if has_invalid_modifiers(directive, rule.allow_modifiers) {
            ctx.diagnostic(disallow_any_modifier_diagnostic(directive_modifiers_span(
                attribute,
                ctx.source_text(),
            )));
        }

        if owner_is_element && is_default_slot && v_slot_value_missing(attribute) {
            ctx.diagnostic(require_attribute_value_diagnostic(attribute.span));
        }
    }
}

fn get_directives_named<'e, 'a>(element: &'e Element<'a>, name: &str) -> Vec<&'e Attribute<'a>> {
    element
        .attributes
        .iter()
        .filter(|attribute| {
            attribute.directive.as_ref().is_some_and(|directive| directive.name == name)
        })
        .collect()
}

/// eslint-plugin-vue `getSlotDirectivesOnChildren` + `iterateChildElementsChains`:
/// group `owner`'s children into "connected" runs of `v-if`/`v-else-if`/
/// `v-else` siblings (a run of one for any other element; whitespace-only
/// text is transparent, anything else breaks the chain), then keep, per
/// group, the `slot` directives of whichever `<template>` children in it
/// carry one.
fn slot_directive_groups<'e, 'a>(
    owner: &'e Element<'a>,
) -> Vec<Vec<(&'e Element<'a>, &'e Attribute<'a>)>> {
    let mut groups = Vec::new();
    for chain in child_element_chains(&owner.children) {
        let slot_directives: Vec<(&Element<'a>, &Attribute<'a>)> = chain
            .into_iter()
            .filter(|element| element_name_eq_lower(element, "template"))
            .filter_map(|element| {
                get_directive(element, "slot", None).map(|attribute| (element, attribute))
            })
            .collect();
        if !slot_directives.is_empty() {
            groups.push(slot_directives);
        }
    }
    groups
}

fn child_element_chains<'e, 'a>(children: &'e [Node<'a>]) -> Vec<Vec<&'e Element<'a>>> {
    let mut chains: Vec<Vec<&Element<'a>>> = Vec::new();
    let mut current: Vec<&Element<'a>> = Vec::new();
    let mut v_if_open = false;
    for child in children {
        match child {
            Node::Element(element) => {
                let connected = if has_directive(element, "if", None) {
                    v_if_open = true;
                    false
                } else if has_directive(element, "else-if", None) {
                    let connected = v_if_open;
                    v_if_open = true;
                    connected
                } else if has_directive(element, "else", None) {
                    let connected = v_if_open;
                    v_if_open = false;
                    connected
                } else {
                    v_if_open = false;
                    false
                };
                if connected {
                    current.push(element);
                } else {
                    if !current.is_empty() {
                        chains.push(std::mem::take(&mut current));
                    }
                    current = vec![element];
                }
            }
            Node::Text(text) if text.value.trim().is_empty() => {}
            _ => v_if_open = false,
        }
    }
    if !current.is_empty() {
        chains.push(current);
    }
    chains
}

/// eslint-plugin-vue's `getNormalizedName`: `"default"` when there's no
/// argument; the argument's own text when there are no modifiers; otherwise
/// the raw source from the argument's start through the end of the key
/// (i.e. including the modifiers), so e.g. `v-slot:foo` and `v-slot:foo.bar`
/// are deliberately treated as *different* slot names.
fn normalized_slot_name<'a>(attribute: &Attribute<'a>, source_text: &'a str) -> &'a str {
    let directive = attribute.directive.as_ref().expect("slot directive");
    let Some(argument) = &directive.argument else { return "default" };
    if directive.modifiers.is_empty() {
        argument.text
    } else {
        let start = argument.span.start as usize;
        let end = attribute.name_span.end as usize;
        &source_text[start..end]
    }
}

/// eslint-plugin-vue `filterSameSlot`: within each sibling group, keep the
/// members whose normalized name matches `current`'s, and — when either
/// side uses `v-for` iteration variables in a dynamic argument — whose
/// `v-for` usage is equivalent to `current`'s (see [`equal_v_for_vars`]).
/// Drops any group left empty.
fn filter_same_slot<'e, 'a>(
    groups: &[Vec<(&'e Element<'a>, &'e Attribute<'a>)>],
    current: &Attribute<'a>,
    current_vars: Option<&VSlotForVars>,
    source_text: &'a str,
) -> Vec<Vec<(&'e Element<'a>, &'e Attribute<'a>)>> {
    let current_name = normalized_slot_name(current, source_text);
    groups
        .iter()
        .map(|group| {
            group
                .iter()
                .copied()
                .filter(|(candidate_element, candidate_attribute)| {
                    if normalized_slot_name(candidate_attribute, source_text) != current_name {
                        return false;
                    }
                    let candidate_for = get_directive(candidate_element, "for", None);
                    let candidate_vars = v_for_vars(candidate_attribute, candidate_for);
                    match (current_vars, &candidate_vars) {
                        (None, None) => true,
                        (Some(a), Some(b)) => equal_v_for_vars(a, b),
                        _ => false,
                    }
                })
                .collect::<Vec<_>>()
        })
        .filter(|group: &Vec<_>| !group.is_empty())
        .collect()
}

/// eslint-plugin-vue's `isUsingScopeVar`: does the `v-slot`'s *dynamic*
/// argument reference a name the directive's own value pattern declares?
/// (e.g. `<template #[data]="{data}">` — `data` isn't defined yet when the
/// argument is evaluated). Approximated by parsing both sides with
/// `oxc_parser` and checking for a name in common, rather than upstream's
/// precise reference-resolution against the value's own scope — see this
/// rule's doc comment for the false-positive tradeoff.
fn is_using_scope_var<'a>(attribute: &Attribute<'a>, directive: &Directive<'a>) -> bool {
    let Some(argument) = &directive.argument else { return false };
    let Some(value) = &attribute.value else { return false };
    if !argument.dynamic {
        return false;
    }
    let Some(inner) = dynamic_argument_inner(argument.text) else { return false };
    let declared = pattern_binding_names(value.text);
    if declared.is_empty() {
        return false;
    }
    let referenced = expression_reference_names(inner);
    referenced.iter().any(|name| declared.contains(name))
}

/// `hasInvalidModifiers`: with `allowModifiers`, only a modifier on a
/// *default* slot (no argument) is invalid; without it, any modifier is.
fn has_invalid_modifiers(directive: &Directive<'_>, allow_modifiers: bool) -> bool {
    if directive.modifiers.is_empty() {
        return false;
    }
    if allow_modifiers { directive.argument.is_none() } else { true }
}

/// `!node.value || isEmptyValueDirective(...) || isEmptyExpressionValueDirective(...)`:
/// missing/empty text (`directive_value_missing`), or present text that has
/// no real (non-comment, non-whitespace) content at all — e.g. a
/// comment-only `v-slot="/**/"`. `isEmptyExpressionValueDirective` detects
/// this upstream via a token-store scan (zero tokens between the value's
/// delimiters); this reproduces its *observable effect* on this class of
/// input via a small comment/whitespace scanner instead of a real
/// tokenizer. Note this is deliberately narrower than "fails to parse as an
/// expression": `v-slot="."` has real content (a `.` token) and is *not*
/// treated as missing, even though `.` alone isn't a valid expression —
/// matching upstream (verified: its own `valid-v-model`-style
/// "parsing-error.vue" test for this exact value is a *pass* case).
fn v_slot_value_missing(attribute: &Attribute<'_>) -> bool {
    if directive_value_missing(attribute) {
        return true;
    }
    let value = attribute.value.as_ref().expect("checked above");
    has_no_real_content(value.text)
}

/// Whether `text` contains nothing but whitespace and `/* ... */` / `// ...`
/// comments.
fn has_no_real_content(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\n' | b'\r' => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'*') => match text[index + 2..].find("*/") {
                Some(offset) => index += 2 + offset + 2,
                None => return true,
            },
            b'/' if bytes.get(index + 1) == Some(&b'/') => index = bytes.len(),
            _ => return false,
        }
    }
    true
}

fn dynamic_argument_inner(text: &str) -> Option<&str> {
    text.strip_prefix('[').and_then(|rest| rest.strip_suffix(']'))
}

/// A `v-for`'s alias pattern's *used* variables, from the perspective of one
/// `v-slot` directive on the same `<template>`, plus enough of the pattern's
/// own shape (top-level `value`/`key`/`index` slots) to check positional
/// correspondence against another such directive — eslint-plugin-vue's
/// `VSlotVForVariables` plus what `equalVSlotVForVariables` needs from it.
struct VSlotForVars {
    /// Top-level `v-for` alias slots (`value`, `key`, `index`, in that
    /// order); `None` for a slot that's absent, a hole, or not a plain
    /// identifier (this rule only needs to recognize plain-identifier
    /// slots, matching every upstream test case for this check).
    slots: Vec<Option<String>>,
    /// The names among `slots` that the dynamic argument actually
    /// references — `getUsingIterationVars`.
    used: Vec<String>,
    /// The `v-for`'s iterator (right-hand side) text, trimmed.
    iterator_text: String,
}

/// `getVSlotVForVariableIfUsingIterationVars`: `None` unless `v_for` is
/// present with a value, the `v-slot`'s argument is *dynamic*, and that
/// argument references at least one of `v_for`'s own top-level aliases.
fn v_for_vars<'a>(v_slot: &Attribute<'a>, v_for: Option<&Attribute<'a>>) -> Option<VSlotForVars> {
    let v_for = v_for?;
    let for_value = v_for.value.as_ref()?;
    let directive = v_slot.directive.as_ref()?;
    let argument = directive.argument.as_ref()?;
    if !argument.dynamic {
        return None;
    }
    let inner = dynamic_argument_inner(argument.text)?;

    let slots = for_alias_slots(for_value.text);
    let referenced = expression_reference_names(inner);
    let used: Vec<String> =
        slots.iter().flatten().filter(|name| referenced.contains(name)).cloned().collect();
    if used.is_empty() {
        return None;
    }
    let iterator_text = find_for_separator(for_value.text)
        .map_or(String::new(), |(_, end)| for_value.text[end..].trim().to_string());
    Some(VSlotForVars { slots, used, iterator_text })
}

/// `equalVSlotVForVariables`: the two `v-for`s must use the same *number* of
/// iteration variables and iterate the same source (the latter approximated
/// by trimmed raw-text equality of the iterator, see this rule's doc
/// comment), and every one of `a`'s *used* aliases must correspond — same
/// top-level slot position, same name — to one of `b`'s, falling back to a
/// plain name match against `b`'s used set. Deliberately asymmetric (mirrors
/// upstream): this must be called as `equal(current, candidate)`.
fn equal_v_for_vars(a: &VSlotForVars, b: &VSlotForVars) -> bool {
    // `a.variables.length !== b.variables.length` — upstream's first guard.
    // Without it, a *subset* of one side's used variables can spuriously
    // "cover" a positionally-matching prefix of the other side's larger used
    // set: e.g. `v-for="(key, value) in xxxx" #[key+value]` (both aliases
    // used) vs `v-for="(key) in xxxx" #[key+value]` (only `key` is its own
    // alias; `value` resolves to something outside this `v-for` entirely) —
    // the second's single-element `used` set trivially "matches" the
    // first's `key` slot, which the final `a.used.iter().all(...)` check
    // alone can't catch when comparing in that direction (confirmed against
    // a live eslint-plugin-vue run: reports nothing for this pair).
    if a.used.len() != b.used.len() {
        return false;
    }
    if a.iterator_text != b.iterator_text {
        return false;
    }
    let mut checked: Vec<&str> = Vec::new();
    let len = a.slots.len().min(b.slots.len());
    for index in 0..len {
        let a_var = a.slots[index].as_deref().filter(|name| a.used.iter().any(|used| used == name));
        let b_var = b.slots[index].as_deref().filter(|name| b.used.iter().any(|used| used == name));
        match (a_var, b_var) {
            (Some(a_name), Some(b_name)) => {
                if a_name != b_name {
                    return false;
                }
                checked.push(a_name);
            }
            (None, None) => {}
            _ => return false,
        }
    }
    a.used
        .iter()
        .all(|name| checked.contains(&name.as_str()) || b.used.iter().any(|used| used == name))
}

/// Collects every `IdentifierReference`/`BindingIdentifier` name a visited
/// subtree contains — used both for "which names does this expression
/// reference" and "which names does this binding pattern declare",
/// depending on which root is visited (an expression only ever contains the
/// former, a pattern only the latter).
struct NameCollector {
    names: Vec<String>,
}

impl<'a> Visit<'a> for NameCollector {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.push(it.name.as_str().to_string());
    }

    fn visit_binding_identifier(&mut self, it: &oxc_ast::ast::BindingIdentifier<'a>) {
        self.names.push(it.name.as_str().to_string());
    }
}

fn expression_reference_names(text: &str) -> Vec<String> {
    let allocator = Allocator::new();
    let snippet = format!("({text});");
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let Some(Statement::ExpressionStatement(statement)) = parser_ret.program.body.first() else {
        return Vec::new();
    };
    let mut collector = NameCollector { names: Vec::new() };
    collector.visit_expression(&statement.expression);
    collector.names
}

/// The names a `v-slot` value *pattern* declares (e.g. `{data}` declares
/// `data`), by parsing it the same way a destructured function parameter
/// would be: `function __f(<pattern>) {}`.
fn pattern_binding_names(text: &str) -> Vec<String> {
    let allocator = Allocator::new();
    let snippet = format!("function __f({text}) {{}}");
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let Some(Statement::FunctionDeclaration(function)) = parser_ret.program.body.first() else {
        return Vec::new();
    };
    let Some(parameter) = function.params.items.first() else { return Vec::new() };
    let mut collector = NameCollector { names: Vec::new() };
    collector.visit_binding_pattern(&parameter.pattern);
    collector.names
}

/// Mirrors vue-eslint-parser's `ALIAS_ITERATOR` regex — the first (leftmost)
/// whole-word `in`/`of` immediately preceded by whitespace or `)`. Copied
/// from `valid-v-for`'s `find_for_separator` (kept local, see
/// `valid-v-model`'s copy of the same helper for why).
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

/// A `v-for` value's top-level alias *slots* (`value`, `key`, `index`, in
/// pattern order) — `Some(name)` for a plain-identifier slot, `None`
/// otherwise (absent, a hole, or a nested destructuring pattern this
/// positional check doesn't need to look inside). Uses the same
/// parse-as-a-real-`for`-statement mechanism as `valid-v-for`'s
/// `check_for_value` / `valid-v-model`'s `for_alias_names`, silently
/// returning nothing on any parse failure.
fn for_alias_slots(raw: &str) -> Vec<Option<String>> {
    let Some((sep_start, sep_end)) = find_for_separator(raw) else { return Vec::new() };
    let aliases_raw = &raw[..sep_start];
    if aliases_raw.trim().is_empty() {
        return Vec::new();
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
        return Vec::new();
    }
    let left = match parser_ret.program.body.first() {
        Some(Statement::ForInStatement(statement)) => &statement.left,
        Some(Statement::ForOfStatement(statement)) => &statement.left,
        _ => return Vec::new(),
    };
    let ForStatementLeft::VariableDeclaration(declaration) = left else { return Vec::new() };
    let Some(declarator) = declaration.declarations.first() else { return Vec::new() };
    let BindingPattern::ArrayPattern(array_pattern) = &declarator.id else { return Vec::new() };

    array_pattern
        .elements
        .iter()
        .map(|element| match element {
            Some(BindingPattern::BindingIdentifier(ident)) => Some(ident.name.as_str().to_string()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::ValidVSlot;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (
                r"<template><MyComponent><Template #foo>x</Template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-slot="{data}">{{data}}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-slot:default="{data}">{{data}}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent #default="{data}">{{data}}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot>default</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template #default>default</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template #named>named</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template #default>default</template><template #named>named</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template #one>one</template><template #two>two</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template #one>one</template><template #two>two</template><template #[one]>one</template><template #[two]>two</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-if="condition" #one>foo</template><template v-else #one>bar</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-if="c1" #one>foo</template><template v-else-if="c2" #one>bar</template><template v-else #one>baz</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key, value) in xxxx" #[key]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key, value) in xxxx" #[key]>{{value}}</template><template v-for="(key, value) in yyyy" #[key]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template #[key]>{{value}}</template><template v-for="(key, value) in yyyy" #[key]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(value, key) in xxxx" #[key]>{{value}}</template><template v-for="(key, value) in xxxx" #[key]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key) in xxxx" #[key+value]>{{value}}</template><template v-for="(key, value) in xxxx" #[key+value]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Same pair, DOM order reversed (larger used-variable set
            // first, smaller second): a regression case for a missed
            // `a.variables.length !== b.variables.length` guard in
            // `equal_v_for_vars`. Without that guard, the second
            // (single-variable) `v-slot` spuriously "matched" the first
            // (two-variable) one via the position-loop's partial
            // correspondence, and — because it isn't in the first matching
            // group — got wrongly flagged as a duplicate. Confirmed against
            // a live eslint-plugin-vue run: reports nothing for this pair.
            (
                r#"<template><MyComponent><template v-for="(key, value) in xxxx" #[key+value]>{{value}}</template><template v-for="(key) in xxxx" #[key+value]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:foo.bar></template></MyComponent></template>",
                Some(json!([{ "allowModifiers": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:foo></template><template v-slot:foo.bar></template><template v-slot:foo.baz></template><template v-slot:foo.bar.baz></template></MyComponent></template>",
                Some(json!([{ "allowModifiers": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // svg
            (
                r#"<template><svg><MyComponent v-slot="slotProps"><MyChildComponent :thing="slotProps.thing" /></MyComponent></svg></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // parsing error: no report at all.
            (
                r#"<template><MyComponent v-slot="."><div /></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("parsing-error.vue")),
            ),
            // Dynamic argument that does NOT reference the value pattern's
            // own names: not a scope-parameter collision.
            (
                r#"<template><MyComponent><template v-slot:[foo]="{data}"></template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Verify location.
            (
                r#"<template><div v-slot="{data}">{{data}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><template v-slot:named></template></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-slot:named="{data}">{{data}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div><template v-slot></template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-slot:named="{data}">{{data}}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-slot="{data}">{{data}}<template #named></template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Verify duplication.
            (
                r#"<template><MyComponent v-slot="{data}" v-slot:one>{{data}}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:one v-slot:two></template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:one>1st</template><template v-slot:one>2nd</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:[one]>1st</template><template v-slot:[one]>2nd</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot>1st</template><template v-slot>2nd</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot>1st</template><template v-slot:default>2nd</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-if="c1" v-slot:one>1st</template><template v-slot:one>2nd</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-if="c1" v-slot:one>1st</template><template v-else v-slot:one>2nd</template><template v-slot:one>3rd</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key, value) in definition" v-slot:one>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key, value) in definition" v-slot:[one]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key, value) in xxxx" v-slot:key>{{value}}</template><template v-for="(key, value) in yyyy" v-slot:key>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key, value) in xxxx" v-slot:[key]>{{value}}</template><template v-for="(key, value) in xxxx" v-slot:[key]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent><template v-for="(key) in xxxx" v-slot:[key]>{{value}}</template><template v-for="(key, value) in xxxx" v-slot:[key]>{{value}}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:foo.bar></template><template v-slot:foo.bar></template></MyComponent></template>",
                Some(json!([{ "allowModifiers": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Verify arguments.
            (
                r#"<template><MyComponent><template v-slot:[data]="{data}"></template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Verify modifiers.
            (
                r#"<template><MyComponent v-slot.foo="{data}">{{data}}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot.foo></template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot:foo.bar></template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><MyComponent><template v-slot.foo></template></MyComponent></template>",
                Some(json!([{ "allowModifiers": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Verify value.
            (
                r"<template><MyComponent v-slot>content</MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-slot="/**/"><div /></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("comment-value1.vue")),
            ),
            (
                r#"<template><MyComponent v-slot="" ><div /></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("empty-value.vue")),
            ),
        ];

        Tester::new(ValidVSlot::NAME, ValidVSlot::PLUGIN, pass, fail).test_and_snapshot();
    }
}
