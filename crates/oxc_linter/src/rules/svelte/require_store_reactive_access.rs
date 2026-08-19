use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct RequireStoreReactiveAccess;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Require store values to be accessed reactively via `$`.
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
    RequireStoreReactiveAccess,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Require store values to be accessed reactively via `$`.",
);

impl Rule for RequireStoreReactiveAccess {}

impl SvelteTemplateRule for RequireStoreReactiveAccess {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
