use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{directive_modifier_span, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn number_modifier_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'KeyboardEvent.keyCode' modifier on 'v-on' directive is deprecated. Using 'KeyboardEvent.key' instead.",
    )
    .with_help("Replace the numeric keyCode modifier with the named `KeyboardEvent.key` modifier, e.g. `.13` becomes `.enter`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDeprecatedVOnNumberModifiers;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the deprecated numeric (`KeyboardEvent.keyCode`) modifiers
    /// on `v-on` directives (Vue 3.0+ removed them in favor of named
    /// `KeyboardEvent.key` modifiers).
    ///
    /// ### Why is this bad?
    ///
    /// Numeric keyCode modifiers have no effect in Vue 3; a listener relying
    /// on one silently stops firing for that key.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <input @keyup.13="submit" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <input @keyup.enter="submit" />
    /// </template>
    /// ```
    NoDeprecatedVOnNumberModifiers,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow using deprecated number (keyCode) modifiers.",
);

impl Rule for NoDeprecatedVOnNumberModifiers {}

impl VueTemplateRule for NoDeprecatedVOnNumberModifiers {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "on" {
                    continue;
                }
                // eslint-plugin-vue's `Number.isInteger(Number.parseInt(mod.name,
                // 10))`: only the *first* modifier that looks like a keyCode is
                // considered, in source order.
                let Some((index, modifier)) = directive
                    .modifiers
                    .iter()
                    .enumerate()
                    .find(|(_, modifier)| parse_keycode_modifier(modifier).is_some())
                else {
                    continue;
                };
                let key_code = parse_keycode_modifier(modifier).expect("checked by `find` above");
                // Verified against real eslint-plugin-vue: single-digit
                // keyCodes (`.0`–`.9`) are NOT reported, despite some of them
                // (`8` = backspace, `9` = tab) appearing in the upstream
                // keyCode→key table used by the (unimplemented here) fixer —
                // this is upstream's actual, if surprising, `keyCode > 9`
                // condition, copied verbatim.
                if !(0..=9).contains(&key_code) {
                    ctx.diagnostic(number_modifier_diagnostic(directive_modifier_span(
                        attribute,
                        ctx.source_text(),
                        index,
                    )));
                }
            }
        });
    }
}

/// eslint-plugin-vue's `Number.parseInt(modifier, 10)` (guarded by
/// `Number.isInteger` on the result): skips leading whitespace, then an
/// optional `+`/`-` sign, then parses the longest leading run of ASCII
/// digits — any trailing non-digit text (e.g. `"13abc"`) is ignored. Returns
/// `None` when no digit follows the (optional) sign, mirroring
/// `Number.isInteger(NaN) === false`.
fn parse_keycode_modifier(modifier: &str) -> Option<i64> {
    let trimmed = modifier.trim_start();
    let (sign, rest): (i64, &str) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().map(|value| value * sign)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDeprecatedVOnNumberModifiers;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r"<template><input v-on:keyup.page-down='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.page-down='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Single-digit keyCodes are not reported (upstream's real,
            // if surprising, `keyCode > 9` condition).
            (
                r"<template><input v-on:keyup.9='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.0='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.4='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.page-down.native='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.0.native='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r"<template><input v-on:keyup.34='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input v-on:keyup.34.native='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `unknown` isn't a keyCode; `34` (the second modifier) is.
            (
                r"<template><input v-on:keyup.unknown.34='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A literal `.` inside a dynamic argument's brackets is not a
            // modifier boundary.
            (
                r"<template><input v-on:[dynamicArg].34='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><input @keyup.10='onArrowUp'></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoDeprecatedVOnNumberModifiers::NAME,
            NoDeprecatedVOnNumberModifiers::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
