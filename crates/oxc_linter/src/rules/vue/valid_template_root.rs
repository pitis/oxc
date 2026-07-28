use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::{Sfc, ast::Node, parse_template};

use crate::{
    rule::Rule,
    vue_template::{VueSfcRule, VueTemplateContext},
};

fn no_child_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The template requires child element.")
        .with_help(
            "Add at least one element, text run, or interpolation as the template's root content.",
        )
        .with_label(span)
}

fn empty_src_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The template root with 'src' attribute is required to be empty.")
        .with_help("Remove the child content, or drop the `src` attribute — a `<template src=\"...\">` may not have both.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidTemplateRoot;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid content in a Vue SFC's `<template>` block: a
    /// `<template src="...">` must be empty, and a `<template>` without `src`
    /// must have at least one root element, text run, or interpolation.
    ///
    /// ### Why is this bad?
    ///
    /// `<template src="...">` loads its content from another file — inline
    /// content alongside `src` is silently ignored, which is almost always a
    /// mistake. A `<template>` with no meaningful content at all renders
    /// nothing and usually indicates a forgotten root element.
    ///
    /// Unlike Vue 2, Vue 3 templates may have multiple root elements, so this
    /// rule (in eslint-plugin-vue v10) no longer restricts a template to a
    /// single root.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    /// </template>
    /// ```
    ///
    /// ```vue
    /// <template src="./root.html">
    ///   <div>this is ignored and shouldn't be here</div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>a</div>
    ///   <div>b</div>
    /// </template>
    /// ```
    ///
    /// ```vue
    /// <template src="./root.html"></template>
    /// ```
    ValidTemplateRoot,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid template root.",
);

impl Rule for ValidTemplateRoot {}

impl VueSfcRule for ValidTemplateRoot {
    fn run_on_sfc<'a>(&self, sfc: &Sfc<'a>, _path: &Path, ctx: &mut VueTemplateContext<'a>) {
        for block in &sfc.blocks {
            if block.name != "template" {
                continue;
            }
            check_template_block(block, ctx);
        }
    }
}

fn check_template_block<'a>(
    block: &oxc_vue_parser::SfcBlock<'a>,
    ctx: &mut VueTemplateContext<'a>,
) {
    let has_src = block.has_attribute("src");
    let offset = block.content_span.start;
    let nodes = parse_template(block.content);
    let root_nodes: Vec<&Node<'a>> = nodes.iter().filter(|node| is_meaningful_root(node)).collect();

    if has_src && !root_nodes.is_empty() {
        for node in root_nodes {
            ctx.diagnostic(empty_src_diagnostic(shift(node.span(), offset)));
        }
    } else if !has_src && root_nodes.is_empty() {
        ctx.diagnostic(no_child_diagnostic(block.span));
    }
}

/// eslint-plugin-vue's `valid-template-root` counts every child of
/// `program.templateBody` whose source text is non-blank as a "root
/// element" — except HTML comments, which vue-eslint-parser never puts in
/// `children` at all (they live on a side channel), so they never count
/// here either, blank or not.
fn is_meaningful_root(node: &Node<'_>) -> bool {
    match node {
        Node::Comment(_) => false,
        Node::Text(text) => !text.value.trim().is_empty(),
        Node::Raw(_) | Node::Element(_) | Node::Interpolation(_) => true,
    }
}

fn shift(span: Span, offset: u32) -> Span {
    Span::new(span.start + offset, span.end + offset)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidTemplateRoot;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<template><div>a</div></template>", None, None, Some(PathBuf::from("test.vue"))),
            // Vue 3: multiple roots are fine.
            (
                "<template><div>a</div><div>b</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A bare text run counts as root content.
            ("<template>just text</template>", None, None, Some(PathBuf::from("test.vue"))),
            // `src` with no inline content is the correct usage.
            (
                r#"<template src="./root.html"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No `<template>` block at all: nothing to check.
            (
                "<script setup>\nconst a = 1;\n</script>\n",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            ("<template></template>", None, None, Some(PathBuf::from("test.vue"))),
            ("<template>\n</template>", None, None, Some(PathBuf::from("test.vue"))),
            // Only a comment: comments don't count as root content.
            (
                "<template><!-- just a comment --></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `src` with inline content: the content must be reported.
            (
                r#"<template src="./root.html"><div>a</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidTemplateRoot::NAME, ValidTemplateRoot::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
