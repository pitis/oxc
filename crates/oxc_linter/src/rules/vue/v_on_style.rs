use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::{DirectiveShorthand, Node};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn expected_shorthand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected '@' instead of 'v-on:'.").with_label(span)
}

fn expected_longhand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected 'v-on:' instead of '@'.").with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VOnStyleValue {
    #[default]
    Shorthand,
    Longform,
}

// Boxed for consistency with this family's other config types, keeping this
// rule's own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct VOnStyle(Box<VOnStyleValue>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces `v-on` directive style: shorthand `@foo` (default) vs.
    /// longform `v-on:foo`.
    ///
    /// ### Why is this bad?
    ///
    /// Mixing `@foo` and `v-on:foo` for event bindings across a codebase is
    /// inconsistent and harder to visually scan.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (default `"shorthand"`):
    /// ```vue
    /// <template>
    ///   <button v-on:click="onClick" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule (default `"shorthand"`):
    /// ```vue
    /// <template>
    ///   <button @click="onClick" />
    /// </template>
    /// ```
    VOnStyle,
    vue,
    style,
    config = VOnStyleValue,
    version = "1.77.0",
    short_description = "Enforce `v-on` directive style.",
);

impl Rule for VOnStyle {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<VOnStyleValue>>(value)
            .map(|config| Self(Box::new(config.into_inner())))
    }
}

impl VueTemplateRule for VOnStyle {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let prefer_shorthand = *self.0 == VOnStyleValue::Shorthand;
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "on" || directive.argument.is_none() {
                    continue;
                }
                let is_shorthand = directive.shorthand == Some(DirectiveShorthand::On);
                if is_shorthand == prefer_shorthand {
                    continue;
                }
                let diagnostic = if prefer_shorthand {
                    expected_shorthand_diagnostic(attribute.span)
                } else {
                    expected_longhand_diagnostic(attribute.span)
                };
                ctx.diagnostic(diagnostic);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::VOnStyle;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Default ("shorthand"): `@click` is fine.
            (
                r#"<template><button @click="onClick" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No argument: `v-on="handlers"` is out of scope for this rule.
            (
                r#"<template><button v-on="handlers" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic argument shorthand is still shorthand.
            (
                r#"<template><button @[event]="onEvent" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "longform": `v-on:click` is fine.
            (
                r#"<template><button v-on:click="onClick" /></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Default ("shorthand"): `v-on:click` reported.
            (
                r#"<template><button v-on:click="onClick" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "longform": shorthand `@click` reported.
            (
                r#"<template><button @click="onClick" /></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic argument longform still reported under "longform".
            (
                r#"<template><button @[event]="onEvent" /></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(VOnStyle::NAME, VOnStyle::PLUGIN, pass, fail).test_and_snapshot();
    }
}
