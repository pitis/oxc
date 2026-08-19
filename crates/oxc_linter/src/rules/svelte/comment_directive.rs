use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct CommentDirective;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Support `<!-- eslint-disable -->` comment directives in markup.
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
    CommentDirective,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Support `<!-- eslint-disable -->` comment directives in markup.",
);

impl Rule for CommentDirective {}

impl SvelteTemplateRule for CommentDirective {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
