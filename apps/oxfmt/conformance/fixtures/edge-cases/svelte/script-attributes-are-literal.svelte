<!-- Svelte does not interpolate in a `<script>` or `<style>` tag's attributes,
     so a `{…}` there is text. `generics` is the one that matters: its value is
     a type parameter list, and laying out `{ label?: string }` as though it
     were an object is laying out prose. Only the quoting is normalized. -->
<script lang='ts' generics="Item extends { label?: string; value: any }" data-b="a {  x  } b">
	let x = 1;
</script>

<style lang='css' data-b="a {  x  } b">
	a {
		color: red;
	}
</style>

<!-- An ordinary element's attribute value does interpolate, and the expression
     in it is formatted. -->
<span title="a {  x  } b">{x}</span>
