use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, BlockKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn parse_error_diagnostic(message: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Svelte parse error: {message}"))
        .with_help("The Svelte compiler rejects this markup; fix it so the file compiles.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct System;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports markup the Svelte compiler would reject as a parse error.
    ///
    /// In eslint-plugin-svelte, `svelte/system` surfaces svelte-eslint-parser
    /// errors. This linter's markup parser never fails — it recovers and
    /// flags the damage instead — so this rule reports the recovered damage:
    /// unterminated `{…}` expressions, comments, and attribute values;
    /// unclosed elements and logic blocks; and orphan `{:else}` / `{/if}`
    /// markers and stray closing tags that matched nothing.
    ///
    /// ### Why is this bad?
    ///
    /// This markup will not compile. Svelte (unlike HTML) requires every
    /// non-void element to be explicitly closed and every logic block to be
    /// terminated; recovered output can silently differ from what was meant.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {#if visible}
    ///   <div>never closed
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {#if visible}
    ///   <div>closed</div>
    /// {/if}
    /// ```
    System,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Report markup that could not be fully parsed.",
);

impl Rule for System {}

impl SvelteTemplateRule for System {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| match node {
            Node::Mustache(tag) if tag.unterminated => {
                diagnostics.push(parse_error_diagnostic("unterminated `{` expression", tag.span));
            }
            Node::Tag(tag) if tag.unterminated => {
                diagnostics.push(parse_error_diagnostic(
                    &format!("unterminated `{{@{}}}` tag", tag.keyword),
                    tag.span,
                ));
            }
            Node::Comment(comment) if comment.unterminated => {
                diagnostics.push(parse_error_diagnostic("unterminated comment", comment.span));
            }
            Node::Element(element) => {
                if element.unclosed {
                    diagnostics.push(parse_error_diagnostic(
                        &format!("`<{}>` was left open", element.name),
                        element.name_span,
                    ));
                }
                for attribute in &element.attributes {
                    let value = match &attribute.kind {
                        AttributeKind::Plain { value, .. } => value.as_ref(),
                        AttributeKind::Directive(directive) => directive.value.as_ref(),
                        _ => None,
                    };
                    let Some(value) = value else { continue };
                    if value.unterminated {
                        diagnostics.push(parse_error_diagnostic(
                            "unterminated attribute value",
                            attribute.span,
                        ));
                        continue;
                    }
                    for part in &value.parts {
                        if let ValuePart::Expression(expression) = part
                            && expression.unterminated
                        {
                            diagnostics.push(parse_error_diagnostic(
                                "unterminated `{` expression",
                                expression.span,
                            ));
                        }
                    }
                }
            }
            Node::Block(block) if block.unclosed => {
                let (keyword, header_span) = match &block.kind {
                    BlockKind::If(if_block) => {
                        ("if", if_block.branches.first().map_or(block.span, |b| b.header_span))
                    }
                    BlockKind::Each(each) => ("each", each.header_span),
                    BlockKind::Await(await_block) => ("await", await_block.header_span),
                    BlockKind::Key(key) => ("key", key.header_span),
                    BlockKind::Snippet(snippet) => ("snippet", snippet.header_span),
                    BlockKind::Unknown(unknown) => (unknown.keyword, unknown.header_span),
                };
                diagnostics.push(parse_error_diagnostic(
                    &format!("`{{#{keyword}}}` block was never closed"),
                    header_span,
                ));
            }
            Node::Raw(span) => {
                // Doctype / processing instructions are legitimate raw
                // pass-through; orphan block markers and stray closing tags
                // are damage the compiler would reject.
                let text = source[span.start as usize..span.end as usize].trim_ascii_start();
                let after_brace = text.strip_prefix('{').map(str::trim_ascii_start);
                if after_brace.is_some_and(|rest| rest.starts_with(':') || rest.starts_with('/')) {
                    diagnostics
                        .push(parse_error_diagnostic("block marker matches no open block", *span));
                } else if text.starts_with("</") {
                    diagnostics
                        .push(parse_error_diagnostic("closing tag matches no open element", *span));
                }
            }
            _ => {}
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::System;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    #[expect(clippy::literal_string_with_formatting_args)] // `{:else}` / `{/if}` are Svelte markup
    fn test() {
        let pass = vec![
            (
                "{#if a}<div>ok</div>{:else}<p>fine</p>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Void and self-closing elements are not "unclosed".
            ("<br><img src={x} /><input>", None, None, Some(PathBuf::from("test.svelte"))),
            // Doctype passes through as raw without being damage.
            ("<!doctype html>\n<div>x</div>", None, None, Some(PathBuf::from("test.svelte"))),
        ];
        let fail = vec![
            // Unclosed element (Svelte requires explicit closing).
            ("{#if a}<div>never closed{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // Unclosed block.
            ("{#each items as item}<p>{item}</p>", None, None, Some(PathBuf::from("test.svelte"))),
            // Unterminated mustache absorbs the rest of the input.
            ("<p>{ oops", None, None, Some(PathBuf::from("test.svelte"))),
            // Orphan markers and stray closing tags.
            ("a{:else}b{/if}c</span>", None, None, Some(PathBuf::from("test.svelte"))),
            // Unterminated attribute value.
            ("<div title=\"unclosed>x</div>", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        Tester::new(System::NAME, System::PLUGIN, pass, fail).test_and_snapshot();
    }
}
