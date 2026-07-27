# externals/prettier/vue/vue/filter.vue

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
@@ -1,38 +1,38 @@
 <!-- vue filters are only allowed in v-bind and interpolation -->
 <template>
   <div class="allowed">
     {{
-      value
-        | thisIsARealSuperLongFilterPipe("arg1", arg2)
-        | anotherPipeLongJustForFun
-        | pipeTheThird
+      value |
+      thisIsARealSuperLongFilterPipe("arg1", arg2) |
+      anotherPipeLongJustForFun |
+      pipeTheThird
     }}
   </div>
   <div
     class="allowed"
     v-bind:something="
-      value
-        | thisIsARealSuperLongFilterPipe('arg1', arg2)
-        | anotherPipeLongJustForFun
-        | pipeTheThird
+      value |
+      thisIsARealSuperLongFilterPipe('arg1', arg2) |
+      anotherPipeLongJustForFun |
+      pipeTheThird
     "
   ></div>
   <div
     class="allowed"
     :class="
-      value
-        | thisIsARealSuperLongFilterPipe('arg1', arg2)
-        | anotherPipeLongJustForFun
-        | pipeTheThird
+      value |
+      thisIsARealSuperLongFilterPipe('arg1', arg2) |
+      anotherPipeLongJustForFun |
+      pipeTheThird
     "
   ></div>
   <div
     class="not-allowed"
     v-if="
       value |
-        thisIsARealSuperLongBitwiseOr('arg1', arg2) |
-        anotherBitwiseOrLongJustForFun |
-        bitwiseOrTheThird
+      thisIsARealSuperLongBitwiseOr('arg1', arg2) |
+      anotherBitwiseOrLongJustForFun |
+      bitwiseOrTheThird
     "
   ></div>
 </template>

`````

### Actual (oxfmt)

`````vue
<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div class="allowed">
    {{
      value |
      thisIsARealSuperLongFilterPipe("arg1", arg2) |
      anotherPipeLongJustForFun |
      pipeTheThird
    }}
  </div>
  <div
    class="allowed"
    v-bind:something="
      value |
      thisIsARealSuperLongFilterPipe('arg1', arg2) |
      anotherPipeLongJustForFun |
      pipeTheThird
    "
  ></div>
  <div
    class="allowed"
    :class="
      value |
      thisIsARealSuperLongFilterPipe('arg1', arg2) |
      anotherPipeLongJustForFun |
      pipeTheThird
    "
  ></div>
  <div
    class="not-allowed"
    v-if="
      value |
      thisIsARealSuperLongBitwiseOr('arg1', arg2) |
      anotherBitwiseOrLongJustForFun |
      bitwiseOrTheThird
    "
  ></div>
</template>

`````

### Expected (prettier)

`````vue
<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div class="allowed">
    {{
      value
        | thisIsARealSuperLongFilterPipe("arg1", arg2)
        | anotherPipeLongJustForFun
        | pipeTheThird
    }}
  </div>
  <div
    class="allowed"
    v-bind:something="
      value
        | thisIsARealSuperLongFilterPipe('arg1', arg2)
        | anotherPipeLongJustForFun
        | pipeTheThird
    "
  ></div>
  <div
    class="allowed"
    :class="
      value
        | thisIsARealSuperLongFilterPipe('arg1', arg2)
        | anotherPipeLongJustForFun
        | pipeTheThird
    "
  ></div>
  <div
    class="not-allowed"
    v-if="
      value |
        thisIsARealSuperLongBitwiseOr('arg1', arg2) |
        anotherBitwiseOrLongJustForFun |
        bitwiseOrTheThird
    "
  ></div>
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
@@ -1,38 +1,38 @@
 <!-- vue filters are only allowed in v-bind and interpolation -->
 <template>
   <div class="allowed">
     {{
-      value
-        | thisIsARealSuperLongFilterPipe('arg1', arg2)
-        | anotherPipeLongJustForFun
-        | pipeTheThird
+      value |
+      thisIsARealSuperLongFilterPipe('arg1', arg2) |
+      anotherPipeLongJustForFun |
+      pipeTheThird
     }}
   </div>
   <div
     class="allowed"
     v-bind:something="
-      value
-        | thisIsARealSuperLongFilterPipe('arg1', arg2)
-        | anotherPipeLongJustForFun
-        | pipeTheThird
+      value |
+      thisIsARealSuperLongFilterPipe('arg1', arg2) |
+      anotherPipeLongJustForFun |
+      pipeTheThird
     "
   ></div>
   <div
     class="allowed"
     :class="
-      value
-        | thisIsARealSuperLongFilterPipe('arg1', arg2)
-        | anotherPipeLongJustForFun
-        | pipeTheThird
+      value |
+      thisIsARealSuperLongFilterPipe('arg1', arg2) |
+      anotherPipeLongJustForFun |
+      pipeTheThird
     "
   ></div>
   <div
     class="not-allowed"
     v-if="
       value |
-        thisIsARealSuperLongBitwiseOr('arg1', arg2) |
-        anotherBitwiseOrLongJustForFun |
-        bitwiseOrTheThird
+      thisIsARealSuperLongBitwiseOr('arg1', arg2) |
+      anotherBitwiseOrLongJustForFun |
+      bitwiseOrTheThird
     "
   ></div>
 </template>

`````

### Actual (oxfmt)

`````vue
<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div class="allowed">
    {{
      value |
      thisIsARealSuperLongFilterPipe('arg1', arg2) |
      anotherPipeLongJustForFun |
      pipeTheThird
    }}
  </div>
  <div
    class="allowed"
    v-bind:something="
      value |
      thisIsARealSuperLongFilterPipe('arg1', arg2) |
      anotherPipeLongJustForFun |
      pipeTheThird
    "
  ></div>
  <div
    class="allowed"
    :class="
      value |
      thisIsARealSuperLongFilterPipe('arg1', arg2) |
      anotherPipeLongJustForFun |
      pipeTheThird
    "
  ></div>
  <div
    class="not-allowed"
    v-if="
      value |
      thisIsARealSuperLongBitwiseOr('arg1', arg2) |
      anotherBitwiseOrLongJustForFun |
      bitwiseOrTheThird
    "
  ></div>
</template>

`````

### Expected (prettier)

`````vue
<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div class="allowed">
    {{
      value
        | thisIsARealSuperLongFilterPipe('arg1', arg2)
        | anotherPipeLongJustForFun
        | pipeTheThird
    }}
  </div>
  <div
    class="allowed"
    v-bind:something="
      value
        | thisIsARealSuperLongFilterPipe('arg1', arg2)
        | anotherPipeLongJustForFun
        | pipeTheThird
    "
  ></div>
  <div
    class="allowed"
    :class="
      value
        | thisIsARealSuperLongFilterPipe('arg1', arg2)
        | anotherPipeLongJustForFun
        | pipeTheThird
    "
  ></div>
  <div
    class="not-allowed"
    v-if="
      value |
        thisIsARealSuperLongBitwiseOr('arg1', arg2) |
        anotherBitwiseOrLongJustForFun |
        bitwiseOrTheThird
    "
  ></div>
</template>

`````
