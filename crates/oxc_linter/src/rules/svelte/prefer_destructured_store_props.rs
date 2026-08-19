use oxc_allocator::Allocator;
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, BlockKind, ExpressionSlot, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{SVELTE_RUNES, parse_svelte_expression, walk_svelte_nodes},
};

fn prefer_destructured_store_props_diagnostic(
    store: &str,
    property: &str,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Destructure {property} from {store} for better change tracking & fewer redraws"
    ))
    .with_help(format!(
        "Add `$: ({{ {property} }} = {store});` to the script and use `{property}` here."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferDestructuredStoreProps;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers destructuring a store's property into a reactive variable over
    /// reading `$store.property` directly in the markup.
    ///
    /// ### Why is this bad?
    ///
    /// `{$store.name}` makes the whole expression depend on the store, so
    /// every store update re-runs it even when `name` did not change.
    /// `$: ({ name } = $store)` narrows the dependency to the one property.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <p>{$user.name}</p>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   $: ({ name } = $user);
    /// </script>
    ///
    /// <p>{name}</p>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Only a non-computed access (`$store.name`) is reported. Upstream also
    /// reports `$store[expr]` when it can prove every identifier in the
    /// expression is top-level; oxlint does not build scopes for markup
    /// expressions, so it leaves computed accesses alone rather than risk
    /// reporting one that reads a block-local binding.
    PreferDestructuredStoreProps,
    svelte,
    perf,
    version = "1.80.0",
    short_description = "Prefer destructuring store properties over `$store.prop`.",
);

impl Rule for PreferDestructuredStoreProps {}

impl SvelteTemplateRule for PreferDestructuredStoreProps {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let allocator = Allocator::new();
        // Only markup expressions: upstream skips anything inside `<script>`.
        let mut expressions: Vec<(&'a str, u32)> = Vec::new();
        collect_markup_expressions(nodes, &mut expressions);

        let mut reports: Vec<(String, String, Span)> = Vec::new();
        for (text, offset) in expressions {
            let Some(expression) = parse_svelte_expression(&allocator, text) else { continue };
            let mut visitor = StorePropVisitor { offset, found: Vec::new() };
            visitor.visit_expression(&expression);
            reports.extend(visitor.found);
        }
        for (store, property, span) in reports {
            ctx.diagnostic(prefer_destructured_store_props_diagnostic(&store, &property, span));
        }
    }
}

/// Finds `$store.property` accesses.
struct StorePropVisitor {
    offset: u32,
    found: Vec<(String, String, Span)>,
}

impl<'a> Visit<'a> for StorePropVisitor {
    fn visit_static_member_expression(
        &mut self,
        member: &oxc_ast::ast::StaticMemberExpression<'a>,
    ) {
        if let oxc_ast::ast::Expression::Identifier(object) = member.object.get_inner_expression() {
            let name = object.name.as_str();
            // `$store`, but not `$$props` and not a rune.
            if name.len() > 1
                && name.starts_with('$')
                && !name.starts_with("$$")
                && SVELTE_RUNES.binary_search(&name).is_err()
            {
                self.found.push((
                    name.to_string(),
                    member.property.name.to_string(),
                    Span::new(member.span.start + self.offset, member.span.end + self.offset),
                ));
            }
        }
        walk::walk_static_member_expression(self, member);
    }
}

/// Every markup expression, with its file offset. `<script>` bodies are not
/// included: this rule is about the template.
fn collect_markup_expressions<'a>(nodes: &[Node<'a>], out: &mut Vec<(&'a str, u32)>) {
    walk_svelte_nodes(nodes, &mut |node| match node {
        Node::Mustache(tag) => out.push((tag.expression, tag.expression_span.start)),
        Node::Tag(tag) => out.push((tag.expression, tag.expression_span.start)),
        Node::Element(element) => {
            for attribute in &element.attributes {
                let value = match &attribute.kind {
                    AttributeKind::Plain { value, .. } => value.as_ref(),
                    AttributeKind::Directive(directive) => directive.value.as_ref(),
                    _ => None,
                };
                let Some(value) = value else { continue };
                for part in &value.parts {
                    if let ValuePart::Expression(expression) = part {
                        out.push((expression.expression, expression.expression_span.start));
                    }
                }
            }
        }
        Node::Block(block) => {
            let mut push = |slot: &ExpressionSlot<'a>| out.push((slot.text, slot.span.start));
            match &block.kind {
                BlockKind::If(if_block) => {
                    for branch in &if_block.branches {
                        if let Some(expression) = &branch.expression {
                            push(expression);
                        }
                    }
                }
                BlockKind::Each(each) => push(&each.expression),
                BlockKind::Await(await_block) => push(&await_block.expression),
                BlockKind::Key(key) => push(&key.expression),
                // A snippet's header declares parameters rather than reading
                // a store.
                BlockKind::Snippet(_) => {}
                BlockKind::Unknown(unknown) => push(&unknown.header_rest),
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::PreferDestructuredStoreProps;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<script>\n\t$: ({ name } = $user);\n</script>\n<p>{name}</p>", None, None, path()),
            // The whole store, not a property of it.
            ("<p>{$user}</p>", None, None, path()),
            // Inside a `<script>`, which this rule does not check.
            ("<script>\n\tconst n = $user.name;\n</script>", None, None, path()),
            // Not a store: `$$props` and the runes.
            ("<p>{$$props.name}</p>", None, None, path()),
            ("<p>{$props.id}</p>", None, None, path()),
            // A computed access is left alone (documented deviation).
            ("<p>{$user[key]}</p>", None, None, path()),
            // An ordinary object.
            ("<p>{user.name}</p>", None, None, path()),
        ];
        let fail = vec![
            ("<p>{$user.name}</p>", None, None, path()),
            ("<div title={$user.name}></div>", None, None, path()),
            ("{#if $user.isAdmin}<p>admin</p>{/if}", None, None, path()),
            // Nested access reports the inner store read too.
            ("<p>{$user.profile.name}</p>", None, None, path()),
            ("{@html $page.html}", None, None, path()),
        ];

        Tester::new(
            PreferDestructuredStoreProps::NAME,
            PreferDestructuredStoreProps::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
