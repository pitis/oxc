use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct NoTargetBlank;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow `target=\"_blank\"` without `rel=\"noopener noreferrer\"`.
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
    NoTargetBlank,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Disallow `target=\"_blank\"` without `rel=\"noopener noreferrer\"`.",
);

impl Rule for NoTargetBlank {}

impl SvelteTemplateRule for NoTargetBlank {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
