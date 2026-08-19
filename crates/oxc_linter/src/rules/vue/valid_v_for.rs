use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, ForStatementLeft, Statement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{AttributeValue, Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        directive_modifiers_span, directive_value_missing, element_name_eq_lower, get_directive,
        is_custom_component, start_tag_span, walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Custom elements in iteration require 'v-bind:key' directives.")
        .with_help("Add a `:key` binding so Vue can track each node's identity.")
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require no argument.")
        .with_help("Remove the argument, e.g. use `v-for=\"item in items\"`.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require no modifier.")
        .with_help("Remove the modifier; `v-for` does not accept any.")
        .with_label(span)
}

fn expected_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-for' directives require that attribute value.")
        .with_help("Give `v-for` an iteration expression, e.g. `v-for=\"item in items\"`.")
        .with_label(span)
}

fn invalid_empty_alias_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Invalid empty alias.")
        .with_help("Give the alias a name, or enable the `allowEmptyAlias` option.")
        .with_label(span)
}

fn invalid_alias_diagnostic(text: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Invalid alias '{text}'."))
        .with_help("Key and index aliases must be plain identifiers.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ValidVFor {
    /// Whether an omitted alias slot (e.g. `(, key) in items`) is allowed.
    /// Default `false`.
    allow_empty_alias: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-for` directives in Vue `<template>` blocks: no
    /// argument, no modifiers, a required value in the `<alias> in
    /// <expression>` / `<alias> of <expression>` form, valid `key`/`index`
    /// aliases, and a `:key` binding on custom elements that appear directly
    /// inside the iteration.
    ///
    /// Native (non-component) elements missing `:key` are not this rule's
    /// concern — that is `vue/require-v-for-key`'s job; this rule only
    /// requires it for custom elements, matching eslint-plugin-vue's split
    /// of the same responsibility across `valid-v-for` and
    /// `require-v-for-key`.
    ///
    /// ### Why is this bad?
    ///
    /// `v-for` accepts none of these variations; using them produces a
    /// template that either fails to compile or iterates incorrectly. An
    /// un-keyed custom component in a loop loses per-item identity across
    /// re-renders.
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream parses the `<alias> in/of <expression>` value through its
    /// own JS parser by rewriting it as `for (let [<alias>] in/of
    /// <expression>);` and forcing the alias list through real `ArrayPattern`
    /// grammar. This rule mirrors that exact mechanism using `oxc_parser`
    /// (see `check_for_value`'s doc comment) rather than reimplementing the
    /// grammar textually, so alias validity (a non-pattern element like a
    /// number or a member expression, or a value that fails to parse at all,
    /// e.g. no top-level ` in `/` of `) matches upstream's parse-or-silently-
    /// ignore behavior. Not reproduced: `isUsingIterationVar` — upstream
    /// additionally checks that a `:key` binding on a `v-for`'d element
    /// actually *references* one of the declared aliases; that needs
    /// resolving identifier references against declared bindings, i.e. real
    /// scope analysis, which isn't available to template rules.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items" v-bind:key.foo="item.id" />
    ///   <div v-for:foo="item in items" />
    ///   <div v-for="(value, , index) in items" />
    ///   <MyRow v-for="item in items" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="item in items" />
    ///   <div v-for="(item, index) in items" />
    ///   <div v-for="(value, key, index) in object" />
    ///   <MyRow v-for="item in items" :key="item.id" />
    /// </template>
    /// ```
    ValidVFor,
    vue,
    correctness,
    config = ValidVFor,
    version = "1.77.0",
    short_description = "Enforce valid `v-for` directives.",
);

impl Rule for ValidVFor {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for ValidVFor {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            let Some(attribute) = get_directive(element, "for", None) else { return };
            let directive = attribute.directive.as_ref().expect("matched by get_directive");

            check_key(element, ctx);

            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            if !directive.modifiers.is_empty() {
                ctx.diagnostic(unexpected_modifier_diagnostic(directive_modifiers_span(
                    attribute,
                    ctx.source_text(),
                )));
            }
            if directive_value_missing(attribute) {
                ctx.diagnostic(expected_value_diagnostic(attribute.span));
                return;
            }

            let value = attribute.value.as_ref().expect("checked by directive_value_missing");
            check_for_value(value, self.allow_empty_alias, ctx);
        });
    }
}

/// eslint-plugin-vue `valid-v-for`'s `checkKey`/`checkChildKey`, minus the
/// `isUsingIterationVar` cross-check (see [`check_for_value`]'s doc comment
/// for why): a keyed element, or one whose `:key` sits on it, is fine;
/// `<template>` (not `<slot>` — unlike `require-v-for-key`, upstream's
/// `valid-v-for` does not special-case `<slot>`) pushes the requirement down
/// to its children; a keyless custom element is reported. Native elements
/// are require-v-for-key's concern, not this rule's.
///
/// A `<template>` child that carries its own `v-for` is skipped here, because
/// the walk in [`ValidVFor::run_on_template`] reaches it on its own and would
/// otherwise report the very same diagnostic at the very same span twice.
/// Upstream's `checkChildKey` skips such a child exactly when its `v-for`
/// *uses* one of the parent's iteration variables ("iterator usage will be
/// checked later by child v-for") — the common
/// `<template v-for="row in rows"><MyComp v-for="c in row.cells"/></template>`
/// shape, for which upstream reports once and so does this. When the child's
/// `v-for` does not reference the parent's alias, upstream falls through and
/// emits a byte-identical duplicate of the report its own child visitor then
/// emits again; deciding which of the two branches applies needs the same
/// reference resolution `isUsingIterationVar` does and this rule doesn't
/// have, so the child is skipped unconditionally. That is never a *lost*
/// diagnostic — the child's own `v-for` visit still reports it — only a
/// dropped duplicate.
fn check_key<'a>(element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
    if get_directive(element, "bind", Some("key")).is_some() {
        return;
    }
    if element_name_eq_lower(element, "template") {
        for child in &element.children {
            if let Node::Element(child_element) = child
                && get_directive(child_element, "for", None).is_none()
            {
                check_key(child_element, ctx);
            }
        }
        return;
    }
    if is_custom_component(element) {
        ctx.diagnostic(require_key_diagnostic(start_tag_span(element, ctx.source_text())));
    }
}

/// The literal prefix this rule wraps the alias list in — see
/// [`check_for_value`].
const FOR_SNIPPET_PREFIX: &str = "for(let [";

/// eslint-plugin-vue's `create`'s v-for handler body from the
/// `expr.type !== "VForExpression"` check onward — reimplemented by actually
/// mirroring how upstream's own parser (`vue-eslint-parser`'s
/// `parseVForExpression`) gets there, rather than inspecting the raw text.
///
/// Upstream rewrites the directive value into `for (let [<aliases>]
/// in/of <expression>);` (dropping one layer of wrapping parens from
/// `<aliases>` first, if present) and parses *that* as a real `for`
/// statement — forcing the alias list through actual `ArrayPattern` grammar.
/// Concretely this means: a non-pattern alias (a number literal, a member
/// expression, …) makes the *entire* parse fail, a hole (`(value, , index)`)
/// is a real array-pattern elision, and a trailing comma
/// (`(item, index,) in items`) does *not* manufacture a phantom element —
/// all exactly like a plain `let [a, b] = x;` would behave. Any parse
/// failure (this rule found no top-level ` in `/` of ` at all, the alias
/// list was blank, or the rewritten `for` statement doesn't parse) is
/// treated exactly like upstream's `if (expr == null) return;`: silently
/// ignored, not reported. (Given the rewrite always produces a
/// `VForExpression`-shaped node on success, upstream's own
/// `expr.type !== "VForExpression"` branch — the source of the old
/// `unexpectedExpression` message — is dead code against the parser
/// version this was verified against, so this doesn't reproduce it either.)
///
/// This parser instance mirrors upstream's mechanism, not its text: it
/// builds a small synthetic snippet and parses that with `oxc_parser`,
/// mapping the resulting `key`/`index` binding spans back into `value`'s
/// original source range for diagnostics.
fn check_for_value<'a>(
    value: &AttributeValue<'a>,
    allow_empty_alias: bool,
    ctx: &mut VueTemplateContext<'a>,
) {
    let raw = value.text;
    let Some((sep_start, sep_end)) = find_for_separator(raw) else { return };

    let aliases_raw = &raw[..sep_start];
    if aliases_raw.trim().is_empty() {
        // A blank *unparenthesized* alias, e.g. `v-for=" in items"`, mirrors
        // `parseVForExpression`'s own `if (!processed.aliases.trim()) return
        // throwEmptyError(...)` guard — which runs unconditionally, before
        // upstream ever builds `for (let [...] in/of ...);` and regardless
        // of whether the (nonexistent, here) alias list would have been
        // parenthesized. That throw is what makes `node.value.expression`
        // null for this input, and `valid-v-for.js`'s handler bails out via
        // `if (expr == null) return;` with **zero** diagnostics — verified
        // by running the real `eslint@9.39.4` + `vue-eslint-parser@10.4.1` +
        // `eslint-plugin-vue@10.9.1` stack against `v-for=" in items"`
        // (`vue/valid-v-for` reports nothing), while the same harness does
        // correctly report `vue/valid-v-for` violations for other inputs
        // (argument/modifier/no-value/missing-key cases all still fire) and
        // does still report `invalidEmptyAlias` for a genuine hole like
        // `(value, , index) in items` — so this isn't upstream silencing
        // everything, just this specific "wholly blank, no parens" shape.
        // Do not remove this early return without re-verifying against the
        // real parser first; the equivalent-looking `elements.first()`
        // check a few lines below is for a *different* case (a parenthesized
        // alias list, e.g. `(, key) in items`, that reaches a real
        // `ArrayPattern` parse) and is not a substitute for this guard.
        return;
    }
    let delimiter = &raw[sep_start..sep_end];
    let iterator_raw = &raw[sep_end..];

    let trimmed = aliases_raw.trim();
    let (inner, inner_raw_offset, has_parens) =
        if trimmed.len() >= 2 && trimmed.starts_with('(') && trimmed.ends_with(')') {
            let leading_ws = aliases_raw.len() - aliases_raw.trim_start().len();
            (&trimmed[1..trimmed.len() - 1], leading_ws + 1, true)
        } else {
            (aliases_raw, 0usize, false)
        };

    let snippet = format!("{FOR_SNIPPET_PREFIX}{inner}]{delimiter}{iterator_raw});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return;
    }
    let left = match parser_ret.program.body.first() {
        Some(Statement::ForInStatement(statement)) => &statement.left,
        Some(Statement::ForOfStatement(statement)) => &statement.left,
        _ => return,
    };
    let ForStatementLeft::VariableDeclaration(declaration) = left else { return };
    let Some(declarator) = declaration.declarations.first() else { return };
    let BindingPattern::ArrayPattern(array_pattern) = &declarator.id else { return };
    let elements = &array_pattern.elements;

    // Upstream is `if (value === null && !shouldAllowEmptyAlias)` on
    // `expr.left[0]` — *strictly* `null`, i.e. a real array-pattern hole
    // (`(, key) in items`, `(,) in items`). An absent first element
    // (`undefined`, i.e. an entirely empty alias list, `() in items`) is a
    // different value and does not report: verified by running
    // eslint-plugin-vue against `v-for="() in items"` (no reports at all)
    // versus `v-for="(,) in items"` (one empty-alias report).
    if matches!(elements.first(), Some(None)) && !allow_empty_alias {
        ctx.diagnostic(invalid_empty_alias_diagnostic(value.span));
    }

    if !has_parens {
        return;
    }

    let prefix_len = u32::try_from(FOR_SNIPPET_PREFIX.len()).unwrap_or(0);
    let inner_raw_offset = u32::try_from(inner_raw_offset).unwrap_or(0);

    let mut check_slot = |slot: Option<&Option<BindingPattern<'_>>>| {
        let Some(slot) = slot else { return };
        match slot {
            None => {
                if !allow_empty_alias {
                    ctx.diagnostic(invalid_empty_alias_diagnostic(value.span));
                }
            }
            Some(BindingPattern::BindingIdentifier(_)) => {}
            Some(pattern) => {
                let pattern_span = pattern.span();
                let raw_start = pattern_span.start - prefix_len + inner_raw_offset;
                let raw_end = pattern_span.end - prefix_len + inner_raw_offset;
                let text = &raw[raw_start as usize..raw_end as usize];
                let span = Span::new(value.span.start + raw_start, value.span.start + raw_end);
                ctx.diagnostic(invalid_alias_diagnostic(text, span));
            }
        }
    };
    check_slot(elements.get(1));
    check_slot(elements.get(2));
}

/// Mirrors vue-eslint-parser's `ALIAS_ITERATOR` regex —
/// `/^([\s\S]*?(?:\s|\)))(\bin\b|\bof\b)([\s\S]*)$/u` — the first (leftmost)
/// whole-word `in`/`of` immediately preceded by whitespace or `)`. Upstream's
/// regex has no bracket/quote awareness, so this doesn't add any either.
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::ValidVFor;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item of items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="(item, index) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="(value, key, index) in object" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Destructured value alias, own shape unchecked.
            (
                r#"<template><div v-for="{ a, b } in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Trailing comma: a real ArrayPattern has exactly 2 elements
            // here, no phantom empty 3rd slot.
            (
                r#"<template><div v-for="(item, index,) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No top-level ` in `/` of ` at all: upstream's parser throws
            // before ever producing an expression, which its own rule
            // logic silently ignores.
            (
                r#"<template><div v-for="items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A blank *unparenthesized* alias (found an ` in `/` of ` but
            // nothing before it): also silently ignored upstream, same
            // `aliases.trim()` guard as the "no separator" case above.
            // Verified directly against real eslint-plugin-vue 10.9.1 +
            // vue-eslint-parser 10.4.1 — reports nothing for this input,
            // while still correctly reporting other violations (see
            // `check_for_value`'s doc comment on the matching early return).
            (
                r#"<template><div v-for=" in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A non-pattern element anywhere in the alias list makes the
            // *whole* parse fail — upstream silently ignores this too.
            (
                r#"<template><div v-for="(item, 1) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="(item, key, foo.bar) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component with a key.
            (
                r#"<template><MyRow v-for="item in items" :key="item.id" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Native element without a key: not this rule's concern.
            (
                r#"<template><div v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Key pushed down through <template>.
            (
                r#"<template><template v-for="item in items"><MyRow :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty key alias allowed via option.
            (
                r#"<template><div v-for="(value, , index) in items" /></template>"#,
                Some(json!([{ "allowEmptyAlias": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A wholly empty *parenthesized* alias list parses to a
            // zero-element `ArrayPattern`, so upstream's `left[0]` is
            // `undefined`, not `null`, and its `value === null` check does
            // not fire — verified against a live eslint-plugin-vue run, which
            // reports nothing at all here.
            (
                r#"<template><div v-for="() in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (
                r#"<template><Template v-for="a in b"><MyComp /></Template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument.
            (
                r#"<template><div v-for:foo="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Modifier.
            (
                r#"<template><div v-for.foo="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No value.
            (r"<template><div v-for /></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Empty value.
            (
                r#"<template><div v-for="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Invalid key alias: a syntactically valid pattern (destructured
            // object) that just isn't a plain Identifier.
            (
                r#"<template><div v-for="(item, {a, b}) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Invalid index alias: same, an array pattern.
            (
                r#"<template><div v-for="(item, key, [a, b]) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty key alias (a real hole), option off.
            (
                r#"<template><div v-for="(value, , index) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty value alias, option off.
            (
                r#"<template><div v-for="(, key) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A lone hole is a real one-element `ArrayPattern` whose only
            // element is `null` — unlike `() in items` above, this *does*
            // report (exactly once).
            (
                r#"<template><div v-for="(,) in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component missing a key.
            (
                r#"<template><MyRow v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component missing a key, pushed down through <template>.
            (
                r#"<template><template v-for="item in items"><MyRow /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `<template v-for>` whose child has its own `v-for` must be
            // reported exactly ONCE (by the child's own `v-for` visit), not
            // once per visit — matching upstream, whose `checkChildKey` skips
            // a child that iterates over the parent's alias.
            (
                r#"<template><template v-for="row in rows"><MyComp v-for="c in row.cells" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVFor::NAME, ValidVFor::PLUGIN, pass, fail).test_and_snapshot();
    }
}
