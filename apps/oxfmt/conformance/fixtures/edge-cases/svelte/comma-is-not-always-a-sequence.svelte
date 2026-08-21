<script lang="ts">
	let a = 1;
	let b = 2;
	let open = false;
	let items = [1, 2, 3];
	const get = () => open;
	const set = (v: boolean) => (open = v);
</script>

<!-- A slot that holds one expression reads a top-level comma as JavaScript's
     sequence operator, and it keeps the parentheses that tell it from an
     argument list. -->
<p>{a, b}</p>
{#key a, b}<span>keyed</span>{/key}
{#if a, b}<span>yes</span>{/if}

<!-- Svelte spells two of its own forms with a comma, in a slot handed over
     whole: the index of an `{#each}` written without `as`, and the pair of
     functions of a binding. Parenthesizing there would join two things Svelte
     reads separately — and for the `{#each}`, lose the index. -->
{#each Array(5), i}<span>{i}</span>{/each}
{#each items as item, i}<span>{item}{i}</span>{/each}
<Dialog bind:open={get, set} />

<!-- A negated logical hugs its parentheses rather than take a break of its
     own: the `!(` sits against the chain, and the break the chain already
     carries is the only one. -->
<button
	onclick={() => {
		if (!(a === 1 || b === 2 || items.length > 0 || open || a + b > 2 || items[0] === 1)) {
			open = true;
		}
	}}>go</button
>
