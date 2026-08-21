<script lang="ts">
	let path = 'u/alice';
	let text = 'a}b';
</script>

<!-- A regex literal is scanned whole, so the characters in it do not end the
     interpolation. The first is the one that bit: `\/` immediately before the
     closing delimiter spells `//`, which a scanner looking for line comments
     follows to end of line, taking the closing brace with it. -->
<p>{path.replace(/^u\//, '')}</p>
<p>{text.replace(/}/, '')}</p>
<p>{text.replace(/'/, '')}</p>
<p>{text.split(/a\/b/).length}</p>

<!-- The other reading of `/` has to survive too. -->
<p>{path.length / 2}</p>
<p>{(path.length + 1) / 2}</p>

<!-- And a `/` right after `{` is a block closer, not a regex. -->
{#each path.split(/\//) as segment}
	<span>{segment}</span>
{/each}
