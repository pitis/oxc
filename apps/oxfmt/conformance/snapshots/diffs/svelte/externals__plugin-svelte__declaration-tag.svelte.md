# externals/plugin-svelte/declaration-tag.svelte

> Reduced port: `{let a = 1, b = 2}` keeps its spelling. A declaration tag's single declarator is formatted through the expression path, where two of them would come back as the sequence expression `(a = 1), (b = 2)` — a different declaration that does not parse as one. See crates/oxc_formatter_svelte/AGENTS.md

## Option 1

`````json
{"printWidth":80,"svelte":{}}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -12,10 +12,9 @@
 {/if}
 
 <Component>
   {let value = getValue()}
-  {let one = 1,
-    two = 2}
+  {let one=1, two=2}
 </Component>
 
 {#if foo}
   {const bar = await 1}

`````

### Actual (oxfmt)

`````svelte
{#each boxes as box}
  {const area = box.width * box.height}
  {const label = (value) => `${value} square pixels`}
  <p>{label(area)}</p>
{/each}

{#if user}
  {let name = $state(user.name)}
  {let greeting = $derived(`Hello ${name}`)}
  <input bind:value={name} />
  <p>{greeting}</p>
{/if}

<Component>
  {let value = getValue()}
  {let one=1, two=2}
</Component>

{#if foo}
  {const bar = await 1}
  <div>
    {const bar = "shadowing"}
    {bar}
  </div>
  {bar}
{/if}

`````

### Expected (prettier)

`````svelte
{#each boxes as box}
  {const area = box.width * box.height}
  {const label = (value) => `${value} square pixels`}
  <p>{label(area)}</p>
{/each}

{#if user}
  {let name = $state(user.name)}
  {let greeting = $derived(`Hello ${name}`)}
  <input bind:value={name} />
  <p>{greeting}</p>
{/if}

<Component>
  {let value = getValue()}
  {let one = 1,
    two = 2}
</Component>

{#if foo}
  {const bar = await 1}
  <div>
    {const bar = "shadowing"}
    {bar}
  </div>
  {bar}
{/if}

`````

## Option 2

`````json
{"printWidth":120,"singleQuote":true,"htmlWhitespaceSensitivity":"ignore","bracketSameLine":true,"svelteIndentScriptAndStyle":true,"svelteSortOrder":"options-scripts-styles-markup","svelte":{"indentScriptAndStyle":true,"sortOrder":"options-scripts-styles-markup"}}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -12,10 +12,9 @@
 {/if}
 
 <Component>
   {let value = getValue()}
-  {let one = 1,
-    two = 2}
+  {let one=1, two=2}
 </Component>
 
 {#if foo}
   {const bar = await 1}

`````

### Actual (oxfmt)

`````svelte
{#each boxes as box}
  {const area = box.width * box.height}
  {const label = (value) => `${value} square pixels`}
  <p>{label(area)}</p>
{/each}

{#if user}
  {let name = $state(user.name)}
  {let greeting = $derived(`Hello ${name}`)}
  <input bind:value={name} />
  <p>{greeting}</p>
{/if}

<Component>
  {let value = getValue()}
  {let one=1, two=2}
</Component>

{#if foo}
  {const bar = await 1}
  <div>
    {const bar = 'shadowing'}
    {bar}
  </div>
  {bar}
{/if}

`````

### Expected (prettier)

`````svelte
{#each boxes as box}
  {const area = box.width * box.height}
  {const label = (value) => `${value} square pixels`}
  <p>{label(area)}</p>
{/each}

{#if user}
  {let name = $state(user.name)}
  {let greeting = $derived(`Hello ${name}`)}
  <input bind:value={name} />
  <p>{greeting}</p>
{/if}

<Component>
  {let value = getValue()}
  {let one = 1,
    two = 2}
</Component>

{#if foo}
  {const bar = await 1}
  <div>
    {const bar = 'shadowing'}
    {bar}
  </div>
  {bar}
{/if}

`````
