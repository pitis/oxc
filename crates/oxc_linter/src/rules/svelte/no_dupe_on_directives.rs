use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct NoDupeOnDirectives;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow duplicate `on:` directives.
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
    NoDupeOnDirectives,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow duplicate `on:` directives.",
);

impl Rule for NoDupeOnDirectives {}

impl SvelteTemplateRule for NoDupeOnDirectives {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
