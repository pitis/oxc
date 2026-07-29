<script setup lang="ts">
import { ref } from "vue";
const items = ref([1, 2, 3]);
</script>

<template>
  <div v-if="someCondition&&otherCondition||yetAnotherLongCondition&&oneMoreToForceBreaking">x</div>
  <div v-show="!visible">y</div>
  <li v-for="(item,idx) in items.filter(i=>i>0)">{{item.name+suffix}}</li>
  <span>{{  greeting  ?  'hello'  :  "goodbye"  }}</span>
  <div :class="{active:isActive,'text-danger':hasError}">z</div>
  <div :class="['a','b',{c:d}]">arr</div>
  <div :class="cn('[&_svg:not([class*=\'size-\'])]:size-3', props.class)">quotes</div>
  <input :value="`hello ${name}`" />
  <input :placeholder="'literal'" />
  <Comp :model-value="someVeryLongExpression && anotherVeryLongExpression && yetAnotherOneThatBreaks" />
</template>
