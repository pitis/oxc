use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct CommentDirective;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Supports `<!-- eslint-disable -->`-style comment directives in Svelte
    /// markup.
    ///
    /// In eslint-plugin-svelte this rule is the machinery that makes HTML
    /// comment directives work (its processor requires the rule to be
    /// enabled, which is why every shared config includes it). In this
    /// linter the directive handling is built into the Svelte markup pass
    /// itself and is always active — `<!-- eslint-disable … -->`,
    /// `<!-- eslint-enable … -->`, `<!-- eslint-disable-line … -->`, and
    /// `<!-- eslint-disable-next-line … -->` (plus the `oxlint-` spellings)
    /// suppress `svelte/*` markup diagnostics whether or not this rule is
    /// configured.
    ///
    /// The rule is registered so real-world configurations (which enable
    /// `svelte/comment-directive` via the recommended preset) resolve
    /// without errors; enabling it changes nothing and it never reports.
    ///
    /// Deviation from upstream: the `reportUnusedDisableDirectives` option
    /// is not yet supported — unused disable directives are not flagged.
    ///
    /// ### Why is this bad?
    ///
    /// Not applicable — see above; this rule exists for configuration
    /// compatibility and never reports.
    ///
    /// ### Examples
    ///
    /// ```svelte
    /// <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    /// {@html content}
    /// ```
    CommentDirective,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Support `<!-- eslint-disable -->` comment directives in markup.",
);

impl Rule for CommentDirective {}

impl SvelteTemplateRule for CommentDirective {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {
        // Intentionally empty: directive semantics live in the markup pass
        // (`svelte_template.rs` + the shared `TemplateCommentDirectives`
        // machinery), always on. See the rule docs.
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::CommentDirective;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        // The rule itself never reports; directive behavior is exercised
        // end-to-end in the other svelte rules' pass cases (e.g. the
        // suppression case in `no_at_html_tags`).
        let pass = vec![
            ("<div>{content}</div>", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<!-- eslint-disable svelte/no-at-html-tags -->\n{@html content}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![];

        Tester::new(CommentDirective::NAME, CommentDirective::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
