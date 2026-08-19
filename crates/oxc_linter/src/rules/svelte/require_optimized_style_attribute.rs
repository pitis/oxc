use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{SvelteStyleSegment, parse_svelte_style_attribute, walk_svelte_elements},
};

fn shorthand_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected shorthand `style` attribute.")
        .with_help("Write the declarations out, e.g. `style=\"color: {color}\"`.")
        .with_label(span)
}

fn complex_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected complex `style` attribute.")
        .with_help("Keep each declaration a simple `property: value` pair so Svelte can update it in place.")
        .with_label(span)
}

fn interpolation_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected interpolation in the property name of a `style` attribute.")
        .with_help("Use a fixed property name, or a `style:` directive.")
        .with_label(span)
}

fn comment_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected comment in a `style` attribute.")
        .with_help("Remove the comment.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireOptimizedStyleAttribute;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the value of a `style` attribute to be a plain list of
    /// `property: value` declarations, so Svelte can compile it into
    /// individual `style.setProperty` updates.
    ///
    /// ### Why is this bad?
    ///
    /// When Svelte cannot statically read the declarations — a shorthand
    /// `{style}`, an interpolated property name, a comment, or anything else
    /// it cannot decompose — it falls back to rewriting the element's whole
    /// `style` attribute on every update. That is slower and clobbers styles
    /// set from elsewhere.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div {style}></div>
    /// <div style="{key}: red"></div>
    /// <div style="color: red; /* comment */"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div style="color: red"></div>
    /// <div style="color: {color}"></div>
    /// <div style:color></div>
    /// ```
    RequireOptimizedStyleAttribute,
    svelte,
    perf,
    version = "1.80.0",
    short_description = "Require `style` attributes Svelte can update in place.",
);

impl Rule for RequireOptimizedStyleAttribute {}

impl SvelteTemplateRule for RequireOptimizedStyleAttribute {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                match &attribute.kind {
                    // `<div {style}>` — the whole value is opaque.
                    AttributeKind::Shorthand { name: "style", .. } => {
                        diagnostics.push(shorthand_diagnostic(attribute.span));
                    }
                    AttributeKind::Plain { name: "style", value: Some(value), .. } => {
                        for segment in parse_svelte_style_attribute(value, source) {
                            match segment {
                                SvelteStyleSegment::Decl(_) => {}
                                SvelteStyleSegment::Comment(span) => {
                                    diagnostics.push(comment_diagnostic(span));
                                }
                                SvelteStyleSegment::Unknown(span) => {
                                    // An interpolated property name gets its
                                    // own message upstream; everything else
                                    // is "complex".
                                    let text = &source[span.start as usize..span.end as usize];
                                    if text.contains('{') && text.contains(':') {
                                        diagnostics.push(interpolation_key_diagnostic(span));
                                    } else {
                                        diagnostics.push(complex_diagnostic(span));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireOptimizedStyleAttribute;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<div style=\"color: red\"></div>", None, None, path()),
            ("<div style=\"color: {color}\"></div>", None, None, path()),
            ("<div style=\"color: red; background: blue\"></div>", None, None, path()),
            ("<div style:color></div>", None, None, path()),
            // A `;` inside quotes or parentheses does not split a declaration.
            ("<div style=\"background: url(a;b)\"></div>", None, None, path()),
            // A trailing comment inside a value stays part of that value,
            // exactly as PostCSS parses it upstream.
            ("<div style=\"color: red /* comment */\"></div>", None, None, path()),
            // No `style` attribute at all.
            ("<div class=\"x\"></div>", None, None, path()),
        ];
        let fail = vec![
            ("<div {style}></div>", None, None, path()),
            // Interpolated property name.
            ("<div style=\"{key}: red\"></div>", None, None, path()),
            // A whole declaration list from one expression.
            ("<div style=\"{styles}\"></div>", None, None, path()),
            // A comment standing on its own between declarations.
            ("<div style=\"color: red; /* comment */\"></div>", None, None, path()),
        ];

        Tester::new(
            RequireOptimizedStyleAttribute::NAME,
            RequireOptimizedStyleAttribute::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
