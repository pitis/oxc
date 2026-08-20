use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, BlockKind, Node, ValuePart};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn expected_opening_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected 1 space after '{', but not found.")
        .with_help("Put a space after the opening brace.")
        .with_label(span)
}

fn expected_closing_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected 1 space before '}', but not found.")
        .with_help("Put a space before the closing brace.")
        .with_label(span)
}

fn unexpected_opening_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected no space after '{', but found.")
        .with_help("Remove the space after the opening brace.")
        .with_label(span)
}

fn unexpected_closing_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected no space before '}', but found.")
        .with_help("Remove the space before the closing brace.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Spacing {
    /// Require no space inside the brace.
    #[default]
    Never,
    /// Require exactly one space inside the brace.
    Always,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ClosingSpacing {
    #[default]
    Never,
    Always,
    /// Require a space only when the marker actually carries an expression,
    /// so `{/if}` stays tight while `{#if x }` does not.
    AlwaysAfterExpression,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct Tags {
    /// Spacing after `{` of a `{#…}`, `{:…}`, `{/…}` or `{@…}` marker.
    opening_brace: Spacing,
    /// Spacing before `}` of the same.
    closing_brace: ClosingSpacing,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MustacheSpacing {
    /// `{ count }` in text position.
    text_expressions: Spacing,
    /// `foo={ bar }` and `{ ...props }`.
    attributes_and_props: Spacing,
    /// `on:click={ handler }`.
    directive_expressions: Spacing,
    /// `{#if}`, `{:else}`, `{/each}`, `{@html}` and friends.
    tags: Tags,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces the same spacing inside every `{…}` of a component.
    ///
    /// ### Why is this bad?
    ///
    /// Writing `{count}` in one place and `{ count }` in the next is noise
    /// that shows up in every diff that touches the markup.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// { count }
    /// <div foo={ bar }></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {count}
    /// <div foo={bar}></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `textExpressions`, `attributesAndProps` and `directiveExpressions`
    /// each take `"never"` (the default) or `"always"`. `tags.openingBrace`
    /// does too; `tags.closingBrace` also takes
    /// `"always-after-expression"`, which spaces only the markers that carry
    /// an expression.
    ///
    /// ```json
    /// {
    ///   "svelte/mustache-spacing": [
    ///     "error",
    ///     { "textExpressions": "always", "tags": { "openingBrace": "always" } }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream rewrites the spacing; the Svelte markup pass reports only.
    /// A block the parser had to recover because its `{/…}` was missing has
    /// no closing marker to check.
    MustacheSpacing,
    svelte,
    style,
    config = MustacheSpacing,
    version = "1.80.0",
    short_description = "Enforce consistent spacing inside `{…}`.",
);

impl Rule for MustacheSpacing {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for MustacheSpacing {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut out = Vec::new();
        let opening = self.tags.opening_brace;
        let closing = self.tags.closing_brace;

        walk_svelte_nodes(nodes, &mut |node| match node {
            Node::Mustache(tag) if !tag.unterminated => {
                check_expression(tag.span, source, self.text_expressions, &mut out);
            }
            Node::Tag(tag) if !tag.unterminated => {
                check_braces(tag.span, source, opening, closing, true, &mut out);
            }
            Node::Element(element) => {
                for attribute in &element.attributes {
                    match &attribute.kind {
                        AttributeKind::Plain { value: Some(value), .. } => {
                            check_value(value, source, self.attributes_and_props, &mut out);
                        }
                        // `{foo}` and `{...props}` are braces of their own.
                        AttributeKind::Shorthand { .. } | AttributeKind::Spread { .. } => {
                            check_expression(
                                attribute.span,
                                source,
                                self.attributes_and_props,
                                &mut out,
                            );
                        }
                        AttributeKind::Directive(directive) => {
                            if let Some(value) = &directive.value {
                                check_value(value, source, self.directive_expressions, &mut out);
                            }
                        }
                        // A comment between attributes has no `{…}`.
                        AttributeKind::Plain { .. } | AttributeKind::Comment { .. } => {}
                    }
                }
            }
            Node::Block(block) => {
                match &block.kind {
                    BlockKind::If(if_block) => {
                        for branch in &if_block.branches {
                            let has_expression = branch.expression.is_some();
                            check_braces(
                                branch.header_span,
                                source,
                                opening,
                                closing,
                                has_expression,
                                &mut out,
                            );
                        }
                    }
                    BlockKind::Each(each) => {
                        check_braces(each.header_span, source, opening, closing, true, &mut out);
                        if let Some(marker) = each.fallback_header_span {
                            check_braces(marker, source, opening, closing, false, &mut out);
                        }
                    }
                    BlockKind::Await(await_block) => {
                        check_braces(
                            await_block.header_span,
                            source,
                            opening,
                            closing,
                            true,
                            &mut out,
                        );
                        for (marker, pattern) in [
                            (await_block.then_header_span, &await_block.then_pattern),
                            (await_block.catch_header_span, &await_block.catch_pattern),
                        ] {
                            let Some(marker) = marker else { continue };
                            if pattern.is_some_and(|slot| !slot.text.trim().is_empty()) {
                                check_braces(marker, source, opening, closing, true, &mut out);
                            } else {
                                // A bare `{:then}` binds nothing, and upstream
                                // then leaves its closing brace unchecked.
                                check_opening_only(marker, source, opening, &mut out);
                            }
                        }
                    }
                    BlockKind::Key(key) => {
                        check_braces(key.header_span, source, opening, closing, true, &mut out);
                    }
                    BlockKind::Snippet(snippet) => {
                        check_braces(snippet.header_span, source, opening, closing, true, &mut out);
                    }
                    BlockKind::Unknown(unknown) => {
                        check_braces(unknown.header_span, source, opening, closing, true, &mut out);
                    }
                }
                if let Some(marker) = block.close_span {
                    check_braces(marker, source, opening, closing, false, &mut out);
                }
            }
            _ => {}
        });
        let diagnostics = out;
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

fn check_value(
    value: &AttributeValue<'_>,
    source: &str,
    spacing: Spacing,
    out: &mut Vec<OxcDiagnostic>,
) {
    for part in &value.parts {
        if let ValuePart::Expression(tag) = part
            && !tag.unterminated
        {
            check_expression(tag.span, source, spacing, out);
        }
    }
}

/// A plain `{expr}`, where one setting governs both braces.
fn check_expression(span: Span, source: &str, spacing: Spacing, out: &mut Vec<OxcDiagnostic>) {
    let closing = match spacing {
        Spacing::Never => ClosingSpacing::Never,
        Spacing::Always => ClosingSpacing::Always,
    };
    check_braces(span, source, spacing, closing, false, out);
}

/// The whitespace runs just inside a `{…}`, as `(after `{`, before `}`)`.
fn inner_padding(span: Span, source: &str) -> Option<(usize, usize)> {
    let text = source.get(span.start as usize..span.end as usize)?;
    let content = text.strip_prefix('{')?.strip_suffix('}')?;
    let leading = content.len() - content.trim_start().len();
    let trailing = content.len() - content.trim_end().len();
    Some((leading, trailing))
}

fn check_braces(
    span: Span,
    source: &str,
    opening: Spacing,
    closing: ClosingSpacing,
    has_expression: bool,
    out: &mut Vec<OxcDiagnostic>,
) {
    let Some((leading, trailing)) = inner_padding(span, source) else { return };
    push_opening(span, leading, opening, out);

    let want_space = closing == ClosingSpacing::Always
        || (closing == ClosingSpacing::AlwaysAfterExpression && has_expression);
    if want_space {
        if trailing == 0 {
            out.push(expected_closing_diagnostic(Span::new(span.end - 1, span.end)));
        }
    } else if trailing > 0 {
        let start = span.end - 1 - u32::try_from(trailing).unwrap_or(0);
        out.push(unexpected_closing_diagnostic(Span::new(start, span.end)));
    }
}

/// Check only the opening brace, for the one marker shape upstream leaves
/// half-checked: a `{:then}` / `{:catch}` with no binding.
fn check_opening_only(span: Span, source: &str, opening: Spacing, out: &mut Vec<OxcDiagnostic>) {
    if let Some((leading, _)) = inner_padding(span, source) {
        push_opening(span, leading, opening, out);
    }
}

fn push_opening(span: Span, leading: usize, opening: Spacing, out: &mut Vec<OxcDiagnostic>) {
    match opening {
        Spacing::Always if leading == 0 => {
            out.push(expected_opening_diagnostic(Span::new(span.start, span.start + 1)));
        }
        Spacing::Never if leading > 0 => {
            let end = span.start + 1 + u32::try_from(leading).unwrap_or(0);
            out.push(unexpected_opening_diagnostic(Span::new(span.start, end)));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::MustacheSpacing;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let always_text = || Some(serde_json::json!([{ "textExpressions": "always" }]));
        let always_attrs = || Some(serde_json::json!([{ "attributesAndProps": "always" }]));
        let always_directives = || Some(serde_json::json!([{ "directiveExpressions": "always" }]));
        let always_tags = || {
            Some(serde_json::json!([
                { "tags": { "openingBrace": "always", "closingBrace": "always" } }
            ]))
        };
        let after_expression = || {
            Some(serde_json::json!([
                { "tags": { "closingBrace": "always-after-expression" } }
            ]))
        };
        let pass = vec![
            ("{count}", None, None, path()),
            ("<div foo={bar}></div>", None, None, path()),
            ("<div {...props}></div>", None, None, path()),
            ("<Widget {foo} />", None, None, path()),
            ("<button on:click={handler}></button>", None, None, path()),
            ("{@html raw}", None, None, path()),
            ("{#if a}x{/if}", None, None, path()),
            ("{#if a}x{:else if b}y{:else}z{/if}", None, None, path()),
            ("{#each xs as x}{x}{:else}none{/each}", None, None, path()),
            ("{#await p}a{:then v}{v}{:catch e}{e}{/await}", None, None, path()),
            ("{#key k}x{/key}", None, None, path()),
            ("{#snippet row(a)}x{/snippet}", None, None, path()),
            ("{ count }", always_text(), None, path()),
            ("<div foo={ bar }></div>", always_attrs(), None, path()),
            ("<button on:click={ handler }></button>", always_directives(), None, path()),
            ("{ #if a }x{ /if }", always_tags(), None, path()),
            // Only the markers that carry an expression get the space.
            ("{#if a }x{/if}", after_expression(), None, path()),
        ];
        let fail = vec![
            ("{ count }", None, None, path()),
            ("{count }", None, None, path()),
            ("{ count}", None, None, path()),
            ("<div foo={ bar }></div>", None, None, path()),
            ("<div {... props }></div>", None, None, path()),
            ("<button on:click={ handler }></button>", None, None, path()),
            ("{@html raw }", None, None, path()),
            ("{#if a }x{/if }", None, None, path()),
            ("{#if a}x{:else if b }y{ :else}z{/if}", None, None, path()),
            ("{#each xs as x}{x}{ :else}none{/each}", None, None, path()),
            ("{#await p}a{:then v }{v}{/await}", None, None, path()),
            ("{count}", always_text(), None, path()),
            ("{#if a}x{/if}", always_tags(), None, path()),
            ("{#if a}x{/if}", after_expression(), None, path()),
        ];

        Tester::new(MustacheSpacing::NAME, MustacheSpacing::PLUGIN, pass, fail).test_and_snapshot();
    }
}
