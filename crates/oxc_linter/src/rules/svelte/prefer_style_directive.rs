use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_style_attribute, walk_svelte_elements},
};

fn prefer_style_directive_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Can use `style:` directive instead.")
        .with_help("Write `style:property=\"value\"` so Svelte updates that property on its own.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferStyleDirective;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers a `style:` directive over a declaration written inside a
    /// `style` attribute.
    ///
    /// ### Why is this bad?
    ///
    /// `style:color={color}` compiles to a targeted `style.setProperty`
    /// call, so Svelte updates just that property and leaves any other
    /// inline styles — including ones set by an action or a parent — intact.
    /// A `style` attribute is rewritten wholesale.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div style="color: red"></div>
    /// <div style="color: {color}"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div style:color="red"></div>
    /// <div style:color={color}></div>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream also rewrites a whole-value conditional
    /// (`style="{cond ? 'color: red' : ''}"`) into a directive. oxlint does
    /// not analyse the expression, so only declarations whose property name
    /// is written literally are reported.
    PreferStyleDirective,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Prefer a `style:` directive over a `style` attribute declaration.",
);

impl Rule for PreferStyleDirective {}

impl SvelteTemplateRule for PreferStyleDirective {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut spans = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // A property that already has its own `style:` directive is left
            // alone — rewriting would make the element set it twice.
            let directives: Vec<&str> = element
                .attributes
                .iter()
                .filter_map(|attribute| match &attribute.kind {
                    AttributeKind::Directive(directive)
                        if directive.kind == DirectiveKind::Style =>
                    {
                        Some(directive.name)
                    }
                    _ => None,
                })
                .collect();

            for attribute in &element.attributes {
                let AttributeKind::Plain { name: "style", value: Some(value), .. } =
                    &attribute.kind
                else {
                    continue;
                };
                for segment in parse_svelte_style_attribute(value, source) {
                    let Some(decl) = segment.as_decl() else { continue };
                    // `!important` cannot be expressed by a directive.
                    if decl.important || directives.contains(&decl.property) {
                        continue;
                    }
                    spans.push(Span::new(decl.property_span.start, decl.value_span.end));
                }
            }
        });
        for span in spans {
            ctx.diagnostic(prefer_style_directive_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::PreferStyleDirective;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<div style:color=\"red\"></div>", None, None, path()),
            ("<div style:color={color}></div>", None, None, path()),
            // `!important` has no directive equivalent.
            ("<div style=\"color: red !important\"></div>", None, None, path()),
            // The property already has its own directive.
            ("<div style=\"color: red\" style:color={c}></div>", None, None, path()),
            // Not statically readable, so nothing to rewrite.
            ("<div style=\"{styles}\"></div>", None, None, path()),
            ("<div style=\"{key}: red\"></div>", None, None, path()),
            ("<div class=\"x\"></div>", None, None, path()),
        ];
        let fail = vec![
            ("<div style=\"color: red\"></div>", None, None, path()),
            ("<div style=\"color: {color}\"></div>", None, None, path()),
            ("<div style=\"color: red; background: blue\"></div>", None, None, path()),
        ];

        Tester::new(PreferStyleDirective::NAME, PreferStyleDirective::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
