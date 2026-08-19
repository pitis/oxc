use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct NoUselessMustaches;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow unnecessary mustache interpolations.
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
    NoUselessMustaches,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Disallow unnecessary mustache interpolations.",
);

impl Rule for NoUselessMustaches {}

impl SvelteTemplateRule for NoUselessMustaches {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
