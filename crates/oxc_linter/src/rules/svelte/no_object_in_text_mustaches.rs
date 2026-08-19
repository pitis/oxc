use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use svelte_markup_parser::ast::{
    AttributeKind, DirectiveKind, ExpressionTag, Node, TagKind, ValuePart,
};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_nodes,
};

fn no_object_in_text_mustaches_diagnostic(phrase: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Unexpected {phrase} in text mustache interpolation."))
        .with_help(
            "The mustache renders this value as text (e.g. `[object Object]`); interpolate an expression that produces a primitive value instead.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoObjectInTextMustaches;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows object, array, function, and class literals in text
    /// mustache interpolations.
    ///
    /// ### Why is this bad?
    ///
    /// A text mustache stringifies its value: an object renders as
    /// `[object Object]`, a function as its source text, and so on — never
    /// what was intended. It is usually a mistake for a props mustache,
    /// e.g. writing `<Component prop="{ foo }" />` (a text interpolation
    /// because of the quotes) instead of `<Component prop={foo}/>` or
    /// `<Component prop={{ foo }} />`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// {{ foo }}
    /// <input class="{{ foo }} bar" />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// {foo}
    /// <input class="{foo} bar" />
    /// <Component prop={{ foo }} />
    /// ```
    NoObjectInTextMustaches,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow objects in text mustache interpolation.",
);

impl Rule for NoObjectInTextMustaches {}

impl SvelteTemplateRule for NoObjectInTextMustaches {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut reports: Vec<(&'static str, Span)> = Vec::new();
        walk_svelte_nodes(nodes, &mut |node| match node {
            // `{expr}` in text position.
            Node::Mustache(tag) => check_expression_tag(tag, &mut reports),
            // `{@html expr}` is a mustache tag too upstream
            // (`SvelteMustacheTag` with kind `raw`), and the rule doesn't
            // filter on kind.
            Node::Tag(tag) if tag.kind == TagKind::Html => {
                if let Some(phrase) = mustache_phrase(tag.expression.trim_ascii()) {
                    reports.push((phrase, tag.span));
                }
            }
            Node::Element(element) => {
                for attribute in &element.attributes {
                    match &attribute.kind {
                        AttributeKind::Plain { value: Some(value), .. } => {
                            // Upstream skips a `SvelteAttribute` whose value
                            // is a single part ("maybe props"): only the
                            // mustaches of a mixed text/expression value are
                            // text interpolations.
                            if value.parts.len() == 1 {
                                continue;
                            }
                            for part in &value.parts {
                                if let ValuePart::Expression(tag) = part {
                                    check_expression_tag(tag, &mut reports);
                                }
                            }
                        }
                        // `style:` directive values are mustache tags
                        // upstream (unlike every other directive kind, whose
                        // expression is a direct AST node), and the
                        // single-part exemption above only applies to
                        // `SvelteAttribute` — so these are always checked.
                        AttributeKind::Directive(directive)
                            if directive.kind == DirectiveKind::Style =>
                        {
                            if let Some(value) = &directive.value {
                                for part in &value.parts {
                                    if let ValuePart::Expression(tag) = part {
                                        check_expression_tag(tag, &mut reports);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        });
        for (phrase, span) in reports {
            ctx.diagnostic(no_object_in_text_mustaches_diagnostic(phrase, span));
        }
    }
}

/// Check one `{expr}` tag, reporting on the whole mustache span (including
/// the braces), matching upstream's report on the `SvelteMustacheTag` node.
fn check_expression_tag(tag: &ExpressionTag<'_>, reports: &mut Vec<(&'static str, Span)>) {
    let (text, _) = tag.trimmed();
    if let Some(phrase) = mustache_phrase(text) {
        reports.push((phrase, tag.span));
    }
}

/// Parse the mustache's expression text and map its top-level node to
/// upstream's `PHRASES` table; `None` for every type the rule doesn't check
/// (and for text that doesn't parse as an expression). Parsed with
/// `preserve_parens: false` so `{({ foo })}` is still an object expression,
/// as in ESTree.
fn mustache_phrase(text: &str) -> Option<&'static str> {
    let allocator = Allocator::new();
    let expression = Parser::new(&allocator, text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
        .ok()?;
    match expression {
        Expression::ObjectExpression(_) => Some("object"),
        Expression::ArrayExpression(_) => Some("array"),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
            Some("function")
        }
        Expression::ClassExpression(_) => Some("class"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoObjectInTextMustaches;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("{a}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{a ? 'b' : 'c'}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{fn(1)}", None, None, Some(PathBuf::from("test.svelte"))),
            ("<input class=\"{a} a\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // A single-mustache attribute value may be props: skipped.
            ("<MyComponent prop={{ a }} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<MyComponent prop=\"{{ a }}\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // Directive expressions (other than `style:`) are not mustache
            // tags upstream.
            ("<input class:foo={{ a }} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("{#if a}{b}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        let fail = vec![
            ("{{ a }}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{[a]}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{() => a}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{function () {\n\treturn a;\n}}", None, None, Some(PathBuf::from("test.svelte"))),
            ("{class A {}}", None, None, Some(PathBuf::from("test.svelte"))),
            // A multi-part attribute value is text interpolation.
            ("<input class=\"{{ a }} a\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested inside blocks.
            ("{#if x}{{ a }}{/if}", None, None, Some(PathBuf::from("test.svelte"))),
            // `style:` directive values are string interpolations even when
            // a single mustache.
            ("<div style:color={{ a }} />", None, None, Some(PathBuf::from("test.svelte"))),
            // `{@html}` is a (raw) mustache tag upstream.
            ("{@html { a }}", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        Tester::new(NoObjectInTextMustaches::NAME, NoObjectInTextMustaches::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
