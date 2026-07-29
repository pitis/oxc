<template>
  <button @click="count++">expr</button>
  <button @click="doSomething( a, b )">call</button>
  <button @click="open = true">assign</button>
  <button @click="a++; b++">statements</button>
  <button @click="foo;">semi-kept</button>
  <button @click="foo">method-ref</button>
  <button @click="(e)=>handle(e,'x')">arrow</button>
  <button @click="veryLongFunctionName(firstArgument, secondArgument, thirdArgument, fourthOne)">breaks</button>
  <button v-on:click="toggle()">longform</button>
</template>
