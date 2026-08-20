use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentOperator, AssignmentTarget, BinaryOperator, BindingPattern, Expression,
    ImportDeclarationSpecifier, SimpleAssignmentTarget, Statement, UnaryOperator,
    VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use svelte_markup_parser::ast::{
    AttributeKind, AttributeValue, BlockKind, DirectiveKind, ExpressionTag, Node, TagKind,
    ValuePart,
};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_scripts, walk_svelte_nodes},
};

fn require_store_reactive_access_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Use the $ prefix or the get function to access reactive values instead of accessing the raw store.",
    )
    .with_help("Prefix the store with `$` (e.g. `{$store}`) to read its reactive value; the bare variable is the store object itself.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireStoreReactiveAccess;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows using a Svelte store object itself as an operand in the
    /// template where its *value* is meant — `{store}` instead of
    /// `{$store}`, `use:store`, `{#if store}`, and so on.
    ///
    /// ### Why is this bad?
    ///
    /// A store variable holds the store handle (an object with
    /// `subscribe`), not its current value. Rendering or testing the handle
    /// is almost always a bug: `{store}` prints `[object Object]` and
    /// `{#if store}` is always truthy. The `$` prefix subscribes and reads
    /// the value.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { writable } from 'svelte/store';
    ///   const count = writable(0);
    /// </script>
    ///
    /// <p>{count}</p>
    /// {#if count}...{/if}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { writable } from 'svelte/store';
    ///   const count = writable(0);
    /// </script>
    ///
    /// <p>{$count}</p>
    /// {#if $count}...{/if}
    /// ```
    RequireStoreReactiveAccess,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Require store values to be accessed reactively via `$`.",
);

impl Rule for RequireStoreReactiveAccess {}

// Ports eslint-plugin-svelte's `require-store-reactive-access` — the
// template side of it.
//
// Which variables are stores: upstream has two checkers — a TypeScript one
// (any expression whose type has a subscribe(fn, fn) member) and an
// ES fallback that tracks variables initialized from `writable` /
// `readable` / `derived` calls imported from 'svelte/store'. This port
// implements the ES fallback: named `svelte/store` imports of those three
// factories, and *top-level* `let`/`const`/`var` declarators of the form
// `id = factory(...)` in any `<script>` block. No type information, no
// namespace imports (`import * as store`), no stores imported from other
// modules, no aliasing through reassignment — all documented gaps.
//
// Where it checks: markup expressions only. Each expression slice is parsed
// with `oxc_parser` and checked in two layers, mirroring upstream:
// - a top-level check where the surrounding markup construct itself is an
//   operand position (text mustaches, HTML-element attribute values,
//   `{...spread}`, directive values/names, `{#if}`/`{#each}`/`{#await}`
//   headers) — component props (`<Foo prop={store} />`) legitimately
//   receive the handle and are exempt, as upstream's
//   `canAcceptStoreMustache`/`canAcceptStoreAttributeElement` decide;
// - an operand walk inside the expression covering upstream's visitor set
//   (call/new callees, unary/update/spread arguments, binary/logical
//   operands, template-literal interpolations, computed keys, `await`,
//   compound assignments, statement tests inside inline functions, …).
//   Contexts upstream marks `consistent: true` (`{#if}`, `!x`, `==`, `&&`,
//   `await`, conditionals, …) report only `const`-declared stores, exactly
//   like upstream's ES checker.
//
// Documented deviations:
// - Script-side usage (`if (store)` inside `<script>` etc.) is NOT checked
//   here: those same violations are only soundly attributable with the
//   type-aware checker (and the script pass is the natural home for it).
// - No autofix (upstream inserts `$`).
// - Store names are matched textually against top-level script bindings;
//   an expression-local shadow of a store name inside a markup slice
//   (e.g. an arrow parameter) would still be reported.
impl SvelteTemplateRule for RequireStoreReactiveAccess {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        // name → declared with `const` (upstream's `consistent` contexts
        // only report const-declared stores).
        let mut stores: FxHashMap<String, bool> = FxHashMap::default();
        for script in svelte_scripts(nodes, ctx.source_text()) {
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let allocator = Allocator::new();
            let parser_ret = Parser::new(&allocator, script.content, source_type).parse();
            if parser_ret.panicked {
                continue;
            }
            let program = &parser_ret.program;

            // Local names of `writable`/`readable`/`derived` from
            // 'svelte/store' (named imports only).
            let mut factories: FxHashSet<String> = FxHashSet::default();
            for statement in &program.body {
                let Statement::ImportDeclaration(import) = statement else { continue };
                if import.source.value != "svelte/store" {
                    continue;
                }
                let Some(specifiers) = &import.specifiers else { continue };
                for specifier in specifiers {
                    let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                        continue;
                    };
                    let imported = specifier.imported.name();
                    if matches!(imported.as_str(), "writable" | "readable" | "derived") {
                        factories.insert(specifier.local.name.to_string());
                    }
                }
            }
            if factories.is_empty() {
                continue;
            }
            for statement in &program.body {
                let Statement::VariableDeclaration(declaration) = statement else { continue };
                let is_const = declaration.kind == VariableDeclarationKind::Const;
                for declarator in &declaration.declarations {
                    let BindingPattern::BindingIdentifier(id) = &declarator.id else { continue };
                    let Some(Expression::CallExpression(call)) = &declarator.init else {
                        continue;
                    };
                    let Expression::Identifier(callee) = &call.callee else { continue };
                    if factories.contains(callee.name.as_str()) {
                        stores.insert(id.name.to_string(), is_const);
                    }
                }
            }
        }
        if stores.is_empty() {
            return;
        }

        let mut checker = Checker { stores: &stores, spans: Vec::new() };
        walk_svelte_nodes(nodes, &mut |node| checker.check_node(node));

        let mut spans = checker.spans;
        spans.sort_unstable_by_key(|span| span.start);
        spans.dedup();
        for span in spans {
            ctx.diagnostic(require_store_reactive_access_diagnostic(span));
        }
    }
}

struct Checker<'s> {
    stores: &'s FxHashMap<String, bool>,
    /// File-absolute report spans.
    spans: Vec<Span>,
}

impl Checker<'_> {
    fn check_node(&mut self, node: &Node<'_>) {
        match node {
            // Text interpolation: `<p>{store}</p>`.
            Node::Mustache(tag) => self.check_tag(tag, Some(false)),
            Node::Tag(tag) => {
                // `{@html store}` is a (raw) mustache upstream; the other
                // `{@…}` tags only get the operand walk (e.g. the callee of
                // `{@render store()}`).
                let (text, span) = svelte_markup_parser::ast::ExpressionSlot {
                    span: tag.expression_span,
                    text: tag.expression,
                }
                .trimmed();
                self.check_slice(
                    text,
                    span.start,
                    if tag.kind == TagKind::Html { Some(false) } else { None },
                );
            }
            Node::Element(element) => {
                // Upstream's `canAcceptStoreAttributeElement`: components
                // and `svelte:*` specials other than `svelte:element` may
                // receive a store as a prop.
                let accepts_store = element.is_component_like()
                    || element.svelte_name().is_some_and(|name| name != "element");
                for attribute in &element.attributes {
                    match &attribute.kind {
                        AttributeKind::Plain { name, value: Some(value), .. } => {
                            if let Some(tag) = value.as_single_expression() {
                                // Single-mustache value: props position on
                                // components / `--style-props`; an operand
                                // on plain HTML elements.
                                let top = if accepts_store || name.starts_with("--") {
                                    None
                                } else {
                                    Some(false)
                                };
                                self.check_tag(tag, top);
                            } else {
                                // `attr="a {store} b"`: text interpolation
                                // on any element.
                                self.check_value_parts(value, Some(false));
                            }
                        }
                        AttributeKind::Plain { value: None, .. }
                        | AttributeKind::Comment { .. } => {}
                        AttributeKind::Shorthand { name, name_span } => {
                            if !accepts_store {
                                self.check_name(name, *name_span, false);
                            }
                        }
                        AttributeKind::Spread { expression, expression_span } => {
                            let (text, span) = svelte_markup_parser::ast::ExpressionSlot {
                                span: *expression_span,
                                text: expression,
                            }
                            .trimmed();
                            self.check_slice(text, span.start, Some(false));
                        }
                        AttributeKind::Directive(directive) => match directive.kind {
                            // `use:store`, `transition:store`, `in:`/`out:`,
                            // `animate:store` — the *name* references the
                            // function.
                            DirectiveKind::Use
                            | DirectiveKind::Transition
                            | DirectiveKind::In
                            | DirectiveKind::Out
                            | DirectiveKind::Animate => {
                                self.check_name(directive.name, directive.name_span, false);
                                if let Some(value) = &directive.value {
                                    self.check_value_parts(value, None);
                                }
                            }
                            // `on:click={store}`.
                            DirectiveKind::On => {
                                if let Some(value) = &directive.value {
                                    self.check_value_parts(value, Some(false));
                                }
                            }
                            DirectiveKind::Bind => {
                                if directive.name == "this" {
                                    // `bind:this={store}` is checked on any
                                    // element.
                                    if let Some(value) = &directive.value {
                                        self.check_value_parts(value, Some(false));
                                    }
                                } else if let Some(value) = &directive.value {
                                    let top = if accepts_store { None } else { Some(false) };
                                    self.check_value_parts(value, top);
                                } else if !accepts_store {
                                    // `bind:value` shorthand on an HTML
                                    // element is `bind:value={value}`.
                                    self.check_name(directive.name, directive.name_span, false);
                                }
                            }
                            // `class:x` is a `consistent` context upstream:
                            // only const-declared stores report.
                            DirectiveKind::Class => {
                                if let Some(value) = &directive.value {
                                    self.check_value_parts(value, Some(true));
                                } else {
                                    self.check_name(directive.name, directive.name_span, true);
                                }
                            }
                            // `style:color={store}` values are mustaches
                            // upstream (never props); the shorthand
                            // references the variable.
                            DirectiveKind::Style => {
                                if let Some(value) = &directive.value {
                                    self.check_value_parts(value, Some(false));
                                } else {
                                    self.check_name(directive.name, directive.name_span, false);
                                }
                            }
                            DirectiveKind::Let => {}
                        },
                    }
                }
            }
            Node::Block(block) => match &block.kind {
                BlockKind::If(if_block) => {
                    for branch in &if_block.branches {
                        if let Some(expression) = &branch.expression {
                            let (text, span) = expression.trimmed();
                            self.check_slice(text, span.start, Some(true));
                        }
                    }
                }
                BlockKind::Each(each) => {
                    let (text, span) = each.expression.trimmed();
                    self.check_slice(text, span.start, Some(false));
                    if let Some(key) = &each.key {
                        let (text, span) = key.trimmed();
                        self.check_slice(text, span.start, None);
                    }
                }
                BlockKind::Await(await_block) => {
                    let (text, span) = await_block.expression.trimmed();
                    self.check_slice(text, span.start, Some(true));
                }
                BlockKind::Key(key) => {
                    // Upstream has no `SvelteKeyBlock` handler: operand walk
                    // only.
                    let (text, span) = key.expression.trimmed();
                    self.check_slice(text, span.start, None);
                }
                BlockKind::Snippet(_) => {}
                BlockKind::Unknown(unknown) => {
                    let (text, span) = unknown.header_rest.trimmed();
                    self.check_slice(text, span.start, None);
                }
            },
            _ => {}
        }
    }

    /// Check every `{expr}` part of an attribute value. `top` is the
    /// top-level operand context (None = walk only) applied to each part.
    fn check_value_parts(&mut self, value: &AttributeValue<'_>, top: Option<bool>) {
        for part in &value.parts {
            if let ValuePart::Expression(tag) = part {
                self.check_tag(tag, top);
            }
        }
    }

    fn check_tag(&mut self, tag: &ExpressionTag<'_>, top: Option<bool>) {
        let (text, span) = tag.trimmed();
        self.check_slice(text, span.start, top);
    }

    /// Report a written name (shorthand attribute, directive name) that is
    /// a store.
    fn check_name(&mut self, name: &str, span: Span, consistent: bool) {
        if !is_simple_identifier(name) {
            return;
        }
        if self.is_reportable_store(name, consistent) {
            self.spans.push(span);
        }
    }

    fn is_reportable_store(&self, name: &str, consistent: bool) -> bool {
        if name.starts_with('$') {
            return false;
        }
        match self.stores.get(name) {
            Some(&is_const) => !consistent || is_const,
            None => false,
        }
    }

    /// Parse one expression slice and check it. `top`: `Some(consistent)`
    /// when the slice as a whole sits in an operand position; `None` for
    /// the operand walk only. `base` is the file offset of `text`.
    fn check_slice(&mut self, text: &str, base: u32, top: Option<bool>) {
        if text.is_empty() {
            return;
        }
        let allocator = Allocator::new();
        let Ok(expression) = Parser::new(&allocator, text, SourceType::ts())
            .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
            .parse_expression()
        else {
            return;
        };

        let mut walker = Walker { stores: self.stores, base, spans: &mut self.spans };
        if let Some(consistent) = top {
            // Svelte 5 function bindings (`bind:value={get, set}`) parse as
            // a sequence; upstream checks each sub-expression.
            if let Expression::SequenceExpression(sequence) = &expression {
                for expression in &sequence.expressions {
                    walker.check(expression, consistent);
                }
            } else {
                walker.check(&expression, consistent);
            }
        }
        walker.visit_expression(&expression);
    }
}

struct Walker<'w> {
    stores: &'w FxHashMap<String, bool>,
    /// File offset of the parsed slice.
    base: u32,
    spans: &'w mut Vec<Span>,
}

impl Walker<'_> {
    /// Upstream's `verifyExpression`: report when `expression` is a bare
    /// identifier naming a store (skipping `$…`); `consistent` contexts
    /// report only const-declared stores (upstream's ES checker).
    fn check(&mut self, expression: &Expression<'_>, consistent: bool) {
        let Expression::Identifier(identifier) = expression else { return };
        let name = identifier.name.as_str();
        if name.starts_with('$') {
            return;
        }
        if let Some(&is_const) = self.stores.get(name)
            && (!consistent || is_const)
        {
            self.spans.push(Span::new(
                identifier.span.start + self.base,
                identifier.span.end + self.base,
            ));
        }
    }

    fn check_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'_>) {
        let name = identifier.name.as_str();
        if name.starts_with('$') {
            return;
        }
        if self.stores.contains_key(name) {
            self.spans.push(Span::new(
                identifier.span.start + self.base,
                identifier.span.end + self.base,
            ));
        }
    }
}

/// The operand-position walk, mirroring upstream's script-expression
/// visitors (which also fire on template expressions there).
impl<'a> Visit<'a> for Walker<'_> {
    fn visit_conditional_expression(&mut self, it: &oxc_ast::ast::ConditionalExpression<'a>) {
        self.check(&it.test, true);
        walk::walk_conditional_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
        self.check(&it.callee, false);
        walk::walk_call_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &oxc_ast::ast::NewExpression<'a>) {
        self.check(&it.callee, false);
        walk::walk_new_expression(self, it);
    }

    fn visit_unary_expression(&mut self, it: &oxc_ast::ast::UnaryExpression<'a>) {
        self.check(
            &it.argument,
            matches!(it.operator, UnaryOperator::LogicalNot | UnaryOperator::Typeof),
        );
        walk::walk_unary_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = &it.argument {
            self.check_identifier_reference(identifier);
        }
        walk::walk_update_expression(self, it);
    }

    fn visit_spread_element(&mut self, it: &oxc_ast::ast::SpreadElement<'a>) {
        self.check(&it.argument, false);
        walk::walk_spread_element(self, it);
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        if it.operator != AssignmentOperator::Assign {
            if let AssignmentTarget::AssignmentTargetIdentifier(identifier) = &it.left {
                self.check_identifier_reference(identifier);
            }
            self.check(&it.right, false);
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_binary_expression(&mut self, it: &oxc_ast::ast::BinaryExpression<'a>) {
        let consistent = matches!(
            it.operator,
            BinaryOperator::Equality
                | BinaryOperator::Inequality
                | BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
        );
        self.check(&it.left, consistent);
        self.check(&it.right, consistent);
        walk::walk_binary_expression(self, it);
    }

    fn visit_logical_expression(&mut self, it: &oxc_ast::ast::LogicalExpression<'a>) {
        self.check(&it.left, true);
        walk::walk_logical_expression(self, it);
    }

    fn visit_template_literal(&mut self, it: &oxc_ast::ast::TemplateLiteral<'a>) {
        for expression in &it.expressions {
            self.check(expression, false);
        }
        walk::walk_template_literal(self, it);
    }

    fn visit_tagged_template_expression(
        &mut self,
        it: &oxc_ast::ast::TaggedTemplateExpression<'a>,
    ) {
        self.check(&it.tag, false);
        walk::walk_tagged_template_expression(self, it);
    }

    fn visit_object_property(&mut self, it: &oxc_ast::ast::ObjectProperty<'a>) {
        if it.computed
            && let Some(key) = it.key.as_expression()
        {
            self.check(key, false);
        }
        walk::walk_object_property(self, it);
    }

    fn visit_property_definition(&mut self, it: &oxc_ast::ast::PropertyDefinition<'a>) {
        if it.computed
            && let Some(key) = it.key.as_expression()
        {
            self.check(key, false);
        }
        walk::walk_property_definition(self, it);
    }

    fn visit_method_definition(&mut self, it: &oxc_ast::ast::MethodDefinition<'a>) {
        if it.computed
            && let Some(key) = it.key.as_expression()
        {
            self.check(key, false);
        }
        walk::walk_method_definition(self, it);
    }

    fn visit_import_expression(&mut self, it: &oxc_ast::ast::ImportExpression<'a>) {
        self.check(&it.source, false);
        walk::walk_import_expression(self, it);
    }

    fn visit_await_expression(&mut self, it: &oxc_ast::ast::AwaitExpression<'a>) {
        self.check(&it.argument, true);
        walk::walk_await_expression(self, it);
    }

    // Statement forms can appear inside inline handlers' function bodies.
    fn visit_if_statement(&mut self, it: &oxc_ast::ast::IfStatement<'a>) {
        self.check(&it.test, true);
        walk::walk_if_statement(self, it);
    }

    fn visit_while_statement(&mut self, it: &oxc_ast::ast::WhileStatement<'a>) {
        self.check(&it.test, true);
        walk::walk_while_statement(self, it);
    }

    fn visit_do_while_statement(&mut self, it: &oxc_ast::ast::DoWhileStatement<'a>) {
        self.check(&it.test, true);
        walk::walk_do_while_statement(self, it);
    }

    fn visit_for_statement(&mut self, it: &oxc_ast::ast::ForStatement<'a>) {
        if let Some(test) = &it.test {
            self.check(test, true);
        }
        walk::walk_for_statement(self, it);
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'a>) {
        self.check(&it.right, false);
        walk::walk_for_in_statement(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'a>) {
        self.check(&it.right, false);
        walk::walk_for_of_statement(self, it);
    }

    fn visit_switch_statement(&mut self, it: &oxc_ast::ast::SwitchStatement<'a>) {
        self.check(&it.discriminant, false);
        walk::walk_switch_statement(self, it);
    }
}

/// A plain ASCII identifier (no dots, no modifiers).
fn is_simple_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireStoreReactiveAccess;
    use crate::{rule::RuleMeta, tester::Tester};

    const SCRIPT: &str = "<script>
	import { writable } from 'svelte/store';
	let store = writable('hello');
	const constStore = writable('hello');
</script>
";

    fn with_script(markup: &str) -> String {
        format!("{SCRIPT}\n{markup}")
    }

    #[test]
    fn test() {
        let pass_sources = vec![
            // `$`-prefixed accesses (upstream valid/attrs-store01,
            // valid/directives-store01).
            with_script("<div prop=\"Hello {$store}\" />"),
            with_script("<div prop={$store} />"),
            with_script("<div {...$store} />"),
            with_script("<div bind:this={$store} />"),
            with_script("<p>{$store}</p>"),
            with_script("<div use:$store />"),
            with_script("{#if $constStore}x{/if}"),
            // Component props legitimately receive the handle (upstream
            // valid/props-store01).
            with_script("<MyComponent prop={store} />"),
            with_script("<MyComponent {store} />"),
            with_script("<MyComponent bind:value={store} />"),
            with_script("<MyComponent bind:store />"),
            with_script("<MyComponent --my-style-var={store} />"),
            // `class:` is a `consistent` context: only const stores report
            // (upstream valid/directives-store01 tail).
            with_script("<div class:name={store} />"),
            with_script("<div class:store />"),
            // `{#if}` is consistent too: `let`-declared store passes.
            with_script("{#if store}x{/if}"),
            // Property/method access on the handle is legitimate
            // (`store.set(1)` in a handler; upstream valid/properties01).
            with_script("<button on:click={() => store.set(1)}>x</button>"),
            with_script("{store.name}"),
            // Call arguments are fine (`get(store)`).
            with_script("{get(store)}"),
            // `{@const}` assigns the handle — fine (upstream has no
            // VariableDeclarator check).
            with_script("{#each [1] as x}{@const s = store}{x}{/each}"),
            // Only svelte/store-created variables are tracked: unknown
            // variables never report.
            with_script("<p>{other}</p>"),
            // SUBSET BOUNDARY (documented): stores imported from another
            // module are not tracked without type info.
            "<script>
	import { count } from './stores.js';
</script>

<p>{count}</p>"
                .to_string(),
            // SUBSET BOUNDARY (documented): script-side raw-store usage is
            // not checked by this markup pass.
            "<script>
	import { writable } from 'svelte/store';
	const flag = writable(false);
	if (flag) { console.log('always true'); }
</script>

<p>{$flag}</p>"
                .to_string(),
        ];
        let fail_sources = vec![
            // Text interpolations & HTML-element attributes (upstream
            // invalid/attrs-store01).
            with_script("<div prop=\"Hello {store}\" />"),
            with_script("<div prop={store} />"),
            with_script("<div {store} />"),
            with_script("<div {...store} />"),
            with_script("<div bind:this={store} />"),
            with_script("<p>{store}</p>"),
            // Multi-part values interpolate as text even on components.
            with_script("<MyComponent message=\"Hello {store}\" />"),
            // Directives (upstream invalid/directives-store01).
            with_script("<button on:click={store} />"),
            with_script("<div style:color={store} />"),
            with_script("<div use:store />"),
            with_script("<div transition:store />"),
            with_script("<div in:store />"),
            with_script("<div out:store />"),
            with_script("{#each [] as e (e)}<div animate:store />{/each}"),
            // `class:` reports const-declared stores.
            with_script("<div class:name={constStore} />"),
            with_script("<div class:constStore />"),
            // Consistent block contexts report const stores.
            with_script("{#if constStore}x{/if}"),
            with_script("{#await constStore}x{/await}"),
            // `{#each store}` is not a consistent context.
            with_script("{#each store as item}{item}{/each}"),
            // Operand walk inside larger expressions.
            with_script("<p>{store + 1}</p>"),
            with_script("<p>{`${store}`}</p>"),
            with_script("<p>{typeof constStore}</p>"),
            with_script("<button on:click={() => store()}>x</button>"),
            with_script("{@html store}"),
        ];

        let pass = pass_sources
            .iter()
            .map(|source| (source.as_str(), None, None, Some(PathBuf::from("test.svelte"))))
            .collect::<Vec<_>>();
        let fail = fail_sources
            .iter()
            .map(|source| (source.as_str(), None, None, Some(PathBuf::from("test.svelte"))))
            .collect::<Vec<_>>();

        Tester::new(
            RequireStoreReactiveAccess::NAME,
            RequireStoreReactiveAccess::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
