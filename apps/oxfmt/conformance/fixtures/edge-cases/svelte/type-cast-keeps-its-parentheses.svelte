<script>
	// A JSDoc cast keeps its parentheses, and what sits inside them is not the
	// bare identifier it looks like — Prettier has a node for the parenthesis
	// there and this printer does not. Two rules read the object and have to be
	// told: the lookup after it keeps a break of its own rather than gluing, and
	// an assignment does not break after its operator to protect a chain that
	// reaches a cast.
	function handle(event) {
		const isMeta =
			/** @type {MouseEvent | KeyboardEvent} */ (event).metaKeyLongEnough ||
			/** @type {MouseEvent | KeyboardEvent} */ (event).ctrlKeyLongEnough;
		const isShift = /** @type {MouseEvent | KeyboardEvent} */ (event).shiftKeyLongEnough;
		// The same shape with ordinary parentheses already read this way, which is
		// what says the lookup half is right and the cast half was not.
		const isAlt = (event || globalThis.window).altKeyLongEnoughToNotFitOnTheLine;
		return isMeta && isShift && isAlt;
	}
</script>

<button onclick={handle}>go</button>
