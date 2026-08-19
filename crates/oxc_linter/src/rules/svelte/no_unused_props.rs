use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, PropertyKey, Statement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashSet;
use svelte_markup_parser::ast::{AttributeKind, BlockKind, DirectiveKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_scripts, walk_svelte_nodes},
};

fn no_unused_props_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{name}' is an unused Props property."))
        .with_help("Remove the property from the `$props()` destructuring, or use it.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoUnusedProps;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports properties destructured from `$props()` that are never used
    /// in the component — neither in the `<script>` block nor anywhere in
    /// the markup.
    ///
    /// Note: this is a static approximation of eslint-plugin-svelte's
    /// type-aware rule. Upstream uses the TypeScript type checker and
    /// primarily reports *type* properties that are never destructured or
    /// accessed; without type information this port instead reports
    /// destructured bindings that are never used. Props declared only in a
    /// type/interface and never destructured are NOT detected.
    ///
    /// ### Why is this bad?
    ///
    /// A destructured prop that nothing reads is dead API surface: callers
    /// keep passing data the component silently drops, and readers assume
    /// the prop does something.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let { name, age } = $props();
    ///   console.log(name);
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let { name, age } = $props();
    ///   console.log(name);
    /// </script>
    ///
    /// <p>{age}</p>
    /// ```
    NoUnusedProps,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow unused `$props()` properties.",
);

impl Rule for NoUnusedProps {}

// Ports eslint-plugin-svelte's `no-unused-props`, heavily approximated.
//
// Upstream requires the TypeScript language service: it resolves the type
// annotated on the `$props()` declaration and reports type properties that
// are never destructured nor reached through a property path (plus unused
// index signatures, nested unused properties, `checkImportedTypes`, …).
// None of that type machinery exists here.
//
// What this port does instead: it finds the top-level
// `let { … } = $props()` destructuring in each `<script>` block and
// reports destructured props whose local binding is used nowhere — checked
// two ways:
// - script side: `oxc_semantic` scope analysis of the script (accurate,
//   shadowing-aware);
// - markup side: a lexical identifier scan over every expression slice in
//   the markup (mustaches, `{@…}` tags, attribute/directive expression
//   parts, shorthand and directive names, and block headers). Any textual
//   identifier occurrence counts as a use.
//
// Documented deviations:
// - Props declared only in a TS type and never destructured are NOT
//   reported (upstream's main case; needs the type checker). Options
//   (`ignoreTypePatterns`, `ignorePropertyPatterns`,
//   `allowUnusedNestedProperties`, `checkImportedTypes`) are not
//   implemented; nested property paths and index signatures are not
//   analyzed.
// - The markup check is purely lexical and deliberately over-counts
//   (shadowed names, object keys, or words inside string literals in a
//   markup expression count as uses), preferring false negatives over
//   false positives.
// - A rest element (`...rest`) in the pattern disables the check for that
//   declaration, like upstream.
// - Props destructured into nested patterns (`let { a: { b } } =
//   $props()`) are treated as used, mirroring upstream (which recurses
//   into the nested *type* instead).
// - Each diagnostic points at the property inside the pattern; upstream
//   labels the whole pattern.
// - `let props = $props()` (no destructuring) is not checked (upstream
//   analyzes property paths through the type; without types every
//   `props.x` access is opaque).
/// One prop destructured out of `$props()`.
struct Prop<'p> {
    /// The prop's name as the parent passes it (the pattern key).
    name: String,
    /// The local binding it lands in (differs when renamed).
    local_name: &'p str,
    /// The property inside the pattern, script-relative.
    span: Span,
}

impl SvelteTemplateRule for NoUnusedProps {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let scripts = svelte_scripts(nodes, ctx.source_text());
        if scripts.is_empty() {
            return;
        }

        // Every identifier appearing in any markup expression slice.
        let mut markup_identifiers: FxHashSet<&str> = FxHashSet::default();
        collect_markup_identifiers(nodes, &mut markup_identifiers);

        let mut reports: Vec<(String, Span)> = Vec::new();
        for script in &scripts {
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let allocator = Allocator::new();
            let parser_ret = Parser::new(&allocator, script.content, source_type).parse();
            if parser_ret.panicked {
                continue;
            }
            let program = allocator.alloc(parser_ret.program);

            // Collect the `$props()` destructured props first, so semantic
            // analysis only runs when there is something to check.
            let mut props: Vec<Prop<'_>> = Vec::new();
            for statement in &program.body {
                let Statement::VariableDeclaration(declaration) = statement else { continue };
                for declarator in &declaration.declarations {
                    let Some(Expression::CallExpression(call)) = &declarator.init else {
                        continue;
                    };
                    let Expression::Identifier(callee) = &call.callee else { continue };
                    if callee.name != "$props" {
                        continue;
                    }
                    let BindingPattern::ObjectPattern(pattern) = &declarator.id else { continue };
                    if pattern.rest.is_some() {
                        // `...rest` captures the remaining props: everything
                        // is potentially used (upstream bails the same way).
                        continue;
                    }
                    for property in &pattern.properties {
                        let name = match &property.key {
                            PropertyKey::StaticIdentifier(identifier) => {
                                identifier.name.to_string()
                            }
                            PropertyKey::StringLiteral(literal) => literal.value.to_string(),
                            // Computed or otherwise dynamic keys can't be
                            // matched statically.
                            _ => continue,
                        };
                        let local = match &property.value {
                            BindingPattern::BindingIdentifier(identifier) => &identifier.name,
                            BindingPattern::AssignmentPattern(assignment) => {
                                match &assignment.left {
                                    BindingPattern::BindingIdentifier(identifier) => {
                                        &identifier.name
                                    }
                                    // Nested pattern with default: treated
                                    // as used.
                                    _ => continue,
                                }
                            }
                            // Nested destructuring: treated as used.
                            _ => continue,
                        };
                        props.push(Prop { name, local_name: local.as_str(), span: property.span });
                    }
                }
            }
            if props.is_empty() {
                continue;
            }

            let semantic = SemanticBuilder::new_linter().build(program).semantic;
            let scoping = semantic.scoping();
            for prop in props {
                let used_in_script = scoping
                    .get_binding(scoping.root_scope_id(), prop.local_name.into())
                    .is_some_and(|symbol_id| {
                        scoping.get_resolved_references(symbol_id).next().is_some()
                    });
                if used_in_script || markup_identifiers.contains(prop.local_name) {
                    continue;
                }
                reports.push((
                    prop.name,
                    Span::new(prop.span.start + script.offset, prop.span.end + script.offset),
                ));
            }
        }

        reports.sort_unstable_by_key(|(_, span)| span.start);
        for (name, span) in reports {
            ctx.diagnostic(no_unused_props_diagnostic(&name, span));
        }
    }
}

/// Add every identifier of every markup expression slice to `out`:
/// mustaches, `{@…}` tag expressions, attribute value expression parts,
/// spreads, directive values, referencing shorthand/directive names, and
/// block headers. Deliberately over-approximates (see the rule comment).
fn collect_markup_identifiers<'a>(nodes: &[Node<'a>], out: &mut FxHashSet<&'a str>) {
    walk_svelte_nodes(nodes, &mut |node| match node {
        Node::Mustache(tag) => scan_identifiers(tag.expression, out),
        Node::Tag(tag) => scan_identifiers(tag.expression, out),
        Node::Element(element) => {
            for attribute in &element.attributes {
                match &attribute.kind {
                    AttributeKind::Plain { value, .. } => {
                        if let Some(value) = value {
                            for part in &value.parts {
                                if let ValuePart::Expression(tag) = part {
                                    scan_identifiers(tag.expression, out);
                                }
                            }
                        }
                    }
                    AttributeKind::Shorthand { name, .. } => {
                        out.insert(name);
                    }
                    AttributeKind::Spread { expression, .. } => scan_identifiers(expression, out),
                    AttributeKind::Directive(directive) => {
                        // Names that reference a variable: actions,
                        // transitions, animations always; `bind:x` /
                        // `class:x` / `style:x` in shorthand form.
                        let name_references = matches!(
                            directive.kind,
                            DirectiveKind::Use
                                | DirectiveKind::Transition
                                | DirectiveKind::In
                                | DirectiveKind::Out
                                | DirectiveKind::Animate
                        ) || (directive.value.is_none()
                            && matches!(
                                directive.kind,
                                DirectiveKind::Bind | DirectiveKind::Class | DirectiveKind::Style
                            ));
                        if name_references {
                            scan_identifiers(directive.name, out);
                        }
                        if let Some(value) = &directive.value {
                            for part in &value.parts {
                                if let ValuePart::Expression(tag) = part {
                                    scan_identifiers(tag.expression, out);
                                }
                            }
                        }
                    }
                }
            }
        }
        Node::Block(block) => match &block.kind {
            BlockKind::If(if_block) => {
                for branch in &if_block.branches {
                    if let Some(expression) = &branch.expression {
                        scan_identifiers(expression.text, out);
                    }
                }
            }
            BlockKind::Each(each) => {
                scan_identifiers(each.expression.text, out);
                if let Some(key) = &each.key {
                    scan_identifiers(key.text, out);
                }
            }
            BlockKind::Await(await_block) => scan_identifiers(await_block.expression.text, out),
            BlockKind::Key(key) => scan_identifiers(key.expression.text, out),
            BlockKind::Snippet(snippet) => {
                if let Some(params) = &snippet.params {
                    scan_identifiers(params.text, out);
                }
            }
            BlockKind::Unknown(unknown) => scan_identifiers(unknown.header_rest.text, out),
        },
        _ => {}
    });
}

/// Lexical identifier scan: every maximal `[A-Za-z_$][A-Za-z0-9_$]*` run
/// not directly preceded by `.` (property names don't reference variables).
/// String/comment contents are NOT excluded — over-counting is the
/// documented direction.
fn scan_identifiers<'a>(text: &'a str, out: &mut FxHashSet<&'a str>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            if start == 0 || bytes[start - 1] != b'.' {
                out.insert(&text[start..i]);
            }
        } else if byte.is_ascii_digit() {
            // Swallow number-like runs (`1e3`, `0x1f`) so their letter
            // parts don't register as identifiers.
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUnusedProps;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Used in script (upstream valid/basic, sans types).
            (
                "<script>
	let { a, b } = $props();
	console.log(a, b);
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Used in a markup mustache.
            (
                "<script>
	let { name } = $props();
</script>

<h1>{name}</h1>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Used in an attribute expression and via shorthand.
            (
                "<script>
	let { href, title } = $props();
</script>

<a href={href} {title}>link</a>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Renamed prop whose local is used (upstream valid/alias).
            (
                "<script>
	let { test, 'aria-label': ariaLabel } = $props();
</script>

<h1>{test}</h1>
<div aria-label={ariaLabel}>x</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Defaults don't hide uses.
            (
                "<script>
	let { size = 10 } = $props();
</script>

<div style:width=\"{size}px\"></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A rest element disables the check (upstream bails the same
            // way: everything may be forwarded).
            (
                "<script>
	let { a, ...rest } = $props();
</script>

<div {...rest}></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Used in block headers.
            (
                "<script>
	let { items, promise } = $props();
</script>

{#each items as item}{item}{/each}
{#await promise}...{/await}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Used through a directive name (`use:action`).
            (
                "<script>
	let { action } = $props();
</script>

<div use:action></div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Snippet prop rendered with `{@render}`.
            (
                "<script>
	let { children } = $props();
</script>

{@render children?.()}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Only `$props()` destructurings are checked.
            (
                "<script>
	let { unused } = notProps();
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested destructuring is treated as used (upstream recurses
            // into the nested type instead; documented deviation).
            (
                "<script>
	let { point: { x } } = $props();
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // SUBSET BOUNDARY (documented): a type-only prop that is never
            // destructured is upstream's main case and is NOT detected here
            // (needs the TypeScript type checker).
            (
                "<script lang=\"ts\">
	interface Props {
		name: string;
		age: number;
	}
	let { name }: Props = $props();
	console.log(name);
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // SUBSET BOUNDARY (documented): the markup scan is lexical, so
            // a shadowing `{#each}` item name counts as a use of the prop
            // (false negative by design).
            (
                "<script>
	let { item } = $props();
	const list = [1, 2];
</script>

{#each list as item}{item}{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            // Never used anywhere.
            (
                "<script>
	let { a } = $props();
</script>

<div>static</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // One of two unused (upstream invalid/simple-unused, sans
            // types).
            (
                "<script>
	let { name, age } = $props();
	console.log(name);
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Renamed prop: the report names the prop, not the local.
            (
                "<script>
	let { value: local } = $props();
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // String-keyed prop (upstream invalid/alias reports the
            // *type's* other property; here the unused binding itself).
            (
                "<script>
	let { 'aria-label': foo } = $props();
</script>

<div>x</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A default value doesn't count as a use.
            (
                "<script>
	let { size = 10 } = $props();
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Script-side analysis is semantic, not lexical: a string with
            // the same word is not a use.
            (
                "<script>
	let { a } = $props();
	console.log('a');
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Multiple unused props → one report each.
            (
                "<script>
	let { one, two, three } = $props();
	console.log(two);
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoUnusedProps::NAME, NoUnusedProps::PLUGIN, pass, fail).test_and_snapshot();
    }
}
