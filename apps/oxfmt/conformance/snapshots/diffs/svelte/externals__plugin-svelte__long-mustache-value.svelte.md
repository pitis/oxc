# externals/plugin-svelte/long-mustache-value.svelte

> Layout-only: a `{…}` whose expression breaks continues at the mustache's indent, where Prettier adds one level. Prettier prints the expression as a real estree node (whose unknown parent makes `printBinaryishExpression` indent); this goes through the JS *fragment* path, which does not. Never changes meaning

## Option 1

`````json
{"printWidth":80,"svelte":{}}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -1,12 +1,12 @@
 <input
   type="password"
   class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
-    'mustache tags'} can be broken up"
+  'mustache tags'} can be broken up"
 />
 
 <p>
   Text with spaces inbetween should break at the most fitting point and not wait
   for a {"breakable" + "mustache tags"} to be broken up. {"If the mustache tag itself is very long, however" +
-    "it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup"}
+  "it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup"}
   {andIiiiiiiiiiiiiiiiiiiiiiiiiiiWillNeverBreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaakaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
 </p>

`````

### Actual (oxfmt)

`````svelte
<input
  type="password"
  class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
  'mustache tags'} can be broken up"
/>

<p>
  Text with spaces inbetween should break at the most fitting point and not wait
  for a {"breakable" + "mustache tags"} to be broken up. {"If the mustache tag itself is very long, however" +
  "it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup"}
  {andIiiiiiiiiiiiiiiiiiiiiiiiiiiWillNeverBreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaakaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
</p>

`````

### Expected (prettier)

`````svelte
<input
  type="password"
  class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
    'mustache tags'} can be broken up"
/>

<p>
  Text with spaces inbetween should break at the most fitting point and not wait
  for a {"breakable" + "mustache tags"} to be broken up. {"If the mustache tag itself is very long, however" +
    "it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup"}
  {andIiiiiiiiiiiiiiiiiiiiiiiiiiiWillNeverBreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaakaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
</p>

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
@@ -1,11 +1,11 @@
 <input
   type="password"
   class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
-    'mustache tags'} can be broken up" />
+  'mustache tags'} can be broken up" />
 
 <p>
-  Text with spaces inbetween should break at the most fitting point and not wait for a {'breakable' + 'mustache tags'} to
-  be broken up. {'If the mustache tag itself is very long, however' +
-    'it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup'}
+  Text with spaces inbetween should break at the most fitting point and not wait for a {'breakable' + 'mustache tags'}
+  to be broken up. {'If the mustache tag itself is very long, however' +
+  'it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup'}
   {andIiiiiiiiiiiiiiiiiiiiiiiiiiiWillNeverBreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaakaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
 </p>

`````

### Actual (oxfmt)

`````svelte
<input
  type="password"
  class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
  'mustache tags'} can be broken up" />

<p>
  Text with spaces inbetween should break at the most fitting point and not wait for a {'breakable' + 'mustache tags'}
  to be broken up. {'If the mustache tag itself is very long, however' +
  'it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup'}
  {andIiiiiiiiiiiiiiiiiiiiiiiiiiiWillNeverBreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaakaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
</p>

`````

### Expected (prettier)

`````svelte
<input
  type="password"
  class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
    'mustache tags'} can be broken up" />

<p>
  Text with spaces inbetween should break at the most fitting point and not wait for a {'breakable' + 'mustache tags'} to
  be broken up. {'If the mustache tag itself is very long, however' +
    'it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup'}
  {andIiiiiiiiiiiiiiiiiiiiiiiiiiiWillNeverBreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaakaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
</p>

`````
