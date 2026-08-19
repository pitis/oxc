use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct NoDomManipulating;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow direct DOM manipulation of `bind:this` elements.
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
    NoDomManipulating,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow direct DOM manipulation of `bind:this` elements.",
);

impl Rule for NoDomManipulating {}

impl SvelteTemplateRule for NoDomManipulating {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
