<script lang="ts">
	let settingsOnly = false;
	let workspace = 'demo';
</script>

<!-- A `<pre>` renders its own whitespace, and so does everything under it —
     a `{#if}` inside one has branches whose text is still inside the `<pre>`.
     What the author wrote is what the page shows, so none of it is laid out
     again, however far the lines run.
     The markers around it are not content: they are markup the browser never
     shows, so a branch still takes the whitespace at its own edges and prints
     it as the break before `{:else}` and `{/if}`. -->
<pre class="text-xs">
# Make sure your repo is up to date
git pull

{#if !settingsOnly}# Push from git repository to workspace
wmill sync push --workspace {workspace} --repository some/very/long/repository/path
	{:else}# Edit wmill.yaml file
vim wmill.yaml
git add wmill.yaml
	{/if}# Commit changes
git commit
git push</pre>
