import { describe, expect, it } from "vitest";
import { format } from "../../dist/index.js";

// NOTE: For now, Vue files are partially handled by Prettier

describe("Format js-in-vue with prettier-plugin-oxfmt", () => {
  it("should format .vue w/ sort-imports", async () => {
    const input = `
<script lang="ts">
import z from "z";
  import a from "a";
    import m from "m";

</script>
<script lang="ts" setup>
import z from "z";
  import a from "a";
    import m from "m";

</script>
<template> <div>{{a+m+z}}</div> </template>
`;
    const result = await format("a.vue", input, {
      vueIndentScriptAndStyle: true,
      experimentalSortImports: {},
    });

    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
  });

  it("should format .vue w/ sort-tailwindcss", async () => {
    const input = `
<script setup>
import { ref } from "vue";
import clsx from "clsx";

const count = ref(0);
const cls = clsx("p-4 flex");
</script>
<template>
  <div class="flex p-4">{{count}}</div>
  <div class="p-4 flex">{{count}}</div>
</template>
`;
    const result = await format("a.vue", input, {
      vueIndentScriptAndStyle: true,
      experimentalSortImports: {},
      experimentalTailwindcss: { functions: ["clsx"] },
    });

    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
  });

  // https://github.com/oxc-project/oxc/issues/20084
  it("should format .vue w/ template literal idempotently (vueIndentScriptAndStyle)", async () => {
    const input = `
<script setup>
const a = \`
  hello
  world
\`;
</script>
<template>
  <div>{{ a }}</div>
</template>
`;
    const result = await format("a.vue", input, {
      vueIndentScriptAndStyle: true,
    });

    // Format again to verify idempotency
    const result2 = await format("a.vue", result.code, {
      vueIndentScriptAndStyle: true,
    });

    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
    expect(result2.code).toBe(result.code);
    expect(result2.errors).toStrictEqual([]);
  });

  it("should format .vue w/ template literal (no vueIndentScriptAndStyle)", async () => {
    const input = `
<script setup>
const a = \`
  hello
  world
\`;
</script>
<template>
  <div>{{ a }}</div>
</template>
`;
    const result = await format("a.vue", input);

    // Format again to verify idempotency
    const result2 = await format("a.vue", result.code);

    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
    expect(result2.code).toBe(result.code);
    expect(result2.errors).toStrictEqual([]);
  });

  it("should format template expressions (directives, v-bind, v-for RHS)", async () => {
    const input = `
<template>
  <div v-if="someCondition&&otherCondition">x</div>
  <div v-show="!visible">y</div>
  <li v-for="(item,idx) in items.filter(i=>i>0)">{{item}}</li>
  <div :class="{active:isActive,'text-danger':hasError}">z</div>
  <div :class="['a','b',{c:d}]">arr</div>
  <input :value="\`hello \${name}\`" />
</template>
`;
    const result = await format("a.vue", input);

    // Format again to verify idempotency
    const result2 = await format("a.vue", result.code);

    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
    expect(result2.code).toBe(result.code);
  });

  it("should format interpolations, keeping the configured quote style", async () => {
    const input = `
<template>
  <span>{{  greeting  ?  'hello'  :  "goodbye"  }}</span>
</template>
`;
    const result = await format("a.vue", input);

    // Interpolations are NOT inside an attribute: double quotes stay.
    expect(result.code).toContain(`{{ greeting ? "hello" : "goodbye" }}`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should force single quotes for strings inside attribute expressions", async () => {
    // The string contains a single quote: without forcing, the quote-swap
    // heuristic would pick double quotes, which the host entity-escapes
    // into `&quot;`. Prettier forces single quotes (`__isInHtmlAttribute`).
    const input = `
<template>
  <div :class="cn('[&_svg:not([class*=\\'size-\\'])]:size-3')">x</div>
</template>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain(`cn('[&_svg:not([class*=\\'size-\\'])]:size-3')`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should format v-on values, preserving inline-handler semicolon semantics", async () => {
    // `@click="foo"` (method handler) and `@click="foo;"` (inline statement)
    // compile differently in Vue; the trailing semicolon must survive.
    const input = `
<template>
  <button @click="count++">a</button>
  <button @click="doSomething( a, b )">b</button>
  <button @click="a++; b++">c</button>
  <button @click="foo;">d</button>
  <button @click="foo">e</button>
  <button @click="(e)=>handle(e,'x')">f</button>
</template>
`;
    const result = await format("a.vue", input);

    // Format again to verify idempotency
    const result2 = await format("a.vue", result.code);

    expect(result.code).toContain(`@click="foo;"`);
    expect(result.code).toContain(`@click="foo"`);
    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
    expect(result2.code).toBe(result.code);
  });

  it("should leave unparsable template expressions untouched", async () => {
    const input = `
<template>
  <div v-if="a &&& b">x</div>
</template>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain(`v-if="a &&& b"`);
    expect(result.errors).toStrictEqual([]);
  });

  // gql-in-js-in-vue: the `oxc_formatter_graphql` IR's blank runs
  // (`exact_line_breaks`, part of the block string's VALUE) must survive the IR→Doc conversion
  // back to the Prettier host (encoded as that many hardlines, which Prettier never collapses).
  it("should preserve gql block-string blank lines through a .vue script", async () => {
    const input = `
<script setup>
const q = graphql\`
  """
  First paragraph.


  Second paragraph after two blanks.
  """
  type Query {
    hello: String
  }
\`;
</script>
`;
    const result = await format("a.vue", input);

    // Format again to verify idempotency
    const result2 = await format("a.vue", result.code);

    expect(result.code).toContain("First paragraph.\n\n\n  Second paragraph after two blanks.");
    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
    expect(result2.code).toBe(result.code);
    expect(result2.errors).toStrictEqual([]);
  });
});
