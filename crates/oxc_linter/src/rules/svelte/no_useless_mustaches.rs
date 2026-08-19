use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, DirectiveKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn no_useless_mustaches_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected mustache interpolation with a string literal value.")
        .with_help("Write the string content directly, without the wrapping mustache.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoUselessMustaches;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows unnecessary mustache interpolations: mustaches whose only
    /// content is a string literal, or a template literal without
    /// interpolations.
    ///
    /// ### Why is this bad?
    ///
    /// A mustache wrapping a constant string is redundant — the text can be
    /// written directly, which is shorter and clearer.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {'Hello'}
    /// {`Hello`}
    /// <div data-text={'foo'} />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// Hello
    /// {'Hello' + name}
    /// {`Hello ${name}`}
    /// <div data-text="foo" />
    /// ```
    NoUselessMustaches,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Disallow unnecessary mustache interpolations.",
);

/// Skip JavaScript trivia (whitespace and comments) at the start of `text`.
/// Returns `None` on an unterminated block comment.
fn skip_trivia(text: &str) -> Option<&str> {
    let mut rest = text.trim_start();
    loop {
        if let Some(comment) = rest.strip_prefix("/*") {
            let (_, after) = comment.split_once("*/")?;
            rest = after.trim_start();
        } else if let Some(comment) = rest.strip_prefix("//") {
            rest = comment.trim_start_matches(|c| c != '\n' && c != '\r').trim_start();
        } else {
            return Some(rest);
        }
    }
}

/// Lex a complete string or template literal at the start of `text`.
/// Returns the raw text between the delimiters, whether it is a template
/// literal, and the remaining text after the closing delimiter.
fn lex_string_literal(text: &str) -> Option<(&str, bool, &str)> {
    let mut chars = text.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '\'' && quote != '"' && quote != '`' {
        return None;
    }
    let is_template = quote == '`';
    while let Some((index, c)) = chars.next() {
        if c == '\\' {
            // An escape sequence: the escaped character is part of the raw
            // value whatever it is.
            chars.next()?;
        } else if c == quote {
            return Some((&text[1..index], is_template, &text[index + 1..]));
        } else if is_template && c == '$' && text[index + 1..].starts_with('{') {
            // `${…}` interpolation: the mustache is not useless.
            return None;
        } else if !is_template && (c == '\n' || c == '\r') {
            // A raw line break in a non-template string is a syntax error;
            // this is not a plain string literal.
            return None;
        }
    }
    None
}

/// Whether `expression` — the raw text between the mustache braces — is
/// exactly one string literal or substitution-free template literal.
///
/// Mirrors upstream `svelte/no-useless-mustaches` semantics: surrounding
/// comments are permitted (and, matching upstream defaults, do not suppress
/// the report), multiline template literals are kept, and any `{` in the raw
/// value keeps the mustache since the brace could not be written as plain
/// text. Upstream inspects a parsed JS expression; this port classifies the
/// raw expression text with a small lexer instead.
fn is_useless_mustache(expression: &str) -> bool {
    let Some(rest) = skip_trivia(expression) else { return false };
    let Some((raw, is_template, tail)) = lex_string_literal(rest) else { return false };
    let Some(tail) = skip_trivia(tail) else { return false };
    if !tail.is_empty() {
        return false;
    }
    // Upstream keeps multiline template literals.
    if is_template && raw.contains(['\n', '\r']) {
        return false;
    }
    // A `{` in the raw value needs the mustache to be written at all
    // (e.g. `{'{foo'}`), so upstream keeps it.
    !raw.contains('{')
}

fn collect_value_spans(value: &AttributeValue<'_>, spans: &mut Vec<Span>) {
    for part in &value.parts {
        if let ValuePart::Expression(expression) = part
            && !expression.unterminated
            && is_useless_mustache(expression.expression)
        {
            spans.push(expression.span);
        }
    }
}

impl Rule for NoUselessMustaches {}

// Deviations from upstream `svelte/no-useless-mustaches`:
// - The `ignoreIncludesComment` and `ignoreStringEscape` options are not
//   supported; upstream defaults (both `false`) apply, so neither comments
//   inside the mustache nor escape sequences suppress the report.
// - No autofix is provided.
impl SvelteTemplateRule for NoUselessMustaches {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut spans = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| match node {
            // `{'str'}` in text position. `{@html 'str'}` and friends are
            // `Node::Tag`s, which upstream (kind `raw`) skips too.
            Node::Mustache(tag) => {
                if !tag.unterminated && is_useless_mustache(tag.expression) {
                    spans.push(tag.span);
                }
            }
            Node::Element(element) => {
                for attribute in &element.attributes {
                    match &attribute.kind {
                        AttributeKind::Plain { value: Some(value), .. } => {
                            collect_value_spans(value, &mut spans);
                        }
                        // `style:prop="{'v'}"`: upstream checks style
                        // directive values as well; every other directive
                        // kind takes a bare expression rather than a
                        // mustache tag, so those are not visited upstream.
                        AttributeKind::Directive(directive)
                            if directive.kind == DirectiveKind::Style =>
                        {
                            if let Some(value) = &directive.value {
                                collect_value_spans(value, &mut spans);
                            }
                        }
                        // An unquoted mixed value like `attr=a{'b c'}d`
                        // parses its `{…}` part as a shorthand attribute in
                        // this markup AST; upstream reports these mustaches
                        // too.
                        AttributeKind::Shorthand { name, .. } if is_useless_mustache(name) => {
                            spans.push(attribute.span);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        });
        for span in spans {
            ctx.diagnostic(no_useless_mustaches_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUselessMustaches;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("foo 'foo'", None, None, Some(PathBuf::from("test.svelte"))),
            ("{foo}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{'foo' || 'bar'}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{1}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{null}", None, None, Some(PathBuf::from("test.svelte"))),
            // Template literal with an interpolation.
            (r"{`foo${foo}`}", None, None, Some(PathBuf::from("test.svelte"))),
            // The `{` cannot be written outside a mustache.
            ("{'{foo'}", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<div data-text=\"foo 'foo' {foo} {'foo' || 'bar'} {1} {null}\" />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Multiline template literals are kept.
            (
                "<HyperMD\n\tvalue={`# Doc\n\nText goes here`}\n/>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `{@html}` is a raw tag, not an interpolation mustache.
            ("{@html '<br>'}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{}", None, None, Some(PathBuf::from("test.svelte"))),
        ];
        let fail = vec![
            (
                "{'space '}\n{' space'}\n{' space '}\n{'  '}\n\n<div data-text=\"{'space '} {' space'} {' space '} {'  '}\" />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Escape sequences do not suppress the report (upstream default).
            (
                "{'\\n'}\n{'\\r'}\n\n<div data-text=\"{'\\r'} {'\\n'}\" />\n\n{'\\\\\\\\'}\n{'\\\\r'}\n{'\\\\'}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div data-text={'a'} />\n<div data-text={'a b'} />\n<div data-text=\"a{'b c'}d\" />\n<div data-text=a{\"b c\"}d />\n<div data-text={'\"<br>\"'} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Strings containing quote characters are still reported.
            (
                "<div data-text=\"{'\\'\"'}\" />\n<div data-text=\"{\"'\\\"\"}\" />\n<div data-text='{'\\'\"'}' />\n<div data-text='{\"'\\\"\"}' />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{'foo'}\n{`foo`}\n\n<div data-text=\"{'foo'} '\" />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            ("{'<br>'}", None, None, Some(PathBuf::from("test.svelte"))),
            // Comments inside the mustache do not suppress the report
            // (upstream default).
            (
                "<div\n\tdata-text=\"{/* comment */ 'comment'} {// comment\n\t'comment'} \"\n/>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside blocks.
            ("{#if a}{'str'}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        Tester::new(NoUselessMustaches::NAME, NoUselessMustaches::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
