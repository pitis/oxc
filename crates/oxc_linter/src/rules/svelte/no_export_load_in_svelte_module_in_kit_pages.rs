use oxc_macros::declare_oxc_lint;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

#[derive(Debug, Default, Clone)]
pub struct NoExportLoadInSvelteModuleInKitPages;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow exporting `load` from a module script in SvelteKit pages.
    ///
    /// ### Why is this bad?
    ///
    /// (Implementation pending — this stub registers the rule.)
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <!-- pending -->
    /// ```
    NoExportLoadInSvelteModuleInKitPages,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow exporting `load` from a module script in SvelteKit pages.",
);

impl Rule for NoExportLoadInSvelteModuleInKitPages {
    fn run<'a>(&self, _node: &AstNode<'a>, _ctx: &LintContext<'a>) {}

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}
