use oxc_allocator::Allocator;
use oxc_ast::ast::TemplateElement;
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{for_each_svelte_expression, svelte_scripts, walk_svelte_nodes},
};

fn no_trailing_spaces_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Trailing spaces not allowed.")
        .with_help("Remove the whitespace at the end of the line.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoTrailingSpaces {
    /// Leave a line that is entirely whitespace alone.
    skip_blank_lines: bool,
    /// Leave the lines of an HTML comment alone.
    ignore_comments: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports whitespace at the end of a line, anywhere in a `.svelte`
    /// file — markup, `<script>` and `<style>` alike.
    ///
    /// ### Why is this bad?
    ///
    /// Trailing whitespace is invisible, shows up as noise in diffs, and
    /// some editors strip it on save while others do not.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div>hello</div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div>hello</div>
    /// ```
    ///
    /// ### Options
    ///
    /// `skipBlankLines` (default `false`) leaves a line that is entirely
    /// whitespace alone; `ignoreComments` (default `false`) leaves the lines
    /// of an HTML comment alone.
    ///
    /// ```json
    /// { "svelte/no-trailing-spaces": ["error", { "skipBlankLines": true }] }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream removes the whitespace; the Svelte markup pass reports only.
    NoTrailingSpaces,
    svelte,
    style,
    config = NoTrailingSpaces,
    version = "1.80.0",
    short_description = "Disallow trailing whitespace at the end of lines.",
);

impl Rule for NoTrailingSpaces {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for NoTrailingSpaces {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        // Lines whose trailing whitespace is inside a template literal, where
        // removing it would change the string. As upstream does, the quasi's
        // own last line is not skipped: it is where the `${` or the closing
        // backtick sits.
        let mut ignored = IgnoredLines::new(source);
        ignored.collect_template_literals(nodes, source);
        if self.ignore_comments {
            walk_svelte_nodes(nodes, &mut |node| {
                if let Node::Comment(comment) = node {
                    ignored.add_span_except_last_line(comment.span);
                }
            });
        }

        let mut offset = 0u32;
        for (index, line) in source.split('\n').enumerate() {
            let line_start = offset;
            // `split` drops the separator, so step past it for the next line.
            offset += u32::try_from(line.len()).unwrap_or(0) + 1;
            if self.skip_blank_lines && line.trim().is_empty() {
                continue;
            }
            // A CRLF file leaves the `\r` on the line; it is trailing
            // whitespace like any other, which is what upstream reports too.
            let trimmed = line.trim_end();
            if trimmed.len() == line.len() || ignored.contains(index) {
                continue;
            }
            ctx.diagnostic(no_trailing_spaces_diagnostic(Span::new(
                line_start + u32::try_from(trimmed.len()).unwrap_or(0),
                line_start + u32::try_from(line.len()).unwrap_or(0),
            )));
        }
    }
}

/// The zero-based lines the rule skips.
struct IgnoredLines {
    /// The file offset each line starts at, ascending.
    line_starts: Vec<u32>,
    lines: FxHashSet<usize>,
}

impl IgnoredLines {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source.match_indices('\n').filter_map(|(index, _)| u32::try_from(index + 1).ok()),
        );
        Self { line_starts, lines: FxHashSet::default() }
    }

    fn line_of(&self, offset: u32) -> usize {
        self.line_starts.partition_point(|&start| start <= offset).saturating_sub(1)
    }

    fn contains(&self, line: usize) -> bool {
        self.lines.contains(&line)
    }

    /// Mark every line the span covers except its last — the last is where
    /// the delimiter that ends the run sits, so it is ordinary source.
    fn add_span_except_last_line(&mut self, span: Span) {
        for line in self.line_of(span.start)..self.line_of(span.end) {
            self.lines.insert(line);
        }
    }

    /// Mark the lines of every template-literal text run in the component,
    /// in the `<script>` blocks and in the markup expressions alike.
    fn collect_template_literals(&mut self, nodes: &[Node<'_>], source: &str) {
        let allocator = Allocator::new();
        for script in svelte_scripts(nodes, source) {
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let parsed = Parser::new(&allocator, script.content, source_type).parse();
            let mut collector = QuasiSpans::default();
            collector.visit_program(&parsed.program);
            self.add_all(&collector.spans, script.offset);
        }
        for_each_svelte_expression(nodes, &mut |text, span| {
            let allocator = Allocator::new();
            let Ok(expression) = Parser::new(&allocator, text, SourceType::ts()).parse_expression()
            else {
                return;
            };
            let mut collector = QuasiSpans::default();
            collector.visit_expression(&expression);
            self.add_all(&collector.spans, span.start);
        });
    }

    fn add_all(&mut self, spans: &[Span], offset: u32) {
        for span in spans {
            self.add_span_except_last_line(Span::new(span.start + offset, span.end + offset));
        }
    }
}

/// Collects the span of every template-literal text run — the part a trailing
/// space would belong to, as opposed to an interpolated expression.
#[derive(Default)]
struct QuasiSpans {
    spans: Vec<Span>,
}

impl<'a> Visit<'a> for QuasiSpans {
    fn visit_template_element(&mut self, element: &TemplateElement<'a>) {
        self.spans.push(element.span);
        walk::walk_template_element(self, element);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoTrailingSpaces;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let skip_blank = || Some(serde_json::json!([{ "skipBlankLines": true }]));
        let ignore_comments = || Some(serde_json::json!([{ "ignoreComments": true }]));
        let pass = vec![
            ("<div>hello</div>\n", None, None, path()),
            ("<div>hello</div>", None, None, path()),
            ("<div>\n\thello\n</div>\n", None, None, path()),
            ("<div>a</div>\n   \n<div>b</div>\n", skip_blank(), None, path()),
            ("<!-- a   \n b -->\n", ignore_comments(), None, path()),
            // A trailing space inside a template literal is string content.
            ("<script>\n\tconst a = `x   \n\ty`;\n</script>\n", None, None, path()),
            ("{`a   \nb`}\n", None, None, path()),
        ];
        let fail = vec![
            ("<div>hello</div>   \n", None, None, path()),
            ("<div>a</div>\n   \n<div>b</div>\n", None, None, path()),
            ("<script>\n\tconst a = 1;   \n</script>\n", None, None, path()),
            ("<style>\n\t.a {}   \n</style>\n", None, None, path()),
            ("<!-- a   \n b -->\n", None, None, path()),
            // The quasi's own last line is still checked, as upstream has it.
            ("<script>\n\tconst a = `x\n\ty`;   \n</script>\n", None, None, path()),
        ];

        Tester::new(NoTrailingSpaces::NAME, NoTrailingSpaces::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
