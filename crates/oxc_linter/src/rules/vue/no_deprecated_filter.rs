use oxc_allocator::Allocator;
use oxc_ast::ast::{Argument, Expression, Statement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn no_deprecated_filter_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Filters are deprecated.")
        .with_help(
            "Vue 3 removed Vue 2's `|` filter syntax; call a method or computed property \
             instead (e.g. `{{ formatted(value) }}` instead of `{{ value | formatted }}`).",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedFilter;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated Vue 2 `|` filter syntax (e.g. `{{ value |
    /// filterName }}`) in `<template>` interpolations and `v-bind`
    /// (`:attr`/`v-bind:attr`) directive values.
    ///
    /// ### Why is this bad?
    ///
    /// Vue 3 removed filters. A `|` left over from a Vue 2 template no
    /// longer calls a registered filter — it's just a JS bitwise-OR
    /// operator applied to whatever expression precedes it, silently
    /// changing the expression's meaning instead of erroring.
    ///
    /// ### Which containers this rule checks
    ///
    /// vue-eslint-parser only builds a filter-sequence node for two specific
    /// containers: `{{ }}` mustache interpolations, and `v-bind`/`:attr`
    /// directive values (verified by reading vue-eslint-parser
    /// 10.4.1's `parseAttributeValue`/`processMustache`, which are the only
    /// two call sites passing `allowFilters: true` to `parseExpression`).
    /// Despite also carrying arbitrary JS expressions, `v-model`, `v-on`
    /// (`@click`), `v-if`, `v-show`, `v-text`, and `v-html` directive values
    /// do **not** get filter-sequence parsing and are not checked here —
    /// verified directly against real eslint-plugin-vue 10.9.1 (`{{ a | b
    /// }}` and `:foo="a | b"` are both reported; `v-model="a | b"`,
    /// `v-on:click="a | b"`, `@click="a | b"`, `v-if="a | b"`, and
    /// `v-show="a | b"` are all silently ignored).
    ///
    /// ### `|` position
    ///
    /// Only a `|` at the *top level* of the raw interpolation/attribute
    /// text counts — one inside a string/template literal, a regex
    /// literal, or nested inside `()`/`[]`/`{}` does not (mirroring
    /// vue-eslint-parser's own text-level `splitFilters`, a fork of Vue 2's
    /// `filter-parser.js`, which runs *before* any JS parsing happens). So
    /// `{{ (a | b) }}` and `{{ foo(a | b) }}` are not reported — verified
    /// against real eslint-plugin-vue. `||` is never treated as a filter
    /// separator either.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>{{ message | capitalize }}</div>
    ///   <div :title="message | capitalize"></div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>{{ capitalize(message) }}</div>
    ///   <div :title="capitalize(message)"></div>
    /// </template>
    /// ```
    NoDeprecatedFilter,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow using deprecated filters syntax.",
);

impl Rule for NoDeprecatedFilter {}

impl VueTemplateRule for NoDeprecatedFilter {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk(nodes, ctx);
    }
}

fn walk<'a>(nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
    for node in nodes {
        match node {
            Node::Interpolation(interpolation) => {
                check(interpolation.expression, interpolation.expression_span, ctx);
            }
            Node::Element(element) => {
                for attribute in &element.attributes {
                    // Only `v-bind`/`:attr` (any argument, static, dynamic,
                    // or none — an object-spread `v-bind="..."` is still
                    // `directive.name == "bind"`) gets filter parsing
                    // upstream; every other directive kind (`model`, `on`,
                    // `if`, `show`, `text`, `html`, …) does not. See this
                    // rule's doc comment for the empirical verification.
                    let is_bind = attribute
                        .directive
                        .as_ref()
                        .is_some_and(|directive| directive.name == "bind");
                    if is_bind && let Some(value) = &attribute.value {
                        check(value.text, value.span, ctx);
                    }
                }
                walk(&element.children, ctx);
            }
            _ => {}
        }
    }
}

/// `text` is the raw interpolation/attribute-value text (untrimmed);
/// `span` is its span in the source.
fn check<'a>(text: &'a str, span: Span, ctx: &mut VueTemplateContext<'a>) {
    let Some(pipe_byte_offset) = find_top_level_pipe(text) else { return };
    let base_code = &text[..pipe_byte_offset];
    if !is_valid_filter_base(base_code) {
        return;
    }
    // eslint-plugin-vue's `VFilterSequenceExpression` spans from the start
    // of the parsed base expression (which begins at its first non-blank
    // character — `parseExpressionBody` naturally skips leading whitespace)
    // to the end of the *last* filter segment as parsed by `parseFilter`,
    // which for a bare filter name ends at its last non-blank character and
    // for a `name(args)` filter ends at the closing `)` — in both cases,
    // exactly the last non-blank character of the whole raw text. So the
    // reported span is simply the raw text trimmed of surrounding
    // whitespace; verified byte-for-byte against real eslint-plugin-vue
    // across bare-identifier, `filter(args)`, multi-filter (`a | b | c`),
    // and padded-whitespace (`{{   a   |   b   }}`) cases — filter segments
    // are *not* independently validated as JS by this rule (upstream builds
    // each filter's "callee" by wrapping its raw text in quotes and parsing
    // that as a string literal, so almost any text becomes a filter name;
    // this rule doesn't need to model that, since it never inspects filter
    // segments, only whether at least one exists and the base parses).
    let Some(trimmed_start) = text.find(|c: char| !c.is_whitespace()) else { return };
    let Some(last_char) = text
        .rfind(|c: char| !c.is_whitespace())
        .and_then(|byte_pos| text[byte_pos..].chars().next().map(|c| byte_pos + c.len_utf8()))
    else {
        return;
    };
    let start = span.start + u32::try_from(trimmed_start).unwrap_or(0);
    let end = span.start + u32::try_from(last_char).unwrap_or(0);
    ctx.diagnostic(no_deprecated_filter_diagnostic(Span::new(start, end)));
}

/// A close port of vue-eslint-parser 10.4.1's `splitFilters` (itself "a fork
/// of [Vue 2's `filter-parser.js`]"), stopping at the *first* top-level `|`
/// instead of collecting every split — this rule only needs to know where
/// the base expression ends, not the individual filter segments (see
/// `check`'s doc comment for why). Tracks single/double/template-string
/// state, a regex-literal heuristic (`validDivisionCharRE`), and
/// `(`/`[`/`{` nesting depth; a `|` only counts when none of those apply and
/// it isn't half of a `||`.
fn find_top_level_pipe(text: &str) -> Option<usize> {
    #[derive(PartialEq, Eq)]
    enum State {
        Code,
        Single,
        Double,
        Template,
        Regex,
    }

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut state = State::Code;
    let mut curly = 0i32;
    let mut square = 0i32;
    let mut paren = 0i32;
    let mut prev_char: Option<char> = None;

    for (index, &(byte_pos, ch)) in chars.iter().enumerate() {
        match state {
            State::Single => {
                if ch == '\'' && prev_char != Some('\\') {
                    state = State::Code;
                }
            }
            State::Double => {
                if ch == '"' && prev_char != Some('\\') {
                    state = State::Code;
                }
            }
            State::Template => {
                if ch == '`' && prev_char != Some('\\') {
                    state = State::Code;
                }
            }
            State::Regex => {
                if ch == '/' && prev_char != Some('\\') {
                    state = State::Code;
                }
            }
            State::Code => {
                let next_char = chars.get(index + 1).map(|&(_, c)| c);
                if ch == '|'
                    && next_char != Some('|')
                    && prev_char != Some('|')
                    && curly == 0
                    && square == 0
                    && paren == 0
                {
                    return Some(byte_pos);
                }
                match ch {
                    '"' => state = State::Double,
                    '\'' => state = State::Single,
                    '`' => state = State::Template,
                    '(' => paren += 1,
                    ')' => paren -= 1,
                    '[' => square += 1,
                    ']' => square -= 1,
                    '{' => curly += 1,
                    '}' => curly -= 1,
                    '/' => {
                        // `validDivisionCharRE = /[\w).+\-_$\]]/u`: a `/`
                        // right after one of these chars (skipping spaces)
                        // is division, not the start of a regex literal.
                        let preceding =
                            chars[..index].iter().rev().map(|&(_, c)| c).find(|&c| c != ' ');
                        let is_division_context = preceding.is_some_and(|c| {
                            c.is_alphanumeric()
                                || matches!(c, ')' | '.' | '+' | '-' | '_' | '$' | ']')
                        });
                        if !is_division_context {
                            state = State::Regex;
                        }
                    }
                    _ => {}
                }
            }
        }
        prev_char = Some(ch);
    }
    None
}

/// Whether `base_code` (the raw text before the first top-level `|`) is
/// something upstream would successfully parse as the base expression of a
/// `VFilterSequenceExpression`. Mirrors vue-eslint-parser's own mechanism —
/// it parses `0(${code})` and requires exactly one, non-spread argument
/// (`parseExpressionBody`'s `!expression` / `SpreadElement` / second-argument
/// checks) — rather than a bare `parse_expression()` call, because the
/// latter doesn't verify the whole input was consumed (e.g. `"a b"` would
/// otherwise parse just `a` and silently ignore the trailing `b`, where
/// upstream's wrap-in-a-call-argument trick turns that into a real syntax
/// error, matching its own silent-on-parse-failure discipline).
fn is_valid_filter_base(base_code: &str) -> bool {
    let snippet = format!("0({base_code});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return false;
    }
    let Some(Statement::ExpressionStatement(statement)) = parser_ret.program.body.first() else {
        return false;
    };
    let Expression::CallExpression(call) = &statement.expression else { return false };
    call.arguments.len() == 1 && !matches!(call.arguments[0], Argument::SpreadElement(_))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedFilter;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><div>{{ a }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `||` is never a filter separator.
            (
                r"<template><div>{{ a || b }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Parenthesized/nested: not at the top level of the raw text.
            (
                r"<template><div>{{ (a | b) }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ foo(a | b) }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo="[a | b]"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `|` inside a string isn't a separator.
            (
                r"<template><div>{{ 'a | b' }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Directives that don't get filter parsing upstream, despite
            // being expression-valued.
            (
                r#"<template><div v-model="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-on:click="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div @click="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-if="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-show="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-text="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A plain (non-directive) attribute is never an expression at
            // all.
            (
                r#"<template><div title="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // An empty/blank base before the `|` fails to parse upstream
            // (silently, no report).
            (
                r"<template><div>{{ | b }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r"<template><div>{{ a | b }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Chained filters.
            (
                r"<template><div>{{ a | b | c }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A filter with arguments.
            (
                r"<template><div>{{ a | b(c) }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `:attr` shorthand.
            (
                r#"<template><div :foo="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-bind:attr` longhand.
            (
                r#"<template><div v-bind:foo="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument-less object-spread `v-bind` is still `bind`.
            (
                r#"<template><div v-bind="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic argument: still `bind`, regardless of the argument
            // itself being dynamic.
            (
                r#"<template><div v-bind:[dyn]="a | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `|` inside a filter's own string argument doesn't confuse
            // the top-level split of the *base*.
            (
                r#"<template><div :foo="a | b('x|y')"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // An object literal closes before the `|`, returning to
            // top-level depth.
            (
                r#"<template><div :foo="{ x: a } | b"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Padded whitespace: the reported span is trimmed.
            (
                r"<template><div>{{   a   |   b   }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoDeprecatedFilter::NAME, NoDeprecatedFilter::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
