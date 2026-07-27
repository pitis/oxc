# externals/prettier/vue/interpolation/bitwise-or-operator.vue

> Vue 2 filter pipes (`{{ x | f }}`) not yet formatted with leading-`|` layout (removed in Vue 3)

## Option 1

`````json
{"printWidth":80}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -5,12 +5,12 @@
         bitwise |
           or |
           operator |
           a_long_long_long_long_long_long_long_long_long_long_variable,
-      )
-        | filter1
-        | filter2
-        | filter3
-        | filter4
+      ) |
+      filter1 |
+      filter2 |
+      filter3 |
+      filter4
     }}
   </div>
 </template>

`````

### Actual (oxfmt)

`````vue
<template>
  <div>
    {{
      fn(
        bitwise |
          or |
          operator |
          a_long_long_long_long_long_long_long_long_long_long_variable,
      ) |
      filter1 |
      filter2 |
      filter3 |
      filter4
    }}
  </div>
</template>

`````

### Expected (prettier)

`````vue
<template>
  <div>
    {{
      fn(
        bitwise |
          or |
          operator |
          a_long_long_long_long_long_long_long_long_long_long_variable,
      )
        | filter1
        | filter2
        | filter3
        | filter4
    }}
  </div>
</template>

`````

## Option 2

`````json
{"printWidth":100,"vueIndentScriptAndStyle":true,"singleQuote":true}
`````

### Diff

`````diff
===================================================================
--- prettier
+++ oxfmt
@@ -1,11 +1,11 @@
 <template>
   <div>
     {{
-      fn(bitwise | or | operator | a_long_long_long_long_long_long_long_long_long_long_variable)
-        | filter1
-        | filter2
-        | filter3
-        | filter4
+      fn(bitwise | or | operator | a_long_long_long_long_long_long_long_long_long_long_variable) |
+      filter1 |
+      filter2 |
+      filter3 |
+      filter4
     }}
   </div>
 </template>

`````

### Actual (oxfmt)

`````vue
<template>
  <div>
    {{
      fn(bitwise | or | operator | a_long_long_long_long_long_long_long_long_long_long_variable) |
      filter1 |
      filter2 |
      filter3 |
      filter4
    }}
  </div>
</template>

`````

### Expected (prettier)

`````vue
<template>
  <div>
    {{
      fn(bitwise | or | operator | a_long_long_long_long_long_long_long_long_long_long_variable)
        | filter1
        | filter2
        | filter3
        | filter4
    }}
  </div>
</template>

`````
