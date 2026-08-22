<script lang="ts">
	// An end-of-line comment before a union type travels with the union: it stays
	// on the operator's line while the members do, and moves down with them as
	// soon as they take an indent of their own.
	type Props = {
		stays?: // the members fit beside the operator, so the comment does too
		AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA | null;
		moves?:
			// the members take a line each, and the comment goes with them
			| AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
			| BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
			| null;
		alsoMoves?:
			// the members fit, but on a line of their own
			((value: string) => void) | null;
	};
	let props: Props = {};
</script>

<span>{props.stays}</span>
