use oxc_macros::declare_oxc_lint;
use vue_sfc_parser::ast::Node;

use crate::{
    rule::Rule,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct CommentDirective;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Supports `<!-- eslint-disable -->`-style comment directives in Vue
    /// templates.
    ///
    /// In eslint-plugin-vue this rule *is* the machinery that makes HTML
    /// comment directives work — its processor requires the rule to be
    /// enabled, which is why every shared config includes it. Here the
    /// directive handling is built into the Vue template pass itself and is
    /// always active: `<!-- eslint-disable … -->`, `<!-- eslint-enable … -->`,
    /// `<!-- eslint-disable-line … -->` and `<!-- eslint-disable-next-line … -->`
    /// (plus the `oxlint-` spellings) suppress `vue/*` template diagnostics
    /// whether or not this rule is configured. Directives written at the top
    /// level of the file, outside every block, are collected too — the same
    /// thing upstream's `extractTopLevelDocumentFragmentComments` does.
    ///
    /// The rule is registered so a real configuration (which enables
    /// `vue/comment-directive` through the recommended preset) resolves
    /// without errors; enabling it changes nothing and it never reports.
    ///
    /// Deviation from upstream: the `reportUnusedDisableDirectives` option is
    /// not supported — an unused disable directive is not flagged.
    ///
    /// ### Why is this bad?
    ///
    /// Not applicable — see above; this rule exists for configuration
    /// compatibility and never reports.
    ///
    /// ### Examples
    ///
    /// ```vue
    /// <template>
    ///   <!-- eslint-disable-next-line vue/no-v-html -->
    ///   <div v-html="content" />
    /// </template>
    /// ```
    CommentDirective,
    vue,
    correctness,
    version = "1.80.0",
    short_description = "Support `<!-- eslint-disable -->` comment directives in templates.",
);

impl Rule for CommentDirective {}

impl VueTemplateRule for CommentDirective {
    fn run_on_template<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut VueTemplateContext<'a>) {
        // Intentionally empty: directive semantics live in the template pass
        // (`vue_template.rs` + the shared `TemplateCommentDirectives`
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
        // The rule itself never reports; the directive behaviour it stands in
        // for is exercised end-to-end by the other vue rules' pass cases.
        let pass = vec![
            (
                "<template><div>{{ content }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                "<template>\n  <!-- eslint-disable-next-line vue/no-v-html -->\n  <div v-html=\"content\" />\n</template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];
        let fail = vec![];

        Tester::new(CommentDirective::NAME, CommentDirective::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
