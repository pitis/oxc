use oxc_macros::declare_oxc_lint;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

#[derive(Debug, Default, Clone)]
pub struct PreferWritableDerived;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefer `$derived` over `$state` synchronized by `$effect`.
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
    PreferWritableDerived,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Prefer `$derived` over `$state` synchronized by `$effect`.",
);

impl Rule for PreferWritableDerived {
    fn run<'a>(&self, _node: &AstNode<'a>, _ctx: &LintContext<'a>) {}

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}
