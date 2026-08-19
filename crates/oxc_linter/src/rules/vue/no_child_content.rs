use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn disallowed_child_content_diagnostic(directive_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Child content is disallowed because it will be overwritten by the v-{directive_name} directive."
    ))
    .with_help("Remove the child content, or drop the directive if the static content should stay.")
    .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoChildContentConfig {
    /// Additional directive names (without the `v-` prefix) that, like
    /// `v-html`/`v-text`, overwrite an element's child content. Default: none.
    additional_directives: Vec<String>,
}

// Boxed (like `vue/valid-v-on`'s `ValidVOn`): `additional_directives:
// Vec<String>` is 24 bytes unboxed, which would make this the largest
// `RuleEnum` variant and grow every rule's in-memory representation from 16
// to 24 bytes. `Box` keeps this rule's own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct NoChildContent(Box<NoChildContentConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows child content (non-whitespace text, elements, comments, or
    /// interpolations) on an element that also carries `v-html` or `v-text`
    /// (or, via the `additionalDirectives` option, another directive that
    /// similarly overwrites an element's content).
    ///
    /// ### Why is this bad?
    ///
    /// `v-html`/`v-text` replace the element's entire content at render
    /// time; any content written in the template is discarded, so keeping
    /// it there is dead code that misleads readers into thinking it renders.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-html="raw">This is discarded.</div>
    ///   <div v-text="text"><span>So is this.</span></div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-html="raw"></div>
    ///   <div v-text="text"></div>
    /// </template>
    /// ```
    NoChildContent,
    vue,
    correctness,
    config = NoChildContent,
    version = "1.77.0",
    short_description = "Disallow element's child contents which would be overwritten by a directive like `v-html` or `v-text`.",
);

impl Rule for NoChildContent {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoChildContent {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| self.check_element(element, ctx));
    }
}

impl NoChildContent {
    fn check_element<'a>(&self, element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
        // Mirrors `elementNode.endTag === null`: a self-closing, void, or
        // (recovered) unclosed element has no real closing tag, so there is
        // no genuine "between the tags" region to check.
        if element.self_closing || element.is_void || element.unclosed {
            return;
        }

        // Every content-overwriting directive on the element, in source
        // order — upstream's visitor runs once per `VAttribute[directive=true]`
        // independently, so an element with *both* `v-html` and `v-text` (or
        // an `additionalDirectives` name alongside either) gets one report
        // per directive, not just the first.
        let directive_names: Vec<&str> = element
            .attributes
            .iter()
            .filter_map(|attribute| {
                let directive = attribute.directive.as_ref()?;
                (directive.name == "html"
                    || directive.name == "text"
                    || self
                        .0
                        .additional_directives
                        .iter()
                        .any(|name| name.as_str() == directive.name))
                .then_some(directive.name)
            })
            .collect();
        if directive_names.is_empty() {
            return;
        }

        let span = content_span(element).or_else(|| {
            // `<textarea>`/`<script>`/`<style>` bodies never become
            // `children` in this fork's parser (they're kept as raw,
            // unparsed text for byte-faithful reprinting — see
            // `no-textarea-mustache`'s doc comment for the same
            // distinction). Approximate upstream's check for this case too:
            // any non-whitespace raw text counts as child content.
            element.raw_text.filter(|raw_text| {
                let text = &ctx.source_text()[raw_text.start as usize..raw_text.end as usize];
                !text.trim().is_empty()
            })
        });
        let Some(span) = span else { return };

        for directive_name in directive_names {
            ctx.diagnostic(disallowed_child_content_diagnostic(directive_name, span));
        }
    }
}

/// eslint-plugin-vue's `isWhiteSpaceTextNode` + `getLocationRange`: `None`
/// when every child is a whitespace-only text node (comments, elements, and
/// interpolations always count as content); otherwise the span from the
/// first non-trivial child's start to the last child's end — matching
/// upstream reporting the full range of *all* children (not just the
/// non-whitespace ones) once any of them counts as content.
fn content_span(element: &Element<'_>) -> Option<Span> {
    let has_content = element.children.iter().any(|child| match child {
        Node::Text(text) => !text.value.trim().is_empty(),
        _ => true,
    });
    if !has_content {
        return None;
    }
    let first = element.children.first()?.span();
    let last = element.children.last()?.span();
    Some(Span::new(first.start, last.end))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoChildContent;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-html="raw"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-html="raw">   </div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-html="raw" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-bind:title="raw">content</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // additionalDirectives not enabled: v-custom-directive is inert
            // to this rule.
            (
                r#"<template><div v-custom-directive="x">content</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><div v-html="raw"><span>content</span></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-html="raw"><!-- comment --></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div v-html='raw'>{{ text }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-text="t">hello</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // additionalDirectives option.
            (
                r#"<template><div v-custom-directive="x">hello</div></template>"#,
                Some(json!([{ "additionalDirectives": ["custom-directive"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Both v-html and v-text on the same element: upstream visits
            // each `VAttribute[directive=true]` independently, so this is
            // TWO reports (one per directive), not one.
            (
                r#"<template><div v-html="a" v-text="b">hello</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoChildContent::NAME, NoChildContent::PLUGIN, pass, fail).test_and_snapshot();
    }
}
