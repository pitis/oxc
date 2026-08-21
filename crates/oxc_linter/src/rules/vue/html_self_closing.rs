use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::{Element, Node, is_void_element};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        VUE_RESERVED_HTML_ELEMENTS, VUE_RESERVED_SVG_ELEMENTS, is_custom_component,
        is_math_element_name, walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_self_closing_diagnostic(span: Span, kind: &str, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Require self-closing on {kind} (<{name}>)."))
        .with_help(format!("Write it as `<{name} />`."))
        .with_label(span)
}

fn disallow_self_closing_diagnostic(span: Span, kind: &str, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Disallow self-closing on {kind} (<{name}/>)."))
        .with_help(format!("Write it as `<{name}>`."))
        .with_label(span)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SelfClosingMode {
    /// Require `<x />`.
    Always,
    /// Require `<x></x>`.
    Never,
    /// Accept either.
    Any,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct HtmlKinds {
    /// Elements with a normal content model (`<div>`, `<p>`, …).
    pub normal: SelfClosingMode,
    /// Void elements (`<br>`, `<img>`, …), which cannot have content at all.
    #[serde(rename = "void")]
    pub void_: SelfClosingMode,
    /// Vue components.
    pub component: SelfClosingMode,
}

impl Default for HtmlKinds {
    fn default() -> Self {
        Self {
            normal: SelfClosingMode::Always,
            void_: SelfClosingMode::Never,
            component: SelfClosingMode::Always,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct HtmlSelfClosing {
    pub html: HtmlKinds,
    pub svg: SelfClosingMode,
    pub math: SelfClosingMode,
}

impl Default for HtmlSelfClosing {
    fn default() -> Self {
        Self {
            html: HtmlKinds::default(),
            svg: SelfClosingMode::Always,
            math: SelfClosingMode::Always,
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces one style for elements with no content: `<MyComponent />` or
    /// `<MyComponent></MyComponent>`, configurable per element kind.
    ///
    /// ### Why is this bad?
    ///
    /// Purely a consistency rule. The two spellings mean the same thing to the
    /// compiler, so mixing them is noise in diffs and review.
    ///
    /// Note that self-closing works for components and SVG but *not* for
    /// normal HTML elements in a real browser parse — `<div />` there is an
    /// open `<div>`. Inside a Vue `<template>` it is compiled, not parsed by
    /// the browser, so it is fine; the default reflects that.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (with the defaults):
    /// ```vue
    /// <template>
    ///   <MyComponent></MyComponent>
    ///   <div></div>
    ///   <img />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <MyComponent />
    ///   <div />
    ///   <div>content</div>
    ///   <img>
    /// </template>
    /// ```
    ///
    /// ### Options
    ///
    /// `{ html: { normal, void, component }, svg, math }`, each `"always"`,
    /// `"never"` or `"any"`. Defaults are `normal: "always"`, `void: "never"`,
    /// `component: "always"`, `svg: "always"`, `math: "always"`.
    ///
    /// ```json
    /// { "vue/html-self-closing": ["error", { "html": { "void": "always" } }] }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream is auto-fixable; this is not, because the `<template>` pass
    /// cannot emit fixes yet.
    ///
    /// Element kind is decided from the tag name rather than from the parsed
    /// namespace vue-eslint-parser tracks, which is the approximation the rest
    /// of this linter's Vue template rules already make. The two disagree only
    /// for a name that is in no well-known list, which upstream classifies as a
    /// custom component here too.
    HtmlSelfClosing,
    vue,
    style,
    config = HtmlSelfClosing,
    version = "1.80.0",
    short_description = "Enforce self-closing style.",
);

impl Rule for HtmlSelfClosing {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

/// Upstream's `ELEMENT_TYPE_MESSAGES`, which are interpolated into the message.
const NORMAL: &str = "HTML elements";
const VOID: &str = "HTML void elements";
const COMPONENT: &str = "Vue.js custom components";
const SVG: &str = "SVG elements";
const MATH: &str = "MathML elements";

impl HtmlSelfClosing {
    /// Upstream's `getElementType`, paired with the mode configured for it.
    /// `None` is upstream's `UNKNOWN`, which has no mode and is never reported.
    fn element_kind(self, element: &Element<'_>) -> Option<(SelfClosingMode, &'static str)> {
        let name = element.name;
        if is_custom_component(element) {
            return Some((self.html.component, COMPONENT));
        }
        if VUE_RESERVED_HTML_ELEMENTS.contains(name) {
            return Some(if is_void_element(name) {
                (self.html.void_, VOID)
            } else {
                (self.html.normal, NORMAL)
            });
        }
        if VUE_RESERVED_SVG_ELEMENTS.contains(name) {
            return Some((self.svg, SVG));
        }
        if is_math_element_name(name) {
            return Some((self.math, MATH));
        }
        None
    }
}

impl VueTemplateRule for HtmlSelfClosing {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let mut reports = Vec::new();
        walk_elements(nodes, &mut |element| {
            let Some((mode, kind)) = self.element_kind(element) else { return };
            match mode {
                SelfClosingMode::Always
                    if !element.self_closing && is_empty(element, source_text) =>
                {
                    // Upstream reports at the end tag when there is one.
                    let span = end_tag_span(element, source_text).unwrap_or(element.span);
                    reports.push(require_self_closing_diagnostic(span, kind, element.name));
                }
                SelfClosingMode::Never if element.self_closing => {
                    // Upstream points at the `/>` itself.
                    let span = Span::new(element.span.end.saturating_sub(2), element.span.end);
                    reports.push(disallow_self_closing_diagnostic(span, kind, element.name));
                }
                _ => {}
            }
        });
        for diagnostic in reports {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Upstream's `isEmpty`: nothing but whitespace between the tags. Comments
/// deliberately count as content.
fn is_empty(element: &Element<'_>, source_text: &str) -> bool {
    if element.self_closing || element.is_void {
        return true;
    }
    let start = element.open_tag_end as usize;
    let end = end_tag_span(element, source_text)
        .map_or(element.span.end as usize, |span| span.start as usize);
    source_text.get(start..end).is_some_and(|content| content.trim().is_empty())
}

/// The `</name>` at the end of `element`, when it has one.
fn end_tag_span(element: &Element<'_>, source_text: &str) -> Option<Span> {
    if element.self_closing || element.is_void || element.unclosed {
        return None;
    }
    let end = element.span.end as usize;
    let start = u32::try_from(source_text.get(..end)?.rfind("</")?).ok()?;
    if start < element.open_tag_end {
        return None;
    }
    Some(Span::new(start, element.span.end))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::HtmlSelfClosing;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            // Defaults: components and normal HTML self-close, void does not.
            ("<template><MyComponent /></template>", None, None, vue()),
            ("<template><div /></template>", None, None, vue()),
            ("<template><img></template>", None, None, vue()),
            // Non-empty elements are never reported.
            ("<template><div>text</div></template>", None, None, vue()),
            ("<template><MyComponent>text</MyComponent></template>", None, None, vue()),
            // A comment counts as content.
            ("<template><div><!-- x --></div></template>", None, None, vue()),
            // `any` accepts both spellings.
            (
                "<template><div /><div></div></template>",
                Some(json!([{ "html": { "normal": "any" } }])),
                None,
                vue(),
            ),
            // Opting void into self-closing.
            (
                "<template><img /></template>",
                Some(json!([{ "html": { "void": "always" } }])),
                None,
                vue(),
            ),
        ];

        let fail = vec![
            ("<template><MyComponent></MyComponent></template>", None, None, vue()),
            ("<template><div></div></template>", None, None, vue()),
            ("<template><img /></template>", None, None, vue()),
            // Whitespace only is still empty.
            ("<template><div>\n  </div></template>", None, None, vue()),
            // `never` on components.
            (
                "<template><MyComponent /></template>",
                Some(json!([{ "html": { "component": "never" } }])),
                None,
                vue(),
            ),
            // SVG defaults to `always`.
            ("<template><svg><circle></circle></svg></template>", None, None, vue()),
        ];

        Tester::new(HtmlSelfClosing::NAME, HtmlSelfClosing::PLUGIN, pass, fail).test_and_snapshot();
    }
}
