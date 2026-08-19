use oxc_allocator::Allocator;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::svelte_style_blocks,
};

fn parse_error_diagnostic(message: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Error parsing style element. Error message: \"{message}\""))
        .with_help("Fix the CSS so the component's `<style>` block parses.")
        .with_label(span)
}

fn unknown_lang_diagnostic(lang: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Found unsupported style element language \"{lang}\""))
        .with_help("Use `css`, `scss`, `sass` or `less`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidStyleParse;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports a component `<style>` block that does not parse, and a `lang`
    /// naming a language the linter cannot read.
    ///
    /// ### Why is this bad?
    ///
    /// A `<style>` block that fails to parse breaks the build — and until it
    /// does, every other CSS-aware rule silently skips the component, so
    /// problems hide behind the syntax error.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <style>
    ///   .a { color: red;
    /// </style>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <style>
    ///   .a { color: red; }
    /// </style>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// - Upstream reports PostCSS's own error text after running the block
    ///   through the configured preprocessor; oxlint parses the block directly
    ///   with `oxc-css-parser`, so the wording differs.
    /// - Upstream treats `lang="less"` and `lang="sass"` as unsupported
    ///   because its pipeline needs a preprocessor for them. oxlint's parser
    ///   reads both natively, so it parses them instead of reporting — saying
    ///   "unsupported" about a block it just parsed would be untrue. Only a
    ///   language it genuinely cannot read (`stylus`, a custom preprocessor)
    ///   is reported.
    /// - Upstream memoises its style context on the *first* `<style>` element,
    ///   so a second block silently reuses the first block's result. Every
    ///   block is checked here.
    ValidStyleParse,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Report `<style>` blocks that do not parse.",
);

impl Rule for ValidStyleParse {}

impl SvelteTemplateRule for ValidStyleParse {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut diagnostics = Vec::new();
        for block in svelte_style_blocks(nodes, ctx.source_text()) {
            let Some(_) = block.syntax else {
                // `lang` names something this parser does not read.
                diagnostics
                    .push(unknown_lang_diagnostic(block.lang.unwrap_or_default(), block.tag_span));
                continue;
            };
            let allocator = Allocator::new();
            // Upstream surfaces the single error its style context carries, so
            // report the first problem per block rather than a cascade.
            let error = match block.parse(&allocator) {
                Err(error) => Some(error),
                Ok((_, recoverable)) => recoverable.into_iter().next(),
            };
            if let Some(error) = error {
                diagnostics.push(parse_error_diagnostic(
                    &error.kind.to_string(),
                    block.absolute(error.span),
                ));
            }
        }
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidStyleParse;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<style>\n\t.a { color: red; }\n</style>", None, None, path()),
            ("<style lang=\"scss\">\n\t.a { .b { color: red; } }\n</style>", None, None, path()),
            (
                "<style lang=\"less\">\n\t@x: red;\n\t.a { color: @x; }\n</style>",
                None,
                None,
                path(),
            ),
            // No style block at all.
            ("<div>x</div>", None, None, path()),
            ("<style></style>", None, None, path()),
            // An empty value parses, exactly as PostCSS tolerates it.
            ("<style>\n\t.a { color: ; }\n</style>", None, None, path()),
        ];
        let fail = vec![
            ("<style>\n\t.a { color: red;\n</style>", None, None, path()),
            ("<style>\n\t.a { color: red } }\n</style>", None, None, path()),
            ("<style lang=\"stylus\">\n\t.a\n\t\tcolor red\n</style>", None, None, path()),
        ];

        Tester::new(ValidStyleParse::NAME, ValidStyleParse::PLUGIN, pass, fail).test_and_snapshot();
    }
}
