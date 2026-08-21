<script lang="ts">
	import type { Snippet } from "svelte";
	const SAMPLE_DATA = { mentionable: [{ type: "page" }] };
	type MobileLinkProps = { href: string; content: Snippet; class?: string };
	type DependencyNode = { kind: string };
</script>

<!-- A snippet header is a function signature with the keyword left out, and is
     formatted as one. Read as an expression instead, `name(params)` is a call:
     the parentheses would hold arguments, laid out as an argument list and
     meaning what an argument means. -->
{#snippet MentionableIcon({ item }: { item: (typeof SAMPLE_DATA.mentionable)[0] })}
	{#if item.type === "page"}<span>page</span>{/if}
{/snippet}

{#snippet MobileLink({ href, content, class: className, ...props }: MobileLinkProps)}
	<a {href} class={className} {...props}>{@render content()}</a>
{/snippet}

<!-- A default value, which as an argument would be an assignment and would be
     parenthesized. -->
{#snippet contentBox(tightTop = false)}
	<div class:tight={tightTop}></div>
{/snippet}

<!-- A parameter's type annotation, which as an argument does not parse at all —
     and whose type literal takes `;` between its members. -->
{#snippet DependencyNode({ node, level }: { node: DependencyNode, level: number })}
	<span>{node.kind}{level}</span>
{/snippet}
