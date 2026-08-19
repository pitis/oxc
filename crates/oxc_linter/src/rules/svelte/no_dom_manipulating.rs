use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashSet;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_scripts, walk_svelte_elements},
};

fn no_dom_manipulating_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Don't manipulate the DOM directly. The Svelte runtime can get confused if there is a difference between the actual DOM and the DOM expected by the Svelte runtime.",
    )
    .with_help(
        "Update the state that renders this element and let Svelte reconcile the DOM instead of mutating the `bind:this` element directly.",
    )
    .with_label(span)
}

/// Upstream's `DOM_MANIPULATING_METHODS`.
const DOM_MANIPULATING_METHODS: [&str; 15] = [
    "appendChild",  // https://developer.mozilla.org/en-US/docs/Web/API/Node/appendChild
    "insertBefore", // https://developer.mozilla.org/en-US/docs/Web/API/Node/insertBefore
    "normalize",    // https://developer.mozilla.org/en-US/docs/Web/API/Node/normalize
    "removeChild",  // https://developer.mozilla.org/en-US/docs/Web/API/Node/removeChild
    "replaceChild", // https://developer.mozilla.org/en-US/docs/Web/API/Node/replaceChild
    "after",        // https://developer.mozilla.org/en-US/docs/Web/API/Element/after
    "append",       // https://developer.mozilla.org/en-US/docs/Web/API/Element/append
    "before",       // https://developer.mozilla.org/en-US/docs/Web/API/Element/before
    "insertAdjacentElement", // https://developer.mozilla.org/en-US/docs/Web/API/Element/insertAdjacentElement
    "insertAdjacentHTML", // https://developer.mozilla.org/en-US/docs/Web/API/Element/insertAdjacentHTML
    "insertAdjacentText", // https://developer.mozilla.org/en-US/docs/Web/API/Element/insertAdjacentText
    "prepend",            // https://developer.mozilla.org/en-US/docs/Web/API/Element/prepend
    "remove",             // https://developer.mozilla.org/en-US/docs/Web/API/Element/remove
    "replaceChildren", // https://developer.mozilla.org/en-US/docs/Web/API/Element/replaceChildren
    "replaceWith",     // https://developer.mozilla.org/en-US/docs/Web/API/Element/replaceWith
];

/// Upstream's `DOM_MANIPULATING_PROPERTIES` (assignment targets).
const DOM_MANIPULATING_PROPERTIES: [&str; 5] = [
    "textContent", // https://developer.mozilla.org/en-US/docs/Web/API/Node/textContent
    "innerHTML",   // https://developer.mozilla.org/en-US/docs/Web/API/Element/innerHTML
    "outerHTML",   // https://developer.mozilla.org/en-US/docs/Web/API/Element/outerHTML
    "innerText",   // https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/innerText
    "outerText",   // https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/outerText
];

#[derive(Debug, Default, Clone)]
pub struct NoDomManipulating;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows direct DOM manipulation of elements captured with
    /// `bind:this`: calling DOM-mutating methods (`remove`, `appendChild`,
    /// `insertBefore`, …) on them, or assigning to DOM-mutating properties
    /// (`innerHTML`, `textContent`, …).
    ///
    /// ### Why is this bad?
    ///
    /// Svelte renders the element from its template; mutating that element's
    /// DOM by hand puts the real DOM out of sync with the DOM the Svelte
    /// runtime expects, which can break later updates in confusing ways.
    /// State changes should drive the template instead.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   let div;
    ///   const remove = () => div.remove();
    /// </script>
    ///
    /// <div bind:this={div}>div</div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   let div;
    ///   let show = true;
    ///   const remove = () => (show = false);
    /// </script>
    ///
    /// {#if show}
    ///   <div bind:this={div}>div</div>
    /// {/if}
    /// ```
    NoDomManipulating,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow direct DOM manipulation of `bind:this` elements.",
);

impl Rule for NoDomManipulating {}

// Ports eslint-plugin-svelte's `no-dom-manipulating`.
//
// Upstream resolves the `bind:this={id}` expression with the parser's unified
// template+script scope analysis, keeps only variables living in the module
// or global scope, and then inspects every reference of those variables.
// This hybrid port re-creates that flow per `<script>` block: the markup
// pass collects the bound names, each script is parsed with `oxc_parser` and
// analyzed with `oxc_semantic`, and only *top-level* script bindings with a
// collected name are traced through their resolved references (so a name
// bound only by template scopes — e.g. an `{#each}` item — is correctly
// ignored, and shadowed uses inside the script are correctly excluded).
//
// Documented deviations from upstream:
// - The `bind:this` name is matched to a script binding purely by name.
//   Upstream's shared scope analysis knows when the template shadows a
//   script variable (`{#each list as item}` + `bind:this={item}` while a
//   top-level `let item` also exists → upstream skips, we would report).
// - Each `<script>` block is parsed and analyzed independently, so a
//   variable declared in one block and manipulated in another is missed.
// - Upstream resolves computed member names with
//   `@eslint-community/eslint-utils`'s `getPropertyName` (constants,
//   template literals, …); this port resolves only `x.prop` and
//   `x["prop"]` (string literal) forms.
impl SvelteTemplateRule for NoDomManipulating {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        // Names bound via `bind:this={identifier}` on HTML elements
        // (including `<svelte:element>`, matching upstream's `isHTMLElement`;
        // components and other `svelte:*` specials are skipped).
        let mut bound_names: FxHashSet<&str> = FxHashSet::default();
        walk_svelte_elements(nodes, &mut |element| {
            if element.is_component_like() {
                return;
            }
            if element.svelte_name().is_some_and(|name| name != "element") {
                return;
            }
            for attribute in &element.attributes {
                let AttributeKind::Directive(directive) = &attribute.kind else { continue };
                if directive.kind != DirectiveKind::Bind || directive.name != "this" {
                    continue;
                }
                let Some(value) = &directive.value else { continue };
                let Some(tag) = value.as_single_expression() else { continue };
                let (text, _) = tag.trimmed();
                // Upstream only tracks `bind:this={id}` where the expression
                // is a plain identifier.
                if is_simple_identifier(text) {
                    bound_names.insert(text);
                }
            }
        });
        if bound_names.is_empty() {
            return;
        }

        let mut spans = Vec::new();
        for script in svelte_scripts(nodes, ctx.source_text()) {
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let allocator = Allocator::new();
            let parser_ret = Parser::new(&allocator, script.content, source_type).parse();
            if parser_ret.panicked {
                continue;
            }
            let program = allocator.alloc(parser_ret.program);
            let semantic = SemanticBuilder::new_linter().build(program).semantic;
            let scoping = semantic.scoping();
            let nodes = semantic.nodes();

            for name in &bound_names {
                // Upstream keeps only module/global-scope variables; the
                // equivalent here is a binding in the script's root scope.
                let Some(symbol_id) = scoping.get_binding(scoping.root_scope_id(), (*name).into())
                else {
                    continue;
                };
                for reference in scoping.get_resolved_references(symbol_id) {
                    let identifier_node = nodes.get_node(reference.node_id());
                    let identifier_span = identifier_node.kind().span();

                    // The identifier must be the *object* of a member
                    // expression with a statically-known property name.
                    let member_node = nodes.parent_node(reference.node_id());
                    let (property, member_span) = match member_node.kind() {
                        AstKind::StaticMemberExpression(member)
                            if member.object.span() == identifier_span =>
                        {
                            (member.property.name.as_str(), member.span)
                        }
                        AstKind::ComputedMemberExpression(member)
                            if member.object.span() == identifier_span =>
                        {
                            match &member.expression {
                                oxc_ast::ast::Expression::StringLiteral(literal) => {
                                    (literal.value.as_str(), member.span)
                                }
                                _ => continue,
                            }
                        }
                        _ => continue,
                    };

                    // Skip through `?.` chains and parentheses, as upstream
                    // skips `ChainExpression` parents (espree has no
                    // parenthesized nodes; oxc preserves them).
                    let mut target = member_node;
                    let mut parent = nodes.parent_node(target.id());
                    while matches!(
                        parent.kind(),
                        AstKind::ChainExpression(_) | AstKind::ParenthesizedExpression(_)
                    ) {
                        target = parent;
                        parent = nodes.parent_node(target.id());
                    }
                    let target_span = target.kind().span();

                    let manipulates = match parent.kind() {
                        AstKind::CallExpression(call) => {
                            call.callee.span() == target_span
                                && DOM_MANIPULATING_METHODS.contains(&property)
                        }
                        AstKind::AssignmentExpression(assignment) => {
                            assignment.left.span() == target_span
                                && DOM_MANIPULATING_PROPERTIES.contains(&property)
                        }
                        _ => false,
                    };
                    if manipulates {
                        // Report on the member expression, like upstream,
                        // shifted to a file-absolute span.
                        spans.push(Span::new(
                            member_span.start + script.offset,
                            member_span.end + script.offset,
                        ));
                    }
                }
            }
        }

        spans.sort_unstable_by_key(|span| span.start);
        for span in spans {
            ctx.diagnostic(no_dom_manipulating_diagnostic(span));
        }
    }
}

/// A plain ASCII identifier (`foo`, `_bar`, `$baz`). Upstream accepts any
/// `Identifier` expression; the ASCII restriction is a documented
/// simplification (non-ASCII identifiers are rare in `bind:this`).
fn is_simple_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDomManipulating;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // State toggle instead of DOM mutation (upstream valid/remove01).
            (
                "<script>
	let div;
	let show;
	const toggle = () => (show = !show);
</script>

{#if show}
	<div bind:this={div}>div</div>
{/if}

<button on:click={() => toggle()}>Click Me</button>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Reading a property is fine (upstream valid/read-prop01).
            (
                "<script>
	let divElement;
	let height = '';
	$: if (divElement) height = `${divElement.clientHeight}px`;
</script>

<div bind:this={divElement} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Unknown method / dynamic member (upstream valid/unknown-method01,
            // valid/computed-member01).
            (
                "<script>
	let div;
	const remove = () => div.unknown();
</script>

<div bind:this={div}>div</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<script>
	let div;
	const remove = () => div[remove]();
</script>

<div bind:this={div}>div</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // The variable is not bound with `bind:this` (upstream
            // valid/non-bind-this01).
            (
                "<script>
	let foo;
	const remove = () => foo.remove();
</script>

<input bind:value={foo} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `bind:this` on an `{#each}` item is a template-scope variable,
            // not a script one (upstream valid/loop01).
            (
                "<script>
	const list = [1, 2, 3];
	const remove = () => {
		for (const item of list) {
			item.remove();
		}
	};
</script>

{#each list as item}
	<input bind:this={item} />
{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `bind:this` on a component is not a DOM element.
            (
                "<script>
	let comp;
	const remove = () => comp.remove();
</script>

<MyComponent bind:this={comp} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Shadowed use resolves to the local, not the bound element
            // (subset boundary: our per-script semantic analysis gets this
            // right, like upstream).
            (
                "<script>
	let div;
	function local() {
		let div = { remove() {} };
		div.remove();
	}
</script>

<div bind:this={div}>div</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Passing the method around without calling it is not checked.
            (
                "<script>
	let div;
	const f = () => fn(div.remove);
</script>

<div bind:this={div}>div</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            // Upstream invalid/remove01.
            (
                "<script>
	let div;
	let show;
	const toggle = () => (show = !show);
	const remove = () => div.remove();
</script>

{#if show}
	<div bind:this={div}>div</div>
{/if}

<button on:click={() => toggle()}>Click Me</button>
<button on:click={() => remove()}>Click Me</button>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Optional chains and parenthesized chains (upstream
            // invalid/chain01).
            (
                "<script>
	let foo;
	const remove1 = () => {
		foo?.remove();
	};
	const remove2 = () => {
		(foo?.remove)();
	};
</script>

<p bind:this={foo}>div</p>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Every method from upstream's list (upstream
            // invalid/well-known-method01, abridged).
            (
                "<script>
	let foo;
	let div;
	const update = () => {
		const newNode = document.createElement('div');
		div.appendChild(newNode);
		div.insertBefore(newNode, foo);
		div.normalize();
		div.removeChild(foo);
		div.replaceChild(newNode, foo);
		div.after(newNode);
		div.append(newNode);
		div.before(newNode);
		div.insertAdjacentElement('beforebegin', newNode);
		div.insertAdjacentHTML('beforebegin', '<b>x</b>');
		div.insertAdjacentText('beforebegin', 'Foo');
		div.prepend(newNode);
		div.remove();
		div.replaceChildren(newNode);
		div.replaceWith(newNode);
	};
</script>

<div bind:this={div}>
	<div bind:this={foo} />
	div
</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Property assignments (upstream invalid/well-known-prop01).
            (
                "<script>
	let div;
	const update = () => {
		div.textContent = '';
		div.innerHTML = '';
		div.outerHTML = '';
		div.innerText = '';
		div.outerText = '';
	};
</script>

<div bind:this={div}>div</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `<svelte:element>` is an HTML element too (upstream
            // invalid/svelte-element01).
            (
                "<script>
	export let tag = 'div';
	let foo;
	let bar;
	const remove = () => {
		foo.remove();
		bar.remove();
	};
</script>

<p bind:this={foo}>div</p>
<svelte:element this={tag} bind:this={bar}>div</svelte:element>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // String-literal computed member.
            (
                "<script>
	let div;
	const update = () => {
		div['remove']();
	};
</script>

<div bind:this={div}>div</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoDomManipulating::NAME, NoDomManipulating::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
