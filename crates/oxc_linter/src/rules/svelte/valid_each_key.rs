use oxc_macros::declare_oxc_lint;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
};

#[derive(Debug, Default, Clone)]
pub struct ValidEachKey;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforce keys in `{#each}` blocks to use the block's own variables.
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
    ValidEachKey,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Enforce keys in `{#each}` blocks to use the block's own variables.",
);

impl Rule for ValidEachKey {}

impl SvelteTemplateRule for ValidEachKey {
    fn run_on_markup<'a>(&self, _nodes: &[Node<'a>], _ctx: &mut SvelteTemplateContext<'a>) {}
}
