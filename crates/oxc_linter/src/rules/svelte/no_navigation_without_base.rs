use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, TemplateLiteral};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashSet;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, svelte_scripts, walk_svelte_elements, walk_svelte_nodes},
};

fn not_prefixed_diagnostic(what: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Found a {what} with a url that isn't prefixed with the base path."
    ))
    .with_help("Prefix the URL with `base` from `$app/paths`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoNavigationWithoutBase;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires SvelteKit navigation — `<a href>` links and `goto()` /
    /// `pushState()` / `replaceState()` calls — to prefix internal URLs with
    /// `base` from `$app/paths`.
    ///
    /// ### Why is this bad?
    ///
    /// An app served under a base path (`/docs`, say) needs every internal
    /// URL to carry that prefix; a bare `/foo` navigates out of the app.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { goto } from '$app/navigation';
    ///   goto('/foo');
    /// </script>
    ///
    /// <a href="/foo">Click me!</a>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { goto } from '$app/navigation';
    ///   import { base } from '$app/paths';
    ///   goto(`${base}/foo`);
    /// </script>
    ///
    /// <a href="{base}/foo">Click me!</a>
    /// <a href="https://svelte.dev">External</a>
    /// ```
    ///
    /// ### Deprecated
    ///
    /// `eslint-plugin-svelte` deprecates this rule in favour of
    /// [`svelte/no-navigation-without-resolve`], which supersedes it for
    /// SvelteKit 2.26 and newer. It is kept so existing configurations keep
    /// resolving.
    ///
    /// [`svelte/no-navigation-without-resolve`]: ./no-navigation-without-resolve.html
    NoNavigationWithoutBase,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Disallow SvelteKit navigation without the `base` path (deprecated).",
);

impl Rule for NoNavigationWithoutBase {}

impl SvelteTemplateRule for NoNavigationWithoutBase {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let allocator = Allocator::new();
        let scripts = svelte_scripts(nodes, source_text);

        let base_names =
            base_path_names(&scripts.iter().map(|s| s.content).collect::<Vec<_>>(), &allocator);
        let mut diagnostics = Vec::new();

        // `goto()` / `pushState()` / `replaceState()`, wherever they appear.
        let mut expressions: Vec<(&str, u32)> = Vec::new();
        for script in &scripts {
            expressions.push((script.content, script.offset));
        }
        collect_template_expressions(nodes, &mut expressions);
        for (text, offset) in expressions {
            for (what, span) in scan_navigation_calls(text, offset, &allocator, &base_names, false)
            {
                diagnostics.push(not_prefixed_diagnostic(what, span));
            }
        }

        // `<a href>` links.
        walk_svelte_elements(nodes, &mut |element| {
            if element.name != "a" {
                return;
            }
            for attribute in &element.attributes {
                let AttributeKind::Plain { name: "href", value: Some(value), .. } = &attribute.kind
                else {
                    continue;
                };
                if !href_is_prefixed(value, &allocator, &base_names) {
                    diagnostics.push(not_prefixed_diagnostic("link", attribute.span));
                }
            }
        });

        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Local names bound to `base` (or `assets`) from `$app/paths`, across all
/// of the component's `<script>` blocks.
pub(super) fn base_path_names(scripts: &[&str], allocator: &Allocator) -> FxHashSet<String> {
    use oxc_ast::ast::{ImportDeclarationSpecifier, Statement};

    let mut names = FxHashSet::default();
    for script in scripts {
        let ret = oxc_parser::Parser::new(allocator, script, oxc_span::SourceType::ts()).parse();
        if ret.panicked {
            continue;
        }
        for statement in &ret.program.body {
            let Statement::ImportDeclaration(import) = statement else { continue };
            if import.source.value != "$app/paths" {
                continue;
            }
            let Some(specifiers) = &import.specifiers else { continue };
            for specifier in specifiers {
                if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier
                    && matches!(specifier.imported.name().as_str(), "base" | "assets")
                {
                    names.insert(specifier.local.name.to_string());
                }
            }
        }
    }
    names
}

/// Whether an `href` value starts with the base path (or is external).
fn href_is_prefixed(
    value: &AttributeValue<'_>,
    allocator: &Allocator,
    base_names: &FxHashSet<String>,
) -> bool {
    match value.parts.first() {
        // `href="/foo"` — a literal, allowed only if it carries a scheme.
        Some(ValuePart::Text(text)) => literal_is_allowed(text.value),
        // `href="{base}/foo"` — the leading expression must be the base.
        Some(ValuePart::Expression(expression)) => {
            let Some(parsed) = parse_svelte_expression(allocator, expression.expression) else {
                return false;
            };
            expression_is_prefixed(&parsed, base_names)
        }
        // An empty value navigates to the current URL.
        None => true,
    }
}

/// Upstream's literal test: anything with a scheme (`https:`, `mailto:`) is
/// external and allowed.
fn literal_is_allowed(value: &str) -> bool {
    let scheme_end = value.find(':');
    scheme_end.is_some_and(|end| value[..end].chars().all(|c| c.is_ascii_alphabetic() || c == '+'))
}

/// Whether the expression is built from the base path.
fn expression_is_prefixed(expression: &Expression<'_>, base_names: &FxHashSet<String>) -> bool {
    match expression.get_inner_expression() {
        Expression::Identifier(identifier) => base_names.contains(identifier.name.as_str()),
        // `base + '/foo'`
        Expression::BinaryExpression(binary) => expression_is_prefixed(&binary.left, base_names),
        // `` `${base}/foo` ``
        Expression::TemplateLiteral(template) => template_starts_with_base(template, base_names),
        Expression::StringLiteral(literal) => literal_is_allowed(literal.value.as_str()),
        _ => false,
    }
}

/// Whether a template literal opens with `${base}`.
fn template_starts_with_base(
    template: &TemplateLiteral<'_>,
    base_names: &FxHashSet<String>,
) -> bool {
    // The leading quasi must be empty, so the first thing in the URL is the
    // interpolated base.
    if !template.quasis.first().is_some_and(|quasi| quasi.value.raw.is_empty()) {
        return false;
    }
    template
        .expressions
        .first()
        .is_some_and(|expression| expression_is_prefixed(expression, base_names))
}

/// Report the SvelteKit navigation calls in `text` whose first argument is
/// not prefixed with the base path.
///
/// `text` is parsed as a program, which covers both a whole `<script>` body
/// and a single markup expression such as `() => goto('/x')`.
pub(super) fn scan_navigation_calls(
    text: &str,
    offset: u32,
    allocator: &Allocator,
    base_names: &FxHashSet<String>,
    only_goto: bool,
) -> Vec<(&'static str, Span)> {
    use oxc_ast_visit::{Visit, walk};

    struct Visitor<'v> {
        offset: u32,
        only_goto: bool,
        base_names: &'v FxHashSet<String>,
        found: Vec<(&'static str, Span)>,
    }

    impl<'a> Visit<'a> for Visitor<'_> {
        fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
            if let Some((what, argument)) = navigation_call(&call.callee, &call.arguments)
                && (!self.only_goto || what == "goto() call")
                && !expression_is_prefixed(argument, self.base_names)
            {
                self.found.push((what, shift(argument.span(), self.offset)));
            }
            walk::walk_call_expression(self, call);
        }
    }

    let ret = oxc_parser::Parser::new(allocator, text, oxc_span::SourceType::ts()).parse();
    if ret.panicked {
        return Vec::new();
    }
    let mut visitor = Visitor { offset, only_goto, base_names, found: Vec::new() };
    visitor.visit_program(&ret.program);
    visitor.found
}

/// The kind of navigation a callee names, with the URL argument.
fn navigation_call<'e, 'a>(
    callee: &Expression<'a>,
    arguments: &'e oxc_allocator::Vec<'a, oxc_ast::ast::Argument<'a>>,
) -> Option<(&'static str, &'e Expression<'a>)> {
    let Expression::Identifier(identifier) = callee.get_inner_expression() else { return None };
    let what = match identifier.name.as_str() {
        "goto" => "goto() call",
        "pushState" => "pushState() call",
        "replaceState" => "replaceState() call",
        _ => return None,
    };
    let argument = arguments.first()?.as_expression()?;
    Some((what, argument))
}

fn shift(span: Span, offset: u32) -> Span {
    Span::new(span.start + offset, span.end + offset)
}

/// Every `{…}` expression in the markup, with its file offset.
fn collect_template_expressions<'a>(nodes: &[Node<'a>], out: &mut Vec<(&'a str, u32)>) {
    walk_svelte_nodes(nodes, &mut |node| {
        if let Node::Element(element) = node {
            for attribute in &element.attributes {
                let value = match &attribute.kind {
                    AttributeKind::Plain { value, .. } => value.as_ref(),
                    AttributeKind::Directive(directive) => directive.value.as_ref(),
                    _ => None,
                };
                let Some(value) = value else { continue };
                for part in &value.parts {
                    if let ValuePart::Expression(expression) = part {
                        out.push((expression.expression, expression.expression_span.start));
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoNavigationWithoutBase;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\timport { base } from '$app/paths';\n\tgoto(`${base}/foo`);\n</script>",
                None,
                None,
                path(),
            ),
            (
                "<script>\n\timport { base } from '$app/paths';\n</script>\n<a href=\"{base}/foo\">x</a>",
                None,
                None,
                path(),
            ),
            // External links carry a scheme.
            ("<a href=\"https://svelte.dev\">x</a>", None, None, path()),
            ("<a href=\"mailto:a@b.c\">x</a>", None, None, path()),
            // Renamed import.
            (
                "<script>\n\timport { base as b } from '$app/paths';\n\timport { goto } from '$app/navigation';\n\tgoto(b + '/foo');\n</script>",
                None,
                None,
                path(),
            ),
        ];
        let fail = vec![
            (
                "<script>\n\timport { goto } from '$app/navigation';\n\tgoto('/foo');\n</script>",
                None,
                None,
                path(),
            ),
            ("<a href=\"/foo\">x</a>", None, None, path()),
            (
                "<script>\n\timport { pushState } from '$app/navigation';\n\tpushState('/foo', {});\n</script>",
                None,
                None,
                path(),
            ),
            (
                "<script>\n\timport { replaceState } from '$app/navigation';\n\treplaceState('/foo', {});\n</script>",
                None,
                None,
                path(),
            ),
            // A base-looking name that was never imported from `$app/paths`.
            (
                "<script>\n\tconst base = '/x';\n\timport { goto } from '$app/navigation';\n\tgoto(`${base}/foo`);\n</script>",
                None,
                None,
                path(),
            ),
        ];

        Tester::new(NoNavigationWithoutBase::NAME, NoNavigationWithoutBase::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
