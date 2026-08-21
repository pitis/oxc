import { describe, expect, it } from "vitest";
import { format } from "../../dist/index.js";

describe("Format non-js", () => {
  it("should format json with options", async () => {
    const jsoncCode = `
{
  // Package name
  "foo": "my",
  // Trailing comma test
  "bar": "1",
}
`.trim();
    const result = await format("foo.jsonc", jsoncCode, {
      insertFinalNewline: false,
    });
    expect(result.code).toBe(`${jsoncCode}`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should format vue with options", async () => {
    const vueCode = `
<template><div>Vue</div></template>
<style>div{color:red;}</style>
`.trim();
    const result = await format("Component.vue", vueCode, {
      vueIndentScriptAndStyle: true,
    });
    expect(result.code).toBe(
      `
<template><div>Vue</div></template>
<style>
  div {
    color: red;
  }
</style>
`.trimStart(),
    );
    expect(result.errors).toStrictEqual([]);
  });

  it("should surface Prettier parse errors as-is", async () => {
    // `.html` is still Prettier's; `.vue` went native and has its own message,
    // covered by the test below.
    const brokenHtml = `<div><span></div>`;
    const result = await format("broken.html", brokenHtml, {});

    expect(result.code).toBe(brokenHtml);
    expect(result.errors[0]?.message).toMatch(/Unexpected closing tag/);
  });

  it("should refuse a .vue file with an element that is never closed", async () => {
    // Printing this would mean writing the `</div>` for the author, at
    // whatever nesting the parser's recovery happened to pick. Both printers
    // refuse it; only the message differs, so the assertion is on the
    // contract rather than the wording.
    const brokenVue = `<template><div></template>`;
    const result = await format("broken.vue", brokenVue, {});

    expect(result.code).toBe(brokenVue);
    expect(result.errors).toHaveLength(1);
  });

  it("should format a .vue file whose end tags HTML makes optional", async () => {
    const result = await format("list.vue", `<template><ul><li>a<li>b</ul></template>`, {});

    expect(result.code).toBe(
      `<template>\n  <ul>\n    <li>a</li>\n    <li>b</li>\n  </ul>\n</template>\n`,
    );
    expect(result.errors).toStrictEqual([]);
  });

  it("should warn when a template expression cannot be parsed", async () => {
    const result = await format(
      "broken-expression.vue",
      `<template><div v-if="a &&& b" /></template>`,
      {},
    );

    // The fragment is kept exactly as written rather than guessed at, and the
    // user is told, which is the whole point of the warning channel.
    expect(result.code).toContain(`v-if="a &&& b"`);
    expect(result.errors).toStrictEqual([]);
    expect(result.warnings[0]?.message).toMatch(/expression-attribute fragment left unformatted/);
  });
});

describe("Format empty", () => {
  it("should format empty string", async () => {
    let result = await format("empty.js", "", {});
    expect(result.code).toBe("");
    expect(result.errors).toStrictEqual([]);

    result = await format("empty.toml", "  ", {});
    expect(result.code).toBe("");
    expect(result.errors).toStrictEqual([]);

    result = await format("empty.json", "\n\n", {});
    expect(result.code).toBe("");
    expect(result.errors).toStrictEqual([]);

    result = await format("empty.md", " \n ", {});
    expect(result.code).toBe("");
    expect(result.errors).toStrictEqual([]);
  });
});
