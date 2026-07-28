use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::Node;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{directive_key_span, directive_value_missing, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

/// eslint-plugin-vue `valid-v-on`'s `VALID_MODIFIERS`, copied verbatim from
/// the 10.9.1 source (`rules/valid-v-on.js`).
const VALID_MODIFIERS: &[&str] = &[
    "stop", "prevent", "capture", "self", "ctrl", "shift", "alt", "meta", "native", "once", "left",
    "right", "middle", "passive", "esc", "tab", "enter", "space", "up", "left", "right", "down",
    "delete", "exact",
];

/// Modifiers that make a bare (value-less) `v-on` listener legal on their
/// own — eslint-plugin-vue's `VERB_MODIFIERS`.
const VERB_MODIFIERS: &[&str] = &["stop", "prevent"];

/// `KeyboardEvent.key` alias names — eslint-plugin-vue's `utils/key-aliases`
/// (sourced from the W3C UI Events `KeyboardEvent.key` values spec), copied
/// verbatim including its duplicate entries.
const KEY_ALIASES: &[&str] = &[
    "unidentified",
    "alt",
    "alt-graph",
    "caps-lock",
    "control",
    "fn",
    "fn-lock",
    "meta",
    "num-lock",
    "scroll-lock",
    "shift",
    "symbol",
    "symbol-lock",
    "hyper",
    "super",
    "enter",
    "tab",
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "arrow-up",
    "end",
    "home",
    "page-down",
    "page-up",
    "backspace",
    "clear",
    "copy",
    "cr-sel",
    "cut",
    "delete",
    "erase-eof",
    "ex-sel",
    "insert",
    "paste",
    "redo",
    "undo",
    "accept",
    "again",
    "attn",
    "cancel",
    "context-menu",
    "escape",
    "execute",
    "find",
    "help",
    "pause",
    "select",
    "zoom-in",
    "zoom-out",
    "brightness-down",
    "brightness-up",
    "eject",
    "log-off",
    "power",
    "print-screen",
    "hibernate",
    "standby",
    "wake-up",
    "all-candidates",
    "alphanumeric",
    "code-input",
    "compose",
    "convert",
    "dead",
    "final-mode",
    "group-first",
    "group-last",
    "group-next",
    "group-previous",
    "mode-change",
    "next-candidate",
    "non-convert",
    "previous-candidate",
    "process",
    "single-candidate",
    "hangul-mode",
    "hanja-mode",
    "junja-mode",
    "eisu",
    "hankaku",
    "hiragana",
    "hiragana-katakana",
    "kana-mode",
    "kanji-mode",
    "katakana",
    "romaji",
    "zenkaku",
    "zenkaku-hankaku",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "soft1",
    "soft2",
    "soft3",
    "soft4",
    "channel-down",
    "channel-up",
    "close",
    "mail-forward",
    "mail-reply",
    "mail-send",
    "media-close",
    "media-fast-forward",
    "media-pause",
    "media-play-pause",
    "media-record",
    "media-rewind",
    "media-stop",
    "media-track-next",
    "media-track-previous",
    "new",
    "open",
    "print",
    "save",
    "spell-check",
    "key11",
    "key12",
    "audio-balance-left",
    "audio-balance-right",
    "audio-bass-boost-down",
    "audio-bass-boost-toggle",
    "audio-bass-boost-up",
    "audio-fader-front",
    "audio-fader-rear",
    "audio-surround-mode-next",
    "audio-treble-down",
    "audio-treble-up",
    "audio-volume-down",
    "audio-volume-up",
    "audio-volume-mute",
    "microphone-toggle",
    "microphone-volume-down",
    "microphone-volume-up",
    "microphone-volume-mute",
    "speech-correction-list",
    "speech-input-toggle",
    "launch-application1",
    "launch-application2",
    "launch-calendar",
    "launch-contacts",
    "launch-mail",
    "launch-media-player",
    "launch-music-player",
    "launch-phone",
    "launch-screen-saver",
    "launch-spreadsheet",
    "launch-web-browser",
    "launch-web-cam",
    "launch-word-processor",
    "browser-back",
    "browser-favorites",
    "browser-forward",
    "browser-home",
    "browser-refresh",
    "browser-search",
    "browser-stop",
    "app-switch",
    "call",
    "camera",
    "camera-focus",
    "end-call",
    "go-back",
    "go-home",
    "headset-hook",
    "last-number-redial",
    "notification",
    "manner-mode",
    "voice-dial",
    "t-v",
    "t-v3-d-mode",
    "t-v-antenna-cable",
    "t-v-audio-description",
    "t-v-audio-description-mix-down",
    "t-v-audio-description-mix-up",
    "t-v-contents-menu",
    "t-v-data-service",
    "t-v-input",
    "t-v-input-component1",
    "t-v-input-component2",
    "t-v-input-composite1",
    "t-v-input-composite2",
    "t-v-input-h-d-m-i1",
    "t-v-input-h-d-m-i2",
    "t-v-input-h-d-m-i3",
    "t-v-input-h-d-m-i4",
    "t-v-input-v-g-a1",
    "t-v-media-context",
    "t-v-network",
    "t-v-number-entry",
    "t-v-power",
    "t-v-radio-service",
    "t-v-satellite",
    "t-v-satellite-b-s",
    "t-v-satellite-c-s",
    "t-v-satellite-toggle",
    "t-v-terrestrial-analog",
    "t-v-terrestrial-digital",
    "t-v-timer",
    "a-v-r-input",
    "a-v-r-power",
    "color-f0-red",
    "color-f1-green",
    "color-f2-yellow",
    "color-f3-blue",
    "color-f4-grey",
    "color-f5-brown",
    "closed-caption-toggle",
    "dimmer",
    "display-swap",
    "d-v-r",
    "exit",
    "favorite-clear0",
    "favorite-clear1",
    "favorite-clear2",
    "favorite-clear3",
    "favorite-recall0",
    "favorite-recall1",
    "favorite-recall2",
    "favorite-recall3",
    "favorite-store0",
    "favorite-store1",
    "favorite-store2",
    "favorite-store3",
    "guide",
    "guide-next-day",
    "guide-previous-day",
    "info",
    "instant-replay",
    "link",
    "list-program",
    "live-content",
    "lock",
    "media-apps",
    "media-last",
    "media-skip-backward",
    "media-skip-forward",
    "media-step-backward",
    "media-step-forward",
    "media-top-menu",
    "navigate-in",
    "navigate-next",
    "navigate-out",
    "navigate-previous",
    "next-favorite-channel",
    "next-user-profile",
    "on-demand",
    "pairing",
    "pin-p-down",
    "pin-p-move",
    "pin-p-toggle",
    "pin-p-up",
    "play-speed-down",
    "play-speed-reset",
    "play-speed-up",
    "random-toggle",
    "rc-low-battery",
    "record-speed-next",
    "rf-bypass",
    "scan-channels-toggle",
    "screen-mode-next",
    "settings",
    "split-screen-toggle",
    "s-t-b-input",
    "s-t-b-power",
    "subtitle",
    "teletext",
    "video-mode-next",
    "wink",
    "zoom-toggle",
    "browser-back",
    "browser-forward",
    "channel-down",
    "channel-up",
    "context-menu",
    "eject",
    "end",
    "enter",
    "home",
    "media-fast-forward",
    "media-play",
    "media-play-pause",
    "media-record",
    "media-rewind",
    "media-stop",
    "media-next-track",
    "media-pause",
    "media-previous-track",
    "power",
    "unidentified",
];

/// ECMAScript keywords that are *always* a syntax error when they stand
/// alone as an expression (unlike `this`/`super`/`true`/`false`/`null`,
/// which parse fine on their own, or the strict-mode-only future-reserved
/// words `yield`/`await`/`static`/`public`/`private`/`protected`/
/// `interface`/`package`/`implements`/`let`, which parse as ordinary
/// identifiers in the non-strict function body Vue template expressions run
/// in). Used to approximate eslint-plugin-vue's `!node.value.expression`
/// (an expression parse failure) for the `avoidKeyword` check: since this
/// parser doesn't parse directive values as JS, a bare value whose whole
/// text is one of these words is the only case in which it can *know*, from
/// the raw text alone, that upstream's parse of the value would fail —
/// mirroring the `/^\w+$/`-tested "single failed-to-parse word" case. Any
/// other malformed-but-not-single-word value (e.g. `"1 +"`) also fails to
/// parse upstream, but upstream doesn't report those either (its `if` has no
/// `else`), so treating every non-reserved-word value as fine — as the
/// `run_on_template` check below does — matches observable behavior.
const ALWAYS_RESERVED_WORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "return",
    "switch",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
];

fn unsupported_modifier_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'v-on' directives don't support the modifier '{name}'."))
        .with_help("Remove the modifier, or add it to the rule's `modifiers` option if it is a custom modifier your event handler recognizes.")
        .with_label(span)
}

fn avoid_keyword_diagnostic(value: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Avoid using JavaScript keyword as \"v-on\" value: {value}."))
        .with_help("Use a method reference or an inline statement instead of a bare keyword.")
        .with_label(span)
}

fn expected_value_or_verb_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "'v-on' directives require a value or verb modifier (like 'stop' or 'prevent').",
    )
    .with_help("Give the listener a handler, e.g. `@click=\"onClick\"`, or add `.stop`/`.prevent`.")
    .with_label(span)
}

#[derive(Debug, Clone, Default, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ValidVOn {
    /// Additional modifier names to accept beyond the built-in ones (e.g.
    /// project-specific custom directives layered on `v-on`).
    modifiers: Vec<String>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-on` directives in Vue `<template>` blocks: only
    /// recognized modifiers (event modifiers, key aliases, key codes, single
    /// characters, or the `modifiers` option's custom names), and a listener
    /// must have a value unless it carries a `.stop`/`.prevent` verb
    /// modifier.
    ///
    /// ### Why is this bad?
    ///
    /// An unrecognized modifier is usually a typo and silently does
    /// nothing; a value-less listener with no verb modifier does nothing at
    /// all.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <button @click.bogus="onClick" />
    ///   <button v-on:click />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <button @click="onClick" />
    ///   <button @click.stop />
    ///   <input @keyup.enter="onEnter" />
    ///   <div v-on="listeners" />
    /// </template>
    /// ```
    ValidVOn,
    vue,
    correctness,
    config = ValidVOn,
    version = "1.77.0",
    short_description = "Enforce valid `v-on` directives.",
);

impl Rule for ValidVOn {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for ValidVOn {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "on" {
                    continue;
                }

                for modifier in &directive.modifiers {
                    if !self.is_valid_modifier(modifier) {
                        ctx.diagnostic(unsupported_modifier_diagnostic(
                            modifier,
                            directive_key_span(attribute),
                        ));
                    }
                }

                let has_verb_modifier =
                    directive.modifiers.iter().any(|modifier| VERB_MODIFIERS.contains(modifier));
                if has_verb_modifier {
                    continue;
                }

                if directive_value_missing(attribute) {
                    ctx.diagnostic(expected_value_or_verb_diagnostic(attribute.span));
                } else if let Some(value) = &attribute.value
                    && ALWAYS_RESERVED_WORDS.contains(&value.text.trim())
                {
                    ctx.diagnostic(avoid_keyword_diagnostic(value.text, value.span));
                }
            }
        });
    }
}

impl ValidVOn {
    /// eslint-plugin-vue's `isValidModifier`.
    fn is_valid_modifier(&self, modifier: &str) -> bool {
        VALID_MODIFIERS.contains(&modifier)
            || looks_like_key_code(modifier)
            || modifier.chars().count() == 1
            || KEY_ALIASES.contains(&modifier)
            || self.modifiers.iter().any(|custom| custom == modifier)
    }
}

/// eslint-plugin-vue's `Number.isInteger(Number.parseInt(modifier, 10))`:
/// `parseInt` allows leading whitespace and an optional sign before the
/// first digit run and ignores anything after it, so e.g. `"13"` and
/// `"13abc"` both count (deprecated numeric keyCode modifiers).
fn looks_like_key_code(modifier: &str) -> bool {
    let trimmed = modifier.trim_start();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    digits.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::ValidVOn;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><button v-on:click="onClick" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><button @click="onClick" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Verb modifier alone is enough, no value required.
            (
                r"<template><button @click.stop /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><button @click.prevent /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Key alias modifier.
            (
                r#"<template><input @keyup.enter="onEnter" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Numeric (deprecated keyCode) modifier.
            (
                r#"<template><input @keyup.13="onEnter" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Single-character modifier.
            (
                r#"<template><input @keyup.a="onA" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Spread, no argument.
            (
                r#"<template><div v-on="listeners" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom modifier via option.
            (
                r#"<template><button @click.foo="onClick" /></template>"#,
                Some(json!([{ "modifiers": ["foo"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Unsupported modifier.
            (
                r#"<template><button @click.bogus="onClick" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No value, no verb modifier.
            (
                r"<template><button v-on:click /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><button @click /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty value.
            (
                r#"<template><button @click="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Bare JS keyword as value.
            (
                r#"<template><button @click="delete" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVOn::NAME, ValidVOn::PLUGIN, pass, fail).test_and_snapshot();
    }
}
