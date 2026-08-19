use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_dupe_use_directives_diagnostic(key_text: &str, line: usize, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "This `{key_text}` directive is the same and duplicate directives in L{line}."
    ))
    .with_help("The duplicate directive runs the same action again; remove it.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDupeUseDirectives;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplicate `use:` directives on the same element: the same
    /// action with the same parameter expression (or both without one).
    ///
    /// ### Why is this bad?
    ///
    /// Unlike attributes, Svelte does not reject two identical `use:`
    /// directives on one element — the action simply runs twice on the same
    /// node. That is almost never intended and usually a copy-paste mistake;
    /// double-mounted actions can double event listeners, observers, or
    /// other side effects.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div use:clickOutside use:clickOutside></div>
    /// <div use:tooltip={text} use:tooltip={text}></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div use:clickOutside use:tooltip={text}></div>
    /// <div use:tooltip={textA} use:tooltip={textB}></div>
    /// ```
    NoDupeUseDirectives,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow duplicate `use:` directives.",
);

impl Rule for NoDupeUseDirectives {}

impl SvelteTemplateRule for NoDupeUseDirectives {
    // Ports eslint-plugin-svelte's `no-dupe-use-directives`, with two
    // deliberate deviations:
    // - Upstream reports every member of a duplicate group (the first
    //   occurrence pointing at the second's line); we report only the later
    //   occurrences, each pointing at the first occurrence's line.
    // - Upstream compares parameter expressions token-by-token (whitespace-
    //   and comment-insensitive); we compare trimmed source text, so
    //   `use:x={fn( a )}` and `use:x={fn(a)}` are not treated as duplicates.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // (key text, parameter expression text, span of the first
            // occurrence) per distinct directive on this element.
            let mut seen: Vec<(&str, Option<&str>, Span)> = Vec::new();
            for attribute in &element.attributes {
                let AttributeKind::Directive(directive) = &attribute.kind else {
                    continue;
                };
                if directive.kind != DirectiveKind::Use {
                    continue;
                }
                // `raw_name` is the full written key (`use:foo.bar`, plus any
                // modifiers), matching upstream's key text for actions.
                let key_text = directive.raw_name;
                let expression = directive.value.as_ref().map(|value| {
                    value.as_single_expression().map_or_else(
                        || value.span.source_text(source_text).trim_ascii(),
                        |expression| expression.trimmed().0,
                    )
                });
                match seen.iter().find(|(key, expr, _)| *key == key_text && *expr == expression) {
                    Some(&(_, _, first_span)) => {
                        let line =
                            source_text[..first_span.start as usize].matches('\n').count() + 1;
                        diagnostics.push(no_dupe_use_directives_diagnostic(
                            key_text,
                            line,
                            attribute.span,
                        ));
                    }
                    None => seen.push((key_text, expression, attribute.span)),
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

    use super::NoDupeUseDirectives;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<div use:action></div>", None, None, Some(PathBuf::from("test.svelte"))),
            // Different actions.
            ("<div use:foo use:bar></div>", None, None, Some(PathBuf::from("test.svelte"))),
            // Same action, different parameters.
            (
                "<div use:action={foo} use:action={bar}></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // With and without a parameter are distinct.
            (
                "<div use:action use:action={foo}></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Dotted action paths are compared whole.
            ("<div use:lib.foo use:lib.bar></div>", None, None, Some(PathBuf::from("test.svelte"))),
            // Same directive on different elements.
            (
                "<div use:action></div><span use:action></span>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            ("<div use:action use:action></div>", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<div use:action={foo} use:action={foo}></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Whitespace around the parameter expression does not matter.
            (
                "<div use:action={foo} use:action={ foo }></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Dotted action path duplicated.
            (
                "<div use:lib.action={x} use:lib.action={x}></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Multiline: the message points at the first occurrence's line.
            (
                "<div\n  use:action={foo}\n  use:action={foo}\n></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Three copies: each later duplicate is reported.
            (
                "<div use:action use:action use:action></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside blocks.
            (
                "{#if a}<div use:action use:action></div>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoDupeUseDirectives::NAME, NoDupeUseDirectives::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
