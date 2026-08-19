use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_syntax::identifier::is_identifier_name;
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Attribute, DirectiveShorthand, Node};

use crate::{
    rule::{Rule, TupleRuleConfig},
    utils::{vue_casing, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_longhand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected 'v-bind' before ':'.").with_label(span)
}

fn expected_longhand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected 'v-bind' before ':'.").with_label(span)
}

fn expected_longhand_for_prop_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected 'v-bind:' instead of '.'.").with_label(span)
}

fn expected_shorthand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected same-name shorthand.").with_label(span)
}

fn unexpected_shorthand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected same-name shorthand.").with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BindStyle {
    #[default]
    Shorthand,
    Longform,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SameNameShorthand {
    Always,
    Never,
    #[default]
    Ignore,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct VBindStyleOptions {
    same_name_shorthand: SameNameShorthand,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VBindStyleConfig(BindStyle, VBindStyleOptions);

// Boxed (like `vue/prop-name-casing`'s `PropNameCasing`): keeps this rule's
// own footprint at one pointer (8 bytes) so it doesn't grow `RuleEnum` past
// 16 bytes.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct VBindStyle(Box<VBindStyleConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces `v-bind` directive style: shorthand `:foo` (default) vs.
    /// longform `v-bind:foo`. The `sameNameShorthand` option additionally
    /// enforces (or forbids) the Vue 3.4+ same-name shorthand `:foo` (short
    /// for `:foo="foo"`) when the bound value is just the argument's own
    /// name; it is `"ignore"` (no additional check) by default.
    ///
    /// ### Why is this bad?
    ///
    /// Mixing `:foo` and `v-bind:foo` (or `.foo` and `v-bind:foo.prop`) for
    /// the same binding kind across a codebase is inconsistent and harder to
    /// visually scan.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (default `"shorthand"`):
    /// ```vue
    /// <template>
    ///   <div v-bind:foo="bar" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule (default `"shorthand"`):
    /// ```vue
    /// <template>
    ///   <div :foo="bar" />
    /// </template>
    /// ```
    VBindStyle,
    vue,
    style,
    config = VBindStyleConfig,
    version = "1.77.0",
    short_description = "Enforce `v-bind` directive style.",
);

impl Rule for VBindStyle {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<TupleRuleConfig<Self>>(value).map(TupleRuleConfig::into_inner)
    }
}

impl VueTemplateRule for VBindStyle {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let VBindStyleConfig(style, options) = &*self.0;
        let prefer_shorthand = *style == BindStyle::Shorthand;
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "bind" || directive.argument.is_none() {
                    continue;
                }
                check_same_name(attribute, options.same_name_shorthand, ctx);
                check_style(attribute, prefer_shorthand, ctx);
            }
        });
    }
}

/// eslint-plugin-vue `v-bind-style`'s `checkAttributeStyle`.
fn check_style<'a>(
    attribute: &Attribute<'a>,
    prefer_shorthand: bool,
    ctx: &mut VueTemplateContext<'a>,
) {
    // `directive.shorthand` is `Some(Bind)` for `:foo`, `Some(BindProp)` for
    // `.foo`, and `None` for the literal `v-bind:foo`.
    let Some(directive) = &attribute.directive else { return };
    let is_shorthand_prop = directive.shorthand == Some(DirectiveShorthand::BindProp);
    let is_shorthand = directive.shorthand == Some(DirectiveShorthand::Bind) || is_shorthand_prop;
    if is_shorthand == prefer_shorthand {
        return;
    }
    let diagnostic = if prefer_shorthand {
        unexpected_longhand_diagnostic(attribute.span)
    } else if is_shorthand_prop {
        expected_longhand_for_prop_diagnostic(attribute.span)
    } else {
        expected_longhand_diagnostic(attribute.span)
    };
    ctx.diagnostic(diagnostic);
}

/// eslint-plugin-vue `v-bind-style`'s `checkAttributeSameName`.
fn check_same_name<'a>(
    attribute: &Attribute<'a>,
    same_name_shorthand: SameNameShorthand,
    ctx: &mut VueTemplateContext<'a>,
) {
    if same_name_shorthand == SameNameShorthand::Ignore {
        return;
    }
    let Some(directive) = &attribute.directive else { return };
    let Some(argument) = &directive.argument else { return };
    if argument.dynamic {
        return;
    }

    // eslint-plugin-vue's `isVBindSameNameShorthand`: true exactly when no
    // `="value"` text was written at all — the Vue 3.4+ same-name shorthand
    // `:foo` (implicitly `:foo="foo"`), which this parser represents as a
    // directive with no `value`.
    let is_shorthand_form = attribute.value.is_none();

    // eslint-plugin-vue's `isSameName`: either the shorthand form above
    // (same name by construction), or an explicit value that is a bare
    // identifier matching the argument's name up to kebab/camelCase
    // normalization.
    let is_same_name = is_shorthand_form
        || attribute.value.as_ref().is_some_and(|value| {
            let text = value.text.trim();
            is_identifier_name(text) && kebab_to_camel(argument.text) == kebab_to_camel(text)
        });
    if !is_same_name {
        return;
    }

    let prefer_shorthand = same_name_shorthand == SameNameShorthand::Always;
    if is_shorthand_form == prefer_shorthand {
        return;
    }
    let diagnostic = if prefer_shorthand {
        expected_shorthand_diagnostic(attribute.span)
    } else {
        unexpected_shorthand_diagnostic(attribute.span)
    };
    ctx.diagnostic(diagnostic);
}

/// eslint-plugin-vue `v-bind-style`'s `kebabCaseToCamelCase`.
fn kebab_to_camel(name: &str) -> std::borrow::Cow<'_, str> {
    if vue_casing::is_kebab_case(name) {
        std::borrow::Cow::Owned(vue_casing::camel_case(name))
    } else {
        std::borrow::Cow::Borrowed(name)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::VBindStyle;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Default ("shorthand"): `:foo` is fine.
            (
                r#"<template><div :foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `.prop` shorthand also counts as shorthand.
            (
                r#"<template><div .foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No argument: `v-bind="obj"` is out of scope for this rule.
            (
                r#"<template><div v-bind="obj" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "longform": `v-bind:foo` is fine.
            (
                r#"<template><div v-bind:foo="bar" /></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `sameNameShorthand` defaults to "ignore": mismatched same-name
            // form is not reported.
            (
                r#"<template><div :foo="foo" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `sameNameShorthand: "always"`: shorthand `:foo` (implicit
            // `:foo="foo"`) already satisfies it.
            (
                r"<template><div :foo /></template>",
                Some(json!(["shorthand", { "sameNameShorthand": "always" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `sameNameShorthand: "never"`: explicit `:foo="foo"` already
            // satisfies it.
            (
                r#"<template><div :foo="foo" /></template>"#,
                Some(json!(["shorthand", { "sameNameShorthand": "never" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `sameNameShorthand`: different value name is not "same name"
            // at all, so it's not checked by that half of the rule.
            (
                r#"<template><div :foo="bar" /></template>"#,
                Some(json!(["shorthand", { "sameNameShorthand": "always" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Default ("shorthand"): `v-bind:foo` reported.
            (
                r#"<template><div v-bind:foo="bar" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "longform": shorthand `:foo` reported.
            (
                r#"<template><div :foo="bar" /></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "longform": `.prop` shorthand reported with its own message.
            (
                r#"<template><div .foo="bar" /></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `sameNameShorthand: "always"`: explicit `:foo="foo"` should be
            // the shorthand `:foo` instead.
            (
                r#"<template><div :foo="foo" /></template>"#,
                Some(json!(["shorthand", { "sameNameShorthand": "always" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `sameNameShorthand: "never"`: shorthand `:foo` should be the
            // explicit `:foo="foo"` instead.
            (
                r"<template><div :foo /></template>",
                Some(json!(["shorthand", { "sameNameShorthand": "never" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(VBindStyle::NAME, VBindStyle::PLUGIN, pass, fail).test_and_snapshot();
    }
}
