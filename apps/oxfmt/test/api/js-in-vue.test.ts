import { describe, expect, it } from "vitest";
import { format } from "../../dist/index.js";

// NOTE: For now, Vue files are partially handled by Prettier

describe("Format js-in-vue with prettier-plugin-oxfmt", () => {
  it("should not indent a comments-only script block", async () => {
    // A comments-only program used to go through the trailing-comments path,
    // whose separator emitted a leading space (` /**`). Oxc's own printer trims
    // it at the line start, but the Prettier doc bridge renders it verbatim.
    const input = `
<script lang="ts">
/**
 * Docs.
 */
</script>

<template>
  <div>x</div>
</template>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain('<script lang="ts">\n/**\n * Docs.\n */\n</script>');
    expect(result.errors).toStrictEqual([]);
  });

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

  it("should leave unparsable template expressions untouched, but report them", async () => {
    const input = `
<template>
  <div v-if="a &&& b">x</div>
</template>
`;
    const result = await format("a.vue", input);

    // Output is unchanged: Prettier emits the fragment verbatim (byte-parity),
    // and the file still counts as successfully formatted (`errors` stays empty).
    expect(result.code).toContain(`v-if="a &&& b"`);
    expect(result.errors).toStrictEqual([]);
    // ...but the failure is no longer silent.
    expect(result.warnings).toHaveLength(1);
    expect(result.warnings[0].message).toContain("syntax error in embedded script:");
    expect(result.warnings[0].message).toContain("a &&& b");
    expect(result.warnings[0].message).not.toContain("internal error:");
  });

  it("should report an unparsable <script> block while leaving it untouched", async () => {
    const input = `
<script lang="ts">
const a = ;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("const a = ;");
    expect(result.errors).toStrictEqual([]);
    expect(result.warnings).toHaveLength(1);
    expect(result.warnings[0].message).toContain("syntax error in embedded script:");
  });

  it("should not report v-on values that only fail the expression parse", async () => {
    // `@click="a++; b++"` legitimately fails the `__js_expression` attempt and
    // is re-formatted as statements. That recovery must stay silent.
    const input = `
<template>
  <button @click="a++; b++">c</button>
  <button @click="foo;">d</button>
</template>
`;
    const result = await format("a.vue", input);

    // Reformatted as statements by the `__vue_event_binding` fallback.
    expect(result.code).toContain("a++;\n      b++;");
    expect(result.errors).toStrictEqual([]);
    expect(result.warnings).toStrictEqual([]);
  });

  it("should print Vue 2 filter sequences with the leading-| layout", async () => {
    // Filters are only valid in v-bind and interpolations (`__vue_expression`);
    // a `|` in `v-if` (`__js_expression`) stays a plain bitwise-or chain.
    const input = `
<template>
  <div>{{ value | thisIsARealSuperLongFilterPipe("arg1", arg2) | anotherPipeLongJustForFun | pipeTheThird }}</div>
  <div :cls="value | shortPipe" v-if="value | thisIsARealSuperLongBitwiseOr('arg1', arg2) | anotherBitwiseOrLongJustForFun">x</div>
</template>
`;
    const result = await format("a.vue", input);

    // Broken filter chain: break BEFORE the `|`, indented under the head.
    expect(result.code).toContain(`| anotherPipeLongJustForFun`);
    expect(result.code).not.toContain(`thisIsARealSuperLongFilterPipe("arg1", arg2) |`);
    // Flat filter chain stays flat; v-if keeps the trailing-operator layout.
    expect(result.code).toContain(`:cls="value | shortPipe"`);
    expect(result.code).toContain(`thisIsARealSuperLongBitwiseOr('arg1', arg2) |`);
    expect(result.code).toMatchSnapshot();
    expect(result.errors).toStrictEqual([]);
  });

  it('should format <script lang="tsx"> blocks', async () => {
    const input = `
<script lang="tsx">
export default {
  render(h): VNode {  return <div>{ this.foo }</div> },
}
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain(`return <div>{this.foo}</div>;`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should drop the trailing comma of a single generic param in ts .vue scripts", async () => {
    // A `lang="ts"` block formats like plain `.ts`, where the lone generic
    // comma is not required, so it is removed. Intentional divergence from
    // Prettier (which keeps it in ts-in-vue); the comma is only required for
    // `.tsx` and `.mts|cts` (see the conformance note on api-component.vue).
    const input = `
<script setup lang="ts">
const getOptions = <T = any,>(list: T[]) => list;
const constrained = <T extends object>(list: T[]) => list;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain(`<T = any>(list: T[]) => list`);
    expect(result.code).toContain(`<T extends object>(list: T[]) => list`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should parenthesize objects touching `}}` when bracketSpacing is off", async () => {
    const input = `
<template>
  <div>{{ { a: { b: 1 } } }}</div>
  <div :x="{ a: { b: 1 } }">y</div>
</template>
`;
    const result = await format("a.vue", input, { bracketSpacing: false });

    // Interpolation: the inner object would create a premature \`}}\`.
    expect(result.code).toContain(`{{ {a: ({b: 1})} }}`);
    // Attribute values have no \`}}\` scanner problem: no parens.
    expect(result.code).toContain(`:x="{a: {b: 1}}"`);
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

  it('should format <script lang="tsx"> blocks', async () => {
    const input = `
<script lang="tsx">
export default {
  render( h ): VNode {return <div>{ this.foo   }</div>    },
}
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("return <div>{this.foo}</div>;");
    expect(result.errors).toStrictEqual([]);
  });

  it('should format generic arrows in <script lang="ts"> blocks', async () => {
    const input = `
<script lang="ts">
export const identity=<T>(x:T):T=>x;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("export const identity = <T>(x: T): T => x;");
    expect(result.errors).toStrictEqual([]);
  });

  // NOTE: Trailing comma of a lone generic arrow param is grammar-keyed, not path-keyed:
  // plain-TS blocks behave like plain .ts files (comma removable).
  // Unlike Prettier, which keeps the comma for any non-.ts `opts.filepath`. (ts-in-vue)
  it('should drop the lone generic param comma in <script lang="ts"> blocks', async () => {
    const input = `
<script setup lang="ts">
const getOptions = <T = any,>(list: T[]) => list;
const identity = <T,>(x: T) => x;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("const getOptions = <T = any>(list: T[]) => list;");
    expect(result.code).toContain("const identity = <T>(x: T) => x;");
    expect(result.errors).toStrictEqual([]);
  });

  it('should keep the lone generic param comma in <script lang="tsx"> blocks', async () => {
    const input = `
<script lang="tsx">
const identity = <T,>(x: T) => x;
const el = <div>hi</div>;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("const identity = <T,>(x: T) => x;");
    expect(result.errors).toStrictEqual([]);
  });

  it('should keep the comma in a JSX-free lang="tsx" block', async () => {
    const input = `
<script setup lang="tsx">
const getOptions = <T = any,>(list: T[]) => list;
const identity = <T,>(x: T) => x;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("const getOptions = <T = any,>(list: T[]) => list;");
    expect(result.code).toContain("const identity = <T,>(x: T) => x;");
    expect(result.errors).toStrictEqual([]);
  });

  it('should detect lang="tsx" after a generic attribute containing `>`', async () => {
    const input = `
<script setup generic="T extends Record<string, string>" lang="tsx">
const pick = <U = T,>(x: U) => x;
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toContain("const pick = <U = T,>(x: U) => x;");
    expect(result.errors).toStrictEqual([]);
  });

  // https://github.com/oxc-project/oxc/issues/25568
  it("should not add a blank line after a dangling comment in an empty object", async () => {
    // The IR's hardline (comment terminator) + softline (before `}`) must print as a single break,
    // like the Rust printer's newline suppression at a line start.
    const input = `
<script setup>
const a = {
  // x
}
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toBe(`<script setup>
const a = {
  // x
};
</script>
`);
    expect(result.errors).toStrictEqual([]);
  });

  // https://github.com/oxc-project/oxc/issues/25569
  it("should dedent template literal interpolation to root inside a function", async () => {
    // The IR's dedent-to-root must survive the Doc conversion
    // (JSON cannot represent the `-Infinity` Prettier expects; it is restored JS-side).
    const input = `
<script setup>
const f = () => {
  s.value = \`
\${items ? Object.entries(items).map(([k, v]) => k + v).join("") : ""}
\`
}
</script>
`;
    const result = await format("a.vue", input, { printWidth: 80 });

    // Format again to verify idempotency
    const result2 = await format("a.vue", result.code, { printWidth: 80 });

    expect(result.code).toBe(`<script setup>
const f = () => {
  s.value = \`
\${
  items
    ? Object.entries(items)
        .map(([k, v]) => k + v)
        .join("")
    : ""
}
\`;
};
</script>
`);
    expect(result.errors).toStrictEqual([]);
    expect(result2.code).toBe(result.code);
    expect(result2.errors).toStrictEqual([]);
  });

  it("should not indent a comment-only script block", async () => {
    // The comment's leading IR `Space` must be dropped at the line start,
    // like the Rust printer does, or `/**` gains a spurious leading space.
    const input = `
<script lang="ts">
/**
 * Docs.
 */
</script>
`;
    const result = await format("a.vue", input);

    expect(result.code).toBe(`<script lang="ts">
/**
 * Docs.
 */
</script>
`);
    expect(result.errors).toStrictEqual([]);
  });
});
