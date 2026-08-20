# externals/plugin-svelte/each-await-block-destructuring.svelte

> Reduced port: an `{#each … as PATTERN}` / `{:then PATTERN}` binding keeps its spelling. Prettier re-serializes it with a bespoke pattern printer (`expandNode`) that preserves literal spelling and never breaks; the fragment path here would reach it through the estree printer, which does neither. Canonical spacing already matches. See crates/oxc_formatter_svelte/AGENTS.md

## Option 1

`````json
{"printWidth":80,"svelte":{}}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -1,16 +1,16 @@
-{#each arr as { a, b = '' }}
+{#each arr as {  a,b =''}}
   {a}
   {b}
 {/each}
 
-{#await promise then { a, b = '' }}
+{#await promise then {  a,b =''}}
   {a}
   {b}
 {/await}
 
 {#await promise}
   Loading
-{:then { a, b = '' }}
+{:then {  a,b =''}}
   {a}
   {b}
 {/await}

`````

### Actual (oxfmt)

`````svelte
{#each arr as {  a,b =''}}
  {a}
  {b}
{/each}

{#await promise then {  a,b =''}}
  {a}
  {b}
{/await}

{#await promise}
  Loading
{:then {  a,b =''}}
  {a}
  {b}
{/await}

`````

### Expected (prettier)

`````svelte
{#each arr as { a, b = '' }}
  {a}
  {b}
{/each}

{#await promise then { a, b = '' }}
  {a}
  {b}
{/await}

{#await promise}
  Loading
{:then { a, b = '' }}
  {a}
  {b}
{/await}

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
@@ -1,16 +1,16 @@
-{#each arr as { a, b = '' }}
+{#each arr as {  a,b =''}}
   {a}
   {b}
 {/each}
 
-{#await promise then { a, b = '' }}
+{#await promise then {  a,b =''}}
   {a}
   {b}
 {/await}
 
 {#await promise}
   Loading
-{:then { a, b = '' }}
+{:then {  a,b =''}}
   {a}
   {b}
 {/await}

`````

### Actual (oxfmt)

`````svelte
{#each arr as {  a,b =''}}
  {a}
  {b}
{/each}

{#await promise then {  a,b =''}}
  {a}
  {b}
{/await}

{#await promise}
  Loading
{:then {  a,b =''}}
  {a}
  {b}
{/await}

`````

### Expected (prettier)

`````svelte
{#each arr as { a, b = '' }}
  {a}
  {b}
{/each}

{#await promise then { a, b = '' }}
  {a}
  {b}
{/await}

{#await promise}
  Loading
{:then { a, b = '' }}
  {a}
  {b}
{/await}

`````
