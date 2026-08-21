# externals/plugin-svelte/long-mustache-value.svelte

> Layout-only, and only at printWidth 120: where a text fill breaks around an embedded `{…}`. Prettier keeps the mustache and the word after it on one line and breaks later; ours breaks before that word. The indentation half of this fixture is fixed — a `{…}` is spliced bare, so its expression now indents itself (see the `svelte-expression` route)

## Option 2

`````json
{"printWidth":120,"singleQuote":true,"htmlWhitespaceSensitivity":"ignore","bracketSameLine":true,"svelteIndentScriptAndStyle":true,"svelteSortOrder":"options-scripts-styles-markup","svelte":{"indentScriptAndStyle":true,"sortOrder":"options-scripts-styles-markup"}}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -3,9 +3,9 @@
   class="attributes are not broken up by default which is the prettier behavior but {'breakable' +
     'mustache tags'} can be broken up" />
 
 <p>
-  Text with spaces inbetween should break at the most fitting point and not wait for a {'breakable' + 'mustache tags'} to
-  be broken up. {'If the mustache tag itself is very long, however' +
+  Text with spaces inbetween should break at the most fitting point and not wait for a {'breakable' + 'mustache tags'}
+  to be broken up. {'If the mustache tag itself is very long, however' +
     'it should be broken uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuup'}
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
