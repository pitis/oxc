use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct NoUnusedSvelteIgnore;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow unusable `svelte-ignore` comments.
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
    NoUnusedSvelteIgnore,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow unusable `svelte-ignore` comments.",
);

impl Rule for NoUnusedSvelteIgnore {}

impl SvelteTemplateRule for NoUnusedSvelteIgnore {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
