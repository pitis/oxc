use oxc_macros::declare_oxc_lint;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

#[derive(Debug, Default, Clone)]
pub struct NoReactiveReassign;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow reassigning reactive values.
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
    NoReactiveReassign,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow reassigning reactive values.",
);

impl Rule for NoReactiveReassign {
    fn run<'a>(&self, _node: &AstNode<'a>, _ctx: &LintContext<'a>) {}

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}
