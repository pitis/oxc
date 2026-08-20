# externals/plugin-svelte/region-markers.svelte

> Not implemented: a `<!-- #endregion -->` immediately after a hoisted `<script>`/`<style>` travels with it when sections are reordered (Prettier's `extractRegionEndTrailAfterHoistedEnd`). The *leading* comment does travel; only the trailing marker does not

## Option 1

`````json
{"printWidth":80,"svelte":{}}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -1,8 +1,9 @@
+<!-- #endregion -->
+
 <!-- #region MARKUP -->
 <body></body>
 
 <!-- #endregion -->
 
 <!-- #region STYLES -->
 <style></style>
-<!-- #endregion -->

`````

### Actual (oxfmt)

`````svelte
<!-- #endregion -->

<!-- #region MARKUP -->
<body></body>

<!-- #endregion -->

<!-- #region STYLES -->
<style></style>

`````

### Expected (prettier)

`````svelte
<!-- #region MARKUP -->
<body></body>

<!-- #endregion -->

<!-- #region STYLES -->
<style></style>
<!-- #endregion -->

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
@@ -1,6 +1,7 @@
 <!-- #region STYLES -->
 <style></style>
+
 <!-- #endregion -->
 
 <!-- #region MARKUP -->
 <body></body>

`````

### Actual (oxfmt)

`````svelte
<!-- #region STYLES -->
<style></style>

<!-- #endregion -->

<!-- #region MARKUP -->
<body></body>

<!-- #endregion -->

`````

### Expected (prettier)

`````svelte
<!-- #region STYLES -->
<style></style>
<!-- #endregion -->

<!-- #region MARKUP -->
<body></body>

<!-- #endregion -->

`````
