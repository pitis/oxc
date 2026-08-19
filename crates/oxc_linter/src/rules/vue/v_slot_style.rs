use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Attribute, Directive, DirectiveShorthand, Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{element_name_eq_lower, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn expected_shorthand_diagnostic(argument: &str, actual: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Expected '#{argument}' instead of '{actual}'.")).with_label(span)
}

fn expected_longform_diagnostic(argument: &str, actual: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Expected 'v-slot:{argument}' instead of '{actual}'."))
        .with_label(span)
}

fn expected_v_slot_diagnostic(actual: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Expected 'v-slot' instead of '{actual}'.")).with_label(span)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum SlotStyle {
    #[serde(rename = "shorthand")]
    Shorthand,
    #[serde(rename = "longform")]
    Longform,
    #[serde(rename = "v-slot")]
    VSlot,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VSlotStyleOptions {
    #[serde(default = "at_component_default")]
    at_component: SlotStyle,
    #[serde(default = "shorthand_default")]
    default: SlotStyle,
    #[serde(default = "shorthand_default")]
    named: SlotStyle,
}

fn at_component_default() -> SlotStyle {
    SlotStyle::VSlot
}

fn shorthand_default() -> SlotStyle {
    SlotStyle::Shorthand
}

impl Default for VSlotStyleOptions {
    fn default() -> Self {
        Self {
            at_component: at_component_default(),
            default: shorthand_default(),
            named: shorthand_default(),
        }
    }
}

/// eslint-plugin-vue `v-slot-style`'s single positional option, which is
/// either a bare style string (applied to all three slot kinds) or a
/// per-slot-kind object.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VSlotStyleConfig {
    Global(SlotStyle),
    PerKind(VSlotStyleOptions),
}

impl Default for VSlotStyleConfig {
    fn default() -> Self {
        Self::PerKind(VSlotStyleOptions::default())
    }
}

impl VSlotStyleConfig {
    fn resolve(&self) -> VSlotStyleOptions {
        match self {
            Self::Global(style) => {
                VSlotStyleOptions { at_component: *style, default: *style, named: *style }
            }
            Self::PerKind(options) => options.clone(),
        }
    }
}

// Boxed for consistency with this family's other config types, keeping this
// rule's own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct VSlotStyle(Box<VSlotStyleConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces `v-slot` directive style, separately for the default slot on
    /// a component (`atComponent`, default `"v-slot"`), the default slot on
    /// a `<template>` (`default`, default `"shorthand"`), and named slots
    /// (`named`, default `"shorthand"`). A single string applies the same
    /// style to all three.
    ///
    /// ### Why is this bad?
    ///
    /// `v-slot`, `v-slot:name`, and `#name` are three different spellings
    /// for the same concept; picking one style per slot kind and enforcing
    /// it keeps templates consistent.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (defaults):
    /// ```vue
    /// <template>
    ///   <MyComponent v-slot:default="{ item }">{{ item }}</MyComponent>
    ///   <template v-slot:default>Default</template>
    ///   <template v-slot:name>Named</template>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule (defaults):
    /// ```vue
    /// <template>
    ///   <MyComponent v-slot="{ item }">{{ item }}</MyComponent>
    ///   <template>Default</template>
    ///   <template #name>Named</template>
    /// </template>
    /// ```
    VSlotStyle,
    vue,
    style,
    config = VSlotStyleConfig,
    version = "1.77.0",
    short_description = "Enforce `v-slot` directive style.",
);

impl Rule for VSlotStyle {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<VSlotStyleConfig>>(value)
            .map(|config| Self(Box::new(config.into_inner())))
    }
}

impl VueTemplateRule for VSlotStyle {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let options = self.0.resolve();
        walk_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let Some(directive) = &attribute.directive else { continue };
                if directive.name != "slot" {
                    continue;
                }
                check_slot(&options, element, attribute, directive, ctx);
            }
        });
    }
}

/// eslint-plugin-vue `v-slot-style`'s visitor body: compares the expected
/// style (from options, per slot kind) against the actual written style, and
/// reports a mismatch on the key span (name through argument, excluding any
/// `="value"`).
fn check_slot<'a>(
    options: &VSlotStyleOptions,
    element: &Element<'a>,
    attribute: &Attribute<'a>,
    directive: &Directive<'a>,
    ctx: &mut VueTemplateContext<'a>,
) {
    let expected = expected_style(options, element, directive);
    let actual = actual_style(directive);
    if actual == expected {
        return;
    }

    let span = slot_key_span(attribute, directive);
    let argument = directive.argument.as_ref().map_or("default", |argument| argument.text);
    let actual_text = &ctx.source_text()[span.start as usize..span.end as usize];

    let diagnostic = match expected {
        SlotStyle::Shorthand => expected_shorthand_diagnostic(argument, actual_text, span),
        SlotStyle::Longform => expected_longform_diagnostic(argument, actual_text, span),
        SlotStyle::VSlot => expected_v_slot_diagnostic(actual_text, span),
    };
    ctx.diagnostic(diagnostic);
}

/// eslint-plugin-vue `v-slot-style`'s `getExpectedStyle`: an absent argument,
/// or a static argument literally named `default`, is the "default slot"
/// case — `default` on a bare `<template>`, `atComponent` when attached
/// directly to a component (or any other element). Every other (named)
/// argument uses `named`.
fn expected_style<'a>(
    options: &VSlotStyleOptions,
    element: &Element<'a>,
    directive: &Directive<'a>,
) -> SlotStyle {
    let is_default = match &directive.argument {
        None => true,
        Some(argument) => !argument.dynamic && argument.text == "default",
    };
    if is_default {
        if element_name_eq_lower(element, "template") {
            options.default
        } else {
            options.at_component
        }
    } else {
        options.named
    }
}

/// eslint-plugin-vue `v-slot-style`'s `getActualStyle`.
fn actual_style(directive: &Directive<'_>) -> SlotStyle {
    if directive.shorthand == Some(DirectiveShorthand::Slot) {
        SlotStyle::Shorthand
    } else if directive.argument.is_some() {
        SlotStyle::Longform
    } else {
        SlotStyle::VSlot
    }
}

/// The span from the start of the directive's key (`v-slot`/`#`) through the
/// end of its argument, excluding any trailing modifiers and the
/// `="value"` part — matches eslint-plugin-vue's `[name.range[0],
/// (argument || name).range[1]]`.
fn slot_key_span(attribute: &Attribute<'_>, directive: &Directive<'_>) -> Span {
    let end = if let Some(argument) = &directive.argument {
        argument.span.end
    } else {
        // `#` is one byte; the literal `v-slot` is six.
        let prefix_len: u32 =
            if directive.shorthand == Some(DirectiveShorthand::Slot) { 1 } else { 6 };
        attribute.name_span.start + prefix_len
    };
    Span::new(attribute.name_span.start, end)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::VSlotStyle;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Default slot on a component: bare `v-slot` (default
            // `atComponent: "v-slot"`).
            (
                r#"<template><MyComponent v-slot="{ item }">{{ item }}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default slot on `<template>`: no directive at all (default
            // `default: "shorthand"` is satisfied trivially — nothing to
            // report because there's no `v-slot`).
            (
                r"<template><MyComponent><template>Default</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Named slot: `#name` shorthand (default `named: "shorthand"`).
            (
                r#"<template><MyComponent><template #name="{ item }">{{ item }}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Explicit `#default` shorthand on `<template>` also satisfies
            // `default: "shorthand"`.
            (
                r"<template><MyComponent><template #default>Default</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Global string option applied to all three kinds.
            (
                r#"<template><MyComponent v-slot:default="{ item }">{{ item }}</MyComponent><template v-slot:name>Named</template></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Per-kind object overriding just `named`.
            (
                r#"<template><MyComponent><template v-slot:name="{ item }">{{ item }}</template></MyComponent></template>"#,
                Some(json!([{ "named": "longform" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (
                r#"<template><MyComp><Template v-slot:bar="p">{{p}}</Template></MyComp></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default slot on a component: explicit `v-slot:default` should
            // be bare `v-slot`.
            (
                r#"<template><MyComponent v-slot:default="{ item }">{{ item }}</MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default slot on a component: `#default` shorthand should be
            // bare `v-slot`.
            (
                r"<template><MyComponent #default>Default</MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default slot on `<template>`: explicit `v-slot:default`
            // should be dropped for the default `"shorthand"` (which for
            // `default` just means not writing it out longform).
            (
                r"<template><MyComponent><template v-slot:default>Default</template></MyComponent></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Named slot: `v-slot:name` longform should be `#name`.
            (
                r#"<template><MyComponent><template v-slot:name="{ item }">{{ item }}</template></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Global string option: shorthand reported when longform is
            // required everywhere.
            (
                r#"<template><MyComponent><template #name="{ item }">{{ item }}</template></MyComponent></template>"#,
                Some(json!(["longform"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Per-kind object: `atComponent: "shorthand"` reports bare
            // `v-slot` on a component.
            (
                r#"<template><MyComponent v-slot="{ item }">{{ item }}</MyComponent></template>"#,
                Some(json!([{ "atComponent": "shorthand" }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(VSlotStyle::NAME, VSlotStyle::PLUGIN, pass, fail).test_and_snapshot();
    }
}
