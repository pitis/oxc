use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct NoSpacesAroundEqualSignsInAttribute;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow spaces around equal signs in attributes.
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
    NoSpacesAroundEqualSignsInAttribute,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Disallow spaces around equal signs in attributes.",
);

impl Rule for NoSpacesAroundEqualSignsInAttribute {}

impl SvelteTemplateRule for NoSpacesAroundEqualSignsInAttribute {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
