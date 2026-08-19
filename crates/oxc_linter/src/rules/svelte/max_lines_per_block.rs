use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_start_tag_span, walk_svelte_elements},
};

fn max_lines_per_block_diagnostic(
    block: &str,
    line_count: usize,
    max: usize,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "{block} block has too many lines ({line_count}). Maximum allowed is {max}."
    ))
    .with_help("Split the block up, or raise the rule's limit.")
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct MaxLinesPerBlockConfig {
    /// Maximum lines inside a `<script>` block. Unset means unlimited.
    script: Option<usize>,
    /// Maximum lines of markup outside the `<script>` and `<style>` blocks.
    template: Option<usize>,
    /// Maximum lines inside a `<style>` block.
    style: Option<usize>,
    /// Skip blank lines when counting.
    skip_blank_lines: bool,
    /// Skip lines that hold nothing but a comment.
    skip_comments: bool,
}

// Boxed: three `Option<usize>` plus two flags exceed `RuleEnum`'s 16 bytes.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct MaxLinesPerBlock(Box<MaxLinesPerBlockConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Caps the number of lines in a component's `<script>`, `<style>` and
    /// template sections.
    ///
    /// ### Why is this bad?
    ///
    /// Long blocks are hard to hold in your head, and in a single-file
    /// component they are the signal that the component is doing several
    /// jobs and wants splitting.
    ///
    /// ### Examples
    ///
    /// With `["error", { "script": 2 }]`:
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let a = 1;
    ///   let b = 2;
    ///   let c = 3;
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let a = 1;
    ///   let b = 2;
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// `script`, `template` and `style` set the per-section limits (each
    /// unlimited when unset); `skipBlankLines` and `skipComments` (both
    /// `false` by default) exclude those lines from the count.
    ///
    /// ```json
    /// {
    ///   "svelte/max-lines-per-block": ["error", { "script": 100, "skipBlankLines": true }]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// `skipComments` recognises `//` and `/* … */` in a `<script>`, `/* … */`
    /// in a `<style>`, and `<!-- … -->` in the template, matched lexically
    /// rather than through a JS/CSS tokenizer. A `//` inside a string or a
    /// regular expression can therefore be mistaken for a comment.
    MaxLinesPerBlock,
    svelte,
    style,
    config = MaxLinesPerBlock,
    version = "1.80.0",
    short_description = "Enforce a maximum number of lines per component block.",
);

impl Rule for MaxLinesPerBlock {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for MaxLinesPerBlock {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let config = &self.0;
        if config.script.is_none() && config.style.is_none() && config.template.is_none() {
            return;
        }
        let source = ctx.source_text();
        let lines: Vec<&str> = source.lines().collect();
        let line_of = |offset: u32| -> usize {
            // 1-based, like the reported line numbers.
            source[..(offset as usize).min(source.len())].lines().count().max(1)
        };

        let mut diagnostics = Vec::new();
        // 1-based lines belonging to a `<script>` or `<style>` block, which
        // the template count excludes.
        let mut block_lines: FxHashSet<usize> = FxHashSet::default();

        walk_svelte_elements(nodes, &mut |element| {
            let (max, label, comment_style) = if element.name.eq_ignore_ascii_case("script") {
                (config.script, "<script>", CommentStyle::Script)
            } else if element.name.eq_ignore_ascii_case("style") {
                (config.style, "<style>", CommentStyle::Style)
            } else {
                return;
            };
            let start = line_of(element.span.start);
            let end = line_of(element.span.end);
            for line in start..=end {
                block_lines.insert(line);
            }
            let Some(max) = max else { return };
            // Upstream counts strictly between the opening and closing tag
            // lines.
            let count = count_lines(
                &lines,
                start,
                end,
                config.skip_blank_lines,
                config.skip_comments.then_some(comment_style),
            );
            if count > max {
                diagnostics.push(max_lines_per_block_diagnostic(
                    label,
                    count,
                    max,
                    svelte_start_tag_span(element),
                ));
            }
        });

        if let Some(max) = config.template {
            let mut count = 0;
            for (index, line) in lines.iter().enumerate() {
                let number = index + 1;
                if block_lines.contains(&number) {
                    continue;
                }
                if config.skip_blank_lines && line.trim().is_empty() {
                    continue;
                }
                if config.skip_comments && is_comment_line(line, CommentStyle::Template) {
                    continue;
                }
                count += 1;
            }
            if count > max {
                diagnostics.push(max_lines_per_block_diagnostic(
                    "template",
                    count,
                    max,
                    Span::empty(0),
                ));
            }
        }

        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CommentStyle {
    Script,
    Style,
    Template,
}

/// Whether the line holds nothing but a comment.
fn is_comment_line(line: &str, style: CommentStyle) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    match style {
        CommentStyle::Script => {
            trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*')
        }
        CommentStyle::Style => trimmed.starts_with("/*") || trimmed.starts_with('*'),
        CommentStyle::Template => trimmed.starts_with("<!--"),
    }
}

/// Count the lines strictly between `start` and `end` (both 1-based, and both
/// excluded, since they hold the block's tags).
fn count_lines(
    lines: &[&str],
    start: usize,
    end: usize,
    skip_blank_lines: bool,
    comment_style: Option<CommentStyle>,
) -> usize {
    if end <= start + 1 {
        return 0;
    }
    let mut count = 0;
    for number in (start + 1)..end {
        let Some(line) = lines.get(number - 1) else { continue };
        if skip_blank_lines && line.trim().is_empty() {
            continue;
        }
        if comment_style.is_some_and(|style| is_comment_line(line, style)) {
            continue;
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::MaxLinesPerBlock;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let script2 = || Some(serde_json::json!([{ "script": 2 }]));
        let pass = vec![
            // No limits configured.
            ("<script>\n\tlet a = 1;\n\tlet b = 2;\n\tlet c = 3;\n</script>", None, None, path()),
            ("<script>\n\tlet a = 1;\n\tlet b = 2;\n</script>", script2(), None, path()),
            // Blank lines can be excluded.
            (
                "<script>\n\tlet a = 1;\n\n\tlet b = 2;\n</script>",
                Some(serde_json::json!([{ "script": 2, "skipBlankLines": true }])),
                None,
                path(),
            ),
            // Comment-only lines can be excluded.
            (
                "<script>\n\t// a comment\n\tlet a = 1;\n\tlet b = 2;\n</script>",
                Some(serde_json::json!([{ "script": 2, "skipComments": true }])),
                None,
                path(),
            ),
            // The template count excludes the script and style blocks.
            (
                "<script>\n\tlet a = 1;\n\tlet b = 2;\n\tlet c = 3;\n</script>\n<p>x</p>",
                Some(serde_json::json!([{ "template": 1 }])),
                None,
                path(),
            ),
        ];
        let fail = vec![
            (
                "<script>\n\tlet a = 1;\n\tlet b = 2;\n\tlet c = 3;\n</script>",
                script2(),
                None,
                path(),
            ),
            (
                "<style>\n\ta {}\n\tb {}\n\tc {}\n</style>",
                Some(serde_json::json!([{ "style": 2 }])),
                None,
                path(),
            ),
            (
                "<p>a</p>\n<p>b</p>\n<p>c</p>",
                Some(serde_json::json!([{ "template": 2 }])),
                None,
                path(),
            ),
            // Blank lines count unless excluded.
            ("<script>\n\tlet a = 1;\n\n\tlet b = 2;\n</script>", script2(), None, path()),
        ];

        Tester::new(MaxLinesPerBlock::NAME, MaxLinesPerBlock::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
