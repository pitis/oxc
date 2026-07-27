# externals/prettier/vue/ts-expression/filter.vue

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
@@ -4,39 +4,39 @@
 <template>
   <div>
     <div class="allowed">
       {{
-        value
-          | thisIsARealSuperLongFilterPipe("arg1", arg2 as unknown)
-          | anotherPipeLongJustForFun
-          | pipeTheThird
+        value |
+        thisIsARealSuperLongFilterPipe("arg1", arg2 as unknown) |
+        anotherPipeLongJustForFun |
+        pipeTheThird
       }}
     </div>
     <div
       class="allowed"
       v-bind:something="
-        value
-          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
-          | anotherPipeLongJustForFun
-          | pipeTheThird
+        value |
+        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
+        anotherPipeLongJustForFun |
+        pipeTheThird
       "
     ></div>
     <div
       class="allowed"
       :class="
-        value
-          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
-          | anotherPipeLongJustForFun
-          | pipeTheThird
+        value |
+        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
+        anotherPipeLongJustForFun |
+        pipeTheThird
       "
     ></div>
     <div
       class="not-allowed"
       v-if="
         value |
-          thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
-          anotherBitwiseOrLongJustForFun |
-          bitwiseOrTheThird
+        thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
+        anotherBitwiseOrLongJustForFun |
+        bitwiseOrTheThird
       "
     ></div>
   </div>
 </template>

`````

### Actual (oxfmt)

`````vue
<script setup lang="ts"></script>

<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div>
    <div class="allowed">
      {{
        value |
        thisIsARealSuperLongFilterPipe("arg1", arg2 as unknown) |
        anotherPipeLongJustForFun |
        pipeTheThird
      }}
    </div>
    <div
      class="allowed"
      v-bind:something="
        value |
        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
        anotherPipeLongJustForFun |
        pipeTheThird
      "
    ></div>
    <div
      class="allowed"
      :class="
        value |
        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
        anotherPipeLongJustForFun |
        pipeTheThird
      "
    ></div>
    <div
      class="not-allowed"
      v-if="
        value |
        thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
        anotherBitwiseOrLongJustForFun |
        bitwiseOrTheThird
      "
    ></div>
  </div>
</template>

`````

### Expected (prettier)

`````vue
<script setup lang="ts"></script>

<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div>
    <div class="allowed">
      {{
        value
          | thisIsARealSuperLongFilterPipe("arg1", arg2 as unknown)
          | anotherPipeLongJustForFun
          | pipeTheThird
      }}
    </div>
    <div
      class="allowed"
      v-bind:something="
        value
          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
          | anotherPipeLongJustForFun
          | pipeTheThird
      "
    ></div>
    <div
      class="allowed"
      :class="
        value
          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
          | anotherPipeLongJustForFun
          | pipeTheThird
      "
    ></div>
    <div
      class="not-allowed"
      v-if="
        value |
          thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
          anotherBitwiseOrLongJustForFun |
          bitwiseOrTheThird
      "
    ></div>
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
@@ -4,39 +4,39 @@
 <template>
   <div>
     <div class="allowed">
       {{
-        value
-          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
-          | anotherPipeLongJustForFun
-          | pipeTheThird
+        value |
+        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
+        anotherPipeLongJustForFun |
+        pipeTheThird
       }}
     </div>
     <div
       class="allowed"
       v-bind:something="
-        value
-          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
-          | anotherPipeLongJustForFun
-          | pipeTheThird
+        value |
+        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
+        anotherPipeLongJustForFun |
+        pipeTheThird
       "
     ></div>
     <div
       class="allowed"
       :class="
-        value
-          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
-          | anotherPipeLongJustForFun
-          | pipeTheThird
+        value |
+        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
+        anotherPipeLongJustForFun |
+        pipeTheThird
       "
     ></div>
     <div
       class="not-allowed"
       v-if="
         value |
-          thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
-          anotherBitwiseOrLongJustForFun |
-          bitwiseOrTheThird
+        thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
+        anotherBitwiseOrLongJustForFun |
+        bitwiseOrTheThird
       "
     ></div>
   </div>
 </template>

`````

### Actual (oxfmt)

`````vue
<script setup lang="ts"></script>

<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div>
    <div class="allowed">
      {{
        value |
        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
        anotherPipeLongJustForFun |
        pipeTheThird
      }}
    </div>
    <div
      class="allowed"
      v-bind:something="
        value |
        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
        anotherPipeLongJustForFun |
        pipeTheThird
      "
    ></div>
    <div
      class="allowed"
      :class="
        value |
        thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown) |
        anotherPipeLongJustForFun |
        pipeTheThird
      "
    ></div>
    <div
      class="not-allowed"
      v-if="
        value |
        thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
        anotherBitwiseOrLongJustForFun |
        bitwiseOrTheThird
      "
    ></div>
  </div>
</template>

`````

### Expected (prettier)

`````vue
<script setup lang="ts"></script>

<!-- vue filters are only allowed in v-bind and interpolation -->
<template>
  <div>
    <div class="allowed">
      {{
        value
          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
          | anotherPipeLongJustForFun
          | pipeTheThird
      }}
    </div>
    <div
      class="allowed"
      v-bind:something="
        value
          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
          | anotherPipeLongJustForFun
          | pipeTheThird
      "
    ></div>
    <div
      class="allowed"
      :class="
        value
          | thisIsARealSuperLongFilterPipe('arg1', arg2 as unknown)
          | anotherPipeLongJustForFun
          | pipeTheThird
      "
    ></div>
    <div
      class="not-allowed"
      v-if="
        value |
          thisIsARealSuperLongBitwiseOr('arg1', arg2 as unknown) |
          anotherBitwiseOrLongJustForFun |
          bitwiseOrTheThird
      "
    ></div>
  </div>
</template>

`````
