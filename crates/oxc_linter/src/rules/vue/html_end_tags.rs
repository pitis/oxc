use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    utils::{start_tag_span, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn missing_end_tag_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'<{name}>' should have end tag."))
        .with_help(format!("Add `</{name}>`, or write the element as `<{name} />`."))
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct HtmlEndTags;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires every element that can have one to be closed — either with an
    /// end tag or by self-closing.
    ///
    /// ### Why is this bad?
    ///
    /// An unclosed element does not end where it looks like it ends: the
    /// compiler keeps nesting until the parent closes, so the siblings written
    /// after it silently become its children. Void elements (`<br>`, `<img>`,
    /// …) are exempt because they cannot have an end tag at all.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>
    ///   <p>text
    ///   </div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>
    ///     <p>text</p>
    ///     <br>
    ///     <MyComponent />
    ///   </div>
    /// </template>
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream is auto-fixable; this is not, because the `<template>` pass
    /// cannot emit fixes yet.
    ///
    /// Upstream additionally suppresses the whole rule when the document has
    /// an invalid EOF (`hasInvalidEOF`), to avoid piling onto a file that is
    /// already broken. `vue_sfc_parser` recovers instead of surfacing that
    /// condition, and `vue/no-parsing-error` reports the underlying breakage,
    /// so a file with an unterminated tag can get findings from both rules
    /// here where upstream gives only the parse error.
    HtmlEndTags,
    vue,
    style,
    version = "1.80.0",
    short_description = "Enforce end tag style.",
);

impl Rule for HtmlEndTags {}

impl VueTemplateRule for HtmlEndTags {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let mut reports = Vec::new();
        walk_elements(nodes, &mut |element| {
            if element.is_void || element.self_closing || !element.unclosed {
                return;
            }
            reports.push((start_tag_span(element, source_text), element.name));
        });
        for (span, name) in reports {
            ctx.diagnostic(missing_end_tag_diagnostic(span, name));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HtmlEndTags;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            ("<template><div></div></template>", None, None, vue()),
            ("<template><div /></template>", None, None, vue()),
            // Void elements cannot take an end tag.
            ("<template><div><br><img><input></div></template>", None, None, vue()),
            // Self-closing components.
            ("<template><MyComponent /></template>", None, None, vue()),
            ("<template><div><p>text</p></div></template>", None, None, vue()),
        ];

        let fail = vec![
            ("<template><div><p>text</div></template>", None, None, vue()),
            ("<template><div></template>", None, None, vue()),
            ("<template><div><span><em>x</span></div></template>", None, None, vue()),
        ];

        Tester::new(HtmlEndTags::NAME, HtmlEndTags::PLUGIN, pass, fail).test_and_snapshot();
    }
}
