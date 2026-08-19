use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_dupe_on_directives_diagnostic(event: &str, line: usize, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "This `on:{event}` directive is the same and duplicate directives in L{line}."
    ))
    .with_help("Remove the duplicate directive, or change its handler if it was meant to differ.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDupeOnDirectives;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplicate `on:` directives: the same event with the same
    /// handler listed more than once on one element. Modifiers are not
    /// considered — `on:click|once` and `on:click` count as the same
    /// directive when their handlers match.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte allows any number of `on:` directives for the same event,
    /// and attaches every one of them — a duplicated directive runs the
    /// same handler twice per event. That is almost certainly a
    /// copy-paste mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <button on:click on:click />
    /// <input on:focus={handler} on:focus={handler} />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <button on:click on:click={handler} />
    /// <input on:focus={focusHandler} on:blur={blurHandler} />
    /// ```
    NoDupeOnDirectives,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow duplicate `on:` directives.",
);

impl Rule for NoDupeOnDirectives {}

/// 1-based line number of a byte offset, for upstream's `L{line}` message.
fn line_number(source_text: &str, offset: u32) -> usize {
    source_text[..offset as usize].bytes().filter(|byte| *byte == b'\n').count() + 1
}

impl SvelteTemplateRule for NoDupeOnDirectives {
    // Ports eslint-plugin-svelte's `no-dupe-on-directives`. Like upstream,
    // every member of a duplicate group is reported (the first occurrence
    // points at the second's line, later ones point at the first's line).
    //
    // Deviation: upstream compares handler expressions token-by-token
    // (whitespace- and comment-insensitive). This AST keeps handlers as
    // source text, so handlers are compared by trimmed text instead —
    // token-identical handlers that differ in inner whitespace or comments
    // are not flagged.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Buckets of (event name, handler text) -> directive spans; a
            // value-less forwarder (`on:click`) only matches other
            // value-less forwarders of the same event.
            let mut groups: Vec<(&str, Option<&str>, Vec<Span>)> = Vec::new();
            for attribute in &element.attributes {
                let AttributeKind::Directive(directive) = &attribute.kind else {
                    continue;
                };
                if directive.kind != DirectiveKind::On {
                    continue;
                }
                let handler = directive.value.as_ref().map(|value| {
                    value.as_single_expression().map_or_else(
                        || value.span.source_text(source_text).trim(),
                        |expression| expression.trimmed().0,
                    )
                });
                match groups
                    .iter_mut()
                    .find(|(event, existing, _)| *event == directive.name && *existing == handler)
                {
                    Some((_, _, spans)) => spans.push(attribute.span),
                    None => groups.push((directive.name, handler, vec![attribute.span])),
                }
            }
            for (event, _, spans) in groups {
                if spans.len() < 2 {
                    continue;
                }
                for (index, span) in spans.iter().enumerate() {
                    let other = if index == 0 { spans[1] } else { spans[0] };
                    let line = line_number(source_text, other.start);
                    diagnostics.push(no_dupe_on_directives_diagnostic(event, line, *span));
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

    use super::NoDupeOnDirectives;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // A forwarder and a handler are different directives.
            (
                "<button on:click on:click={myHandler} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Same event, different handlers.
            (
                "<button on:click={foo} on:click={bar} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<button\n\ton:focus|once\n\ton:focus={(evt) => console.log(evt)}\n\ton:keydown={() => console.log('foo')}\n\ton:keydown={(evt) => console.log(evt)}\n/>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Same handler on different events.
            (
                "<button on:click={handler} on:keydown={handler} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Same directive on different elements.
            (
                "<div on:click={handler} /><div on:click={handler} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            ("<button on:click on:click />", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<button on:click={myHandler} on:click={myHandler} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Modifiers are ignored: all five are the same forwarder.
            (
                "<button on:click|once on:click on:click|self on:click|capture on:click|stopPropagation|self />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Identical inline handlers.
            (
                "<button on:keydown={() => console.log('foo')} on:keydown={() => console.log('foo')} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Quoted and unquoted expression values compare equal.
            (
                "<button on:click={handler} on:click=\"{handler}\" />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Groups are per element; nested elements report independently.
            (
                "<div on:mousemove on:mousemove>\n\t<div on:mousemove on:mousemove />\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Line numbers in the message: the first duplicate points at
            // the second's line, later ones point back at the first's.
            (
                "<button\n\ton:click={handler}\n\ton:click={handler}\n\ton:click={handler}\n/>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoDupeOnDirectives::NAME, NoDupeOnDirectives::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
