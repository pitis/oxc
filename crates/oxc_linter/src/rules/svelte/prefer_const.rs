use oxc_allocator::Allocator;
use oxc_ast::{
    AstKind,
    ast::{Expression, VariableDeclarationKind},
};
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node, ValuePart};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, svelte_scripts, walk_svelte_elements, walk_svelte_nodes},
};

fn prefer_const_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{name}' is never reassigned. Use 'const' instead."))
        .with_help("Replace `let` with `const`.")
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct PreferConstConfig {
    /// Runes whose result must stay a `let`, because Svelte reassigns the
    /// binding itself.
    excluded_runes: Vec<String>,
}

impl Default for PreferConstConfig {
    fn default() -> Self {
        Self { excluded_runes: vec!["$props".to_string(), "$derived".to_string()] }
    }
}

// Boxed: the rune list would blow `RuleEnum`'s 16-byte budget unboxed.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct PreferConst(Box<PreferConstConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `const` for a `<script>` binding that is never reassigned —
    /// the Svelte-aware counterpart of `eslint/prefer-const`.
    ///
    /// ### Why is this bad?
    ///
    /// `const` states that the binding never changes, which is easier to read
    /// and lets the compiler and the reader rely on it.
    ///
    /// This rule exists because the core `eslint/prefer-const` cannot run on
    /// `.svelte` files: it only sees the `<script>`, so a binding the markup
    /// writes through `bind:this` or `bind:value` looks unassigned, and
    /// turning it into a `const` breaks the build. This rule reads the markup
    /// as well, so those bindings are left alone.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let greeting = 'hello';
    /// </script>
    ///
    /// <p>{greeting}</p>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   const greeting = 'hello';
    ///   let count = 0;
    ///   let element;
    /// </script>
    ///
    /// <button onclick={() => count++}>{count}</button>
    /// <div bind:this={element}></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `excludedRunes` (default `["$props", "$derived"]`): declarations
    /// initialised with one of these runes keep their `let`, because Svelte
    /// reassigns the binding behind the scenes.
    ///
    /// ```json
    /// {
    ///   "svelte/prefer-const": ["error", { "excludedRunes": ["$props"] }]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream wraps the core rule and inherits all of its options. This
    /// implementation covers the common case only: a `let` declarator with an
    /// initializer whose bindings are never written again, in the script or
    /// the markup. A `let x;` assigned exactly once later is not reported,
    /// and the core rule's `destructuring` and `ignoreReadBeforeAssign`
    /// options are not supported.
    PreferConst,
    svelte,
    style,
    config = PreferConst,
    version = "1.80.0",
    short_description = "Require `const` for bindings that are never reassigned.",
);

impl Rule for PreferConst {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for PreferConst {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let written_by_markup = markup_written_names(nodes);

        let mut reports: Vec<(String, Span)> = Vec::new();
        for script in svelte_scripts(nodes, source) {
            let allocator = Allocator::new();
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let parser_ret = Parser::new(&allocator, script.content, source_type).parse();
            if parser_ret.panicked {
                continue;
            }
            let program = allocator.alloc(parser_ret.program);
            let semantic = SemanticBuilder::new_linter().build(program).semantic;
            let scoping = semantic.scoping();

            for node in semantic.nodes() {
                let AstKind::VariableDeclaration(declaration) = node.kind() else { continue };
                if declaration.kind != VariableDeclarationKind::Let {
                    continue;
                }
                // A `let` in a `for…in` / `for…of` head is the loop's own
                // binding; the core rule handles those separately.
                if matches!(
                    semantic.nodes().parent_kind(node.id()),
                    AstKind::ForInStatement(_)
                        | AstKind::ForOfStatement(_)
                        | AstKind::ForStatement(_)
                ) {
                    continue;
                }
                for declarator in &declaration.declarations {
                    let Some(init) = &declarator.init else { continue };
                    if self.is_excluded_rune(init) {
                        continue;
                    }
                    for identifier in declarator.id.get_binding_identifiers() {
                        let name = identifier.name.as_str();
                        if written_by_markup.contains(name) {
                            continue;
                        }
                        let reassigned = scoping
                            .get_resolved_references(identifier.symbol_id())
                            .any(oxc_semantic::Reference::is_write);
                        if !reassigned {
                            reports.push((
                                name.to_string(),
                                Span::new(
                                    identifier.span.start + script.offset,
                                    identifier.span.end + script.offset,
                                ),
                            ));
                        }
                    }
                }
            }
        }
        for (name, span) in reports {
            ctx.diagnostic(prefer_const_diagnostic(&name, span));
        }
    }
}

impl PreferConst {
    /// Whether the initializer is one of the runes that keeps its `let`.
    fn is_excluded_rune(&self, init: &Expression<'_>) -> bool {
        let callee = match init.get_inner_expression() {
            Expression::CallExpression(call) => &call.callee,
            _ => return false,
        };
        let name = match callee.get_inner_expression() {
            Expression::Identifier(identifier) => identifier.name.as_str(),
            // `$derived.by(…)`, `$state.raw(…)`, …
            Expression::StaticMemberExpression(member) => {
                match member.object.get_inner_expression() {
                    Expression::Identifier(object) => object.name.as_str(),
                    _ => return false,
                }
            }
            _ => return false,
        };
        self.0.excluded_runes.iter().any(|rune| rune == name)
    }
}

/// Names the markup writes: `bind:` targets, and assignment targets inside
/// markup expressions such as inline event handlers.
fn markup_written_names<'a>(nodes: &[Node<'a>]) -> FxHashSet<&'a str> {
    let mut names = FxHashSet::default();

    walk_svelte_elements(nodes, &mut |element| {
        for attribute in &element.attributes {
            let AttributeKind::Directive(directive) = &attribute.kind else { continue };
            if directive.kind != DirectiveKind::Bind {
                continue;
            }
            match &directive.value {
                // `bind:value={name}` — the expression is the target.
                Some(value) => {
                    for part in &value.parts {
                        if let ValuePart::Expression(expression) = part {
                            names.extend(leading_identifier(expression.expression));
                        }
                    }
                }
                // `bind:value` — shorthand for `bind:value={value}`.
                None => {
                    names.insert(directive.name);
                }
            }
        }
    });

    // Assignments written inside any markup expression.
    let allocator = Allocator::new();
    let mut expressions: Vec<&'a str> = Vec::new();
    collect_expressions(nodes, &mut expressions);
    for text in expressions {
        let Some(expression) = parse_svelte_expression(&allocator, text) else { continue };
        let mut visitor = AssignmentTargetVisitor { names: Vec::new() };
        visitor.visit_expression(&expression);
        // The visitor borrows the arena copy, so match the names back to the
        // original text to keep the `'a` lifetime.
        for name in visitor.names {
            if let Some(found) = text.match_indices(name.as_str()).next() {
                names.insert(&text[found.0..found.0 + name.len()]);
            }
        }
    }

    names
}

/// The identifier a `bind:` expression ultimately writes: `name`, `name.prop`
/// and `name[i]` all write through `name`.
fn leading_identifier(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    let end = trimmed
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(trimmed.len());
    (end > 0).then(|| &trimmed[..end])
}

/// Collects every markup expression's text.
fn collect_expressions<'a>(nodes: &[Node<'a>], out: &mut Vec<&'a str>) {
    walk_svelte_nodes(nodes, &mut |node| match node {
        Node::Mustache(tag) => out.push(tag.expression),
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
                        out.push(expression.expression);
                    }
                }
            }
        }
        _ => {}
    });
}

/// Collects the names an expression assigns to or updates.
struct AssignmentTargetVisitor {
    names: Vec<String>,
}

impl<'a> Visit<'a> for AssignmentTargetVisitor {
    fn visit_assignment_expression(&mut self, assignment: &oxc_ast::ast::AssignmentExpression<'a>) {
        if let Some(name) = assignment.left.get_identifier_name() {
            self.names.push(name.to_string());
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_update_expression(&mut self, update: &oxc_ast::ast::UpdateExpression<'a>) {
        if let Some(name) = update.argument.get_identifier_name() {
            self.names.push(name.to_string());
        }
        walk::walk_update_expression(self, update);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::PreferConst;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            (
                "<script>\n\tconst greeting = 'hello';\n</script>\n<p>{greeting}</p>",
                None,
                None,
                path(),
            ),
            // Reassigned in the script.
            (
                "<script>\n\tlet count = 0;\n\tfunction bump() { count += 1; }\n</script>",
                None,
                None,
                path(),
            ),
            // Written by the markup.
            (
                "<script>\n\tlet element = null;\n</script>\n<div bind:this={element}></div>",
                None,
                None,
                path(),
            ),
            ("<script>\n\tlet value = '';\n</script>\n<input bind:value />", None, None, path()),
            (
                "<script>\n\tlet count = 0;\n</script>\n<button on:click={() => count++}>x</button>",
                None,
                None,
                path(),
            ),
            // Excluded runes keep their `let`.
            ("<script>\n\tlet { a } = $props();\n</script>", None, None, path()),
            ("<script>\n\tlet doubled = $derived(count * 2);\n</script>", None, None, path()),
            ("<script>\n\tlet total = $derived.by(() => 1);\n</script>", None, None, path()),
            // No initializer: not reported (documented deviation).
            ("<script>\n\tlet later;\n</script>", None, None, path()),
        ];
        let fail = vec![
            (
                "<script>\n\tlet greeting = 'hello';\n</script>\n<p>{greeting}</p>",
                None,
                None,
                path(),
            ),
            // A rune that is not excluded.
            ("<script>\n\tlet count = $state(0);\n</script>", None, None, path()),
            // Nested scopes are checked too.
            (
                "<script>\n\tfunction f() {\n\t\tlet local = 1;\n\t\treturn local;\n\t}\n</script>",
                None,
                None,
                path(),
            ),
            // Destructuring: every never-written binding is reported.
            ("<script>\n\tlet { a, b } = obj;\n</script>\n<p>{a}{b}</p>", None, None, path()),
            // `$derived` reported once it is taken off the exclusion list.
            (
                "<script>\n\tlet doubled = $derived(count * 2);\n</script>",
                Some(serde_json::json!([{ "excludedRunes": ["$props"] }])),
                None,
                path(),
            ),
        ];

        Tester::new(PreferConst::NAME, PreferConst::PLUGIN, pass, fail).test_and_snapshot();
    }
}
