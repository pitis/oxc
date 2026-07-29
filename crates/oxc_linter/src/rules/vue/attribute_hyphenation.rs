use lazy_regex::Regex;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::{Attribute, Node};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{Rule, TupleRuleConfig},
    utils::{
        AlwaysNever, deserialize_to_regexp_group_vec, is_custom_component, vue_casing,
        walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn must_be_hyphenated_diagnostic(text: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Attribute '{text}' must be hyphenated.")).with_label(span)
}

fn cannot_be_hyphenated_diagnostic(text: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Attribute '{text}' can't be hyphenated.")).with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct AttributeHyphenationOptions {
    /// Attribute names (matched as a substring) that are never reported,
    /// in addition to the built-in `data-`/`aria-`/`slot-scope`/SVG-weird-case
    /// list.
    ignore: Vec<String>,
    /// Tag-name patterns; a custom element whose raw tag name matches any of
    /// them is skipped entirely. Each entry is either a bare tag name
    /// (matched as an exact, case-sensitive string — not a substring) or a
    /// `"/pattern/flags"` regex literal, mirroring eslint-plugin-vue's
    /// `toRegExpGroupMatcher`.
    #[serde(deserialize_with = "deserialize_to_regexp_group_vec")]
    ignore_tags: Vec<Regex>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AttributeHyphenationConfig(AlwaysNever, AttributeHyphenationOptions);

// Boxed (like `vue/no-v-html`'s `NoVHtml`): the `Vec<String>`/`Vec<Regex>`
// options would blow `RuleEnum`'s 16-byte budget unboxed (see `block_order.rs`
// for the same pattern); `Box` keeps this rule's own footprint at one pointer
// (8 bytes).
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct AttributeHyphenation(Box<AttributeHyphenationConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces attribute naming style (kebab-case vs. camelCase) for
    /// `v-bind`/`v-model` argument names and plain attribute names, on
    /// custom components and `<slot>`. Default `"always"` requires
    /// hyphenation; `"never"` requires camelCase instead.
    ///
    /// ### Why is this bad?
    ///
    /// Vue templates are effectively HTML, and HTML attribute names are
    /// case-insensitive; mixing casing styles for the same kind of name is
    /// inconsistent and can be confusing since a camelCase name silently
    /// behaves the same as its lowercase form.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (default `"always"`):
    /// ```vue
    /// <template>
    ///   <MyComponent myProp="value" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule (default `"always"`):
    /// ```vue
    /// <template>
    ///   <MyComponent my-prop="value" />
    /// </template>
    /// ```
    AttributeHyphenation,
    vue,
    style,
    config = AttributeHyphenationConfig,
    version = "1.77.0",
    short_description = "Enforce attribute naming style on custom components in template.",
);

impl Rule for AttributeHyphenation {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<TupleRuleConfig<Self>>(value).map(TupleRuleConfig::into_inner)
    }
}

impl VueTemplateRule for AttributeHyphenation {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let AttributeHyphenationConfig(style, options) = &*self.0;
        let use_hyphenated = *style != AlwaysNever::Never;
        walk_elements(nodes, &mut |element| {
            if (!is_custom_component(element) && element.name != "slot")
                || options.ignore_tags.iter().any(|pattern| pattern.is_match(element.name))
            {
                return;
            }
            for attribute in &element.attributes {
                let Some(name) = attribute_name(attribute) else { continue };
                if is_ignored_attribute(name, &options.ignore, use_hyphenated) {
                    continue;
                }
                // Upstream reports `node: node.key` but with an explicit
                // `loc: node.loc` — the *whole* `VAttribute`'s location —
                // and eslint's `loc` wins over `node`. So the label covers
                // the attribute including its `="value"`, not just the key.
                let diagnostic = if use_hyphenated {
                    must_be_hyphenated_diagnostic(attribute.name, attribute.span)
                } else {
                    cannot_be_hyphenated_diagnostic(attribute.name, attribute.span)
                };
                ctx.diagnostic(diagnostic);
            }
        });
    }
}

/// eslint-plugin-vue `attribute-hyphenation`'s `getAttributeName`: a plain
/// attribute reports under its own raw name; a `v-bind`/`v-model` directive
/// with a static argument reports under that argument's raw name; every
/// other directive (including dynamic `:[arg]` and directives without an
/// argument) is not checked by this rule.
fn attribute_name<'a>(attribute: &Attribute<'a>) -> Option<&'a str> {
    match &attribute.directive {
        None => Some(attribute.name),
        Some(directive) if directive.name == "bind" || directive.name == "model" => {
            let argument = directive.argument.as_ref()?;
            if argument.dynamic { None } else { Some(argument.text) }
        }
        Some(_) => None,
    }
}

/// eslint-plugin-vue `attribute-hyphenation`'s `isIgnoredAttribute`: matched
/// (skipped) when the name contains one of the ignored substrings, or when
/// it is already compatible with the requested casing (no uppercase letter
/// when hyphenation is required; no hyphen when camelCase is required).
fn is_ignored_attribute(name: &str, ignore: &[String], use_hyphenated: bool) -> bool {
    if IGNORED_BUILTIN_ATTRIBUTES.iter().any(|builtin| name.contains(builtin))
        || SVG_ATTRIBUTES_WEIRD_CASE.iter().any(|attr| name.contains(attr))
        || ignore.iter().any(|attr| name.contains(attr.as_str()))
    {
        return true;
    }
    if use_hyphenated { !vue_casing::has_upper(name) } else { !name.contains('-') }
}

const IGNORED_BUILTIN_ATTRIBUTES: &[&str] = &["data-", "aria-", "slot-scope"];

/// eslint-plugin-vue's `utils/svg-attributes-weird-case.json`: SVG attribute
/// names that mix case by spec and would otherwise falsely trip this rule.
const SVG_ATTRIBUTES_WEIRD_CASE: &[&str] = &[
    "accent-height",
    "alignment-baseline",
    "arabic-form",
    "attributeName",
    "attributeType",
    "baseFrequency",
    "baseline-shift",
    "baseProfile",
    "calcMode",
    "cap-height",
    "clipPathUnits",
    "clip-path",
    "clip-rule",
    "color-interpolation",
    "color-interpolation-filters",
    "color-profile",
    "color-rendering",
    "contentScriptType",
    "contentStyleType",
    "diffuseConstant",
    "dominant-baseline",
    "edgeMode",
    "enable-background",
    "externalResourcesRequired",
    "fill-opacity",
    "fill-rule",
    "filterRes",
    "filterUnits",
    "flood-color",
    "flood-opacity",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "glyph-name",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "glyphRef",
    "gradientTransform",
    "gradientUnits",
    "horiz-adv-x",
    "horiz-origin-x",
    "image-rendering",
    "kernelMatrix",
    "kernelUnitLength",
    "keyPoints",
    "keySplines",
    "keyTimes",
    "lengthAdjust",
    "letter-spacing",
    "lighting-color",
    "limitingConeAngle",
    "marker-end",
    "marker-mid",
    "marker-start",
    "markerHeight",
    "markerUnits",
    "markerWidth",
    "maskContentUnits",
    "maskUnits",
    "numOctaves",
    "overline-position",
    "overline-thickness",
    "panose-1",
    "paint-order",
    "pathLength",
    "patternContentUnits",
    "patternTransform",
    "patternUnits",
    "pointer-events",
    "pointsAtX",
    "pointsAtY",
    "pointsAtZ",
    "preserveAlpha",
    "preserveAspectRatio",
    "primitiveUnits",
    "referrerPolicy",
    "refX",
    "refY",
    "rendering-intent",
    "repeatCount",
    "repeatDur",
    "requiredExtensions",
    "requiredFeatures",
    "shape-rendering",
    "specularConstant",
    "specularExponent",
    "spreadMethod",
    "startOffset",
    "stdDeviation",
    "stitchTiles",
    "stop-color",
    "stop-opacity",
    "strikethrough-position",
    "strikethrough-thickness",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "surfaceScale",
    "systemLanguage",
    "tableValues",
    "targetX",
    "targetY",
    "text-anchor",
    "text-decoration",
    "text-rendering",
    "textLength",
    "transform-origin",
    "underline-position",
    "underline-thickness",
    "unicode-bidi",
    "unicode-range",
    "units-per-em",
    "v-alphabetic",
    "v-hanging",
    "v-ideographic",
    "v-mathematical",
    "vector-effect",
    "vert-adv-y",
    "vert-origin-x",
    "vert-origin-y",
    "viewBox",
    "viewTarget",
    "word-spacing",
    "writing-mode",
    "x-height",
    "xChannelSelector",
    "yChannelSelector",
    "zoomAndPan",
];

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::AttributeHyphenation;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Native elements are not checked.
            (
                r#"<template><div myAttr="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Default ("always"): already-hyphenated names pass.
            (
                r#"<template><MyComponent my-prop="a" :my-bound-prop="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Single-word names have nothing to hyphenate.
            (
                r#"<template><MyComponent prop="a" :bound="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `<slot>` is checked like a component.
            (
                r#"<template><slot my-prop="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Built-in ignored prefixes.
            (
                r#"<template><MyComponent data-fooBar="a" aria-fooBar="b" v-slot-scope="c" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // SVG weird-case attribute.
            (
                r#"<template><MyComponent :viewBox="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic argument is never checked.
            (
                r#"<template><MyComponent :[myProp]="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Non-bind/model directives are not checked.
            (
                r#"<template><MyComponent v-myDirective="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "never": camelCase passes.
            (
                r#"<template><MyComponent myProp="a" :myBoundProp="b" /></template>"#,
                Some(json!(["never"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignore` option.
            (
                r#"<template><MyComponent myProp="a" /></template>"#,
                Some(json!(["always", { "ignore": ["myProp"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreTags` option: bare string is an exact (anchored) match,
            // not a substring search.
            (
                r#"<template><IgnoredTag myProp="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["IgnoredTag"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreTags` option: `"/pattern/flags"` regex-literal form.
            (
                r#"<template><IgnoredTag myProp="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["/^Ignored/"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // v-model with static argument, already hyphenated.
            (
                r#"<template><MyComponent v-model:my-value="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Default ("always"): camelCase name on a component.
            (
                r#"<template><MyComponent myProp="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Bound (`v-bind`) argument, shorthand form.
            (
                r#"<template><MyComponent :myProp="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Longhand `v-bind:`.
            (
                r#"<template><MyComponent v-bind:myProp="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-model` with a static argument.
            (
                r#"<template><MyComponent v-model:myValue="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `<slot>`.
            (
                r#"<template><slot myProp="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "never": hyphenated name reported.
            (
                r#"<template><MyComponent my-prop="a" /></template>"#,
                Some(json!(["never"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "never": bound hyphenated argument reported.
            (
                r#"<template><MyComponent :my-prop="a" /></template>"#,
                Some(json!(["never"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreTags` bare string is an exact match, not a substring
            // search: a superstring tag name is still checked.
            (
                r#"<template><IgnoredTagExtra myProp="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["IgnoredTag"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // ...and a tag name that merely *contains* the pattern as a
            // substring is still checked too.
            (
                r#"<template><MyIgnoredTag myProp="a" /></template>"#,
                Some(json!(["always", { "ignoreTags": ["IgnoredTag"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(AttributeHyphenation::NAME, AttributeHyphenation::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
