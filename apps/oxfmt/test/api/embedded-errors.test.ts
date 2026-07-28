import { beforeEach, describe, expect, it, vi } from "vitest";
import { jsTextToDoc } from "../../dist/index.js";

type NapiImpl = () => Promise<string | null>;

// The plugin's `textToDoc` is not part of the public bundle surface, so the
// TS-level precedence logic is exercised against the source module with the
// NAPI call mocked. `dist/` is used for everything that must go through Rust.
//
// A plain mutable holder rather than `vi.fn()`: the mock's own result tracking
// turns a rejected call into an unhandled rejection even though `textToDoc`
// catches it, and the rejection path is exactly what needs testing here.
const napi = { impl: (async () => null) as NapiImpl, calls: 0 };
vi.mock("../../src-js/index", () => ({
  jsTextToDoc: () => {
    napi.calls += 1;
    return napi.impl();
  },
}));

const { textToDoc } = await import("../../src-js/libs/prettier-plugin-oxfmt/text-to-doc");
const { withEmbeddedWarnings } = await import("../../src-js/libs/embedded-warnings");

/** `__ts_expression` + a non-JS/TS filepath means two attempts (`ts`, then `tsx`). */
const TWO_ATTEMPT_OPTIONS = {
  parser: "__ts_expression",
  parentParser: "vue",
  filepath: "a.vue",
  _oxfmtPluginOptionsJson: JSON.stringify({ config: {}, filepath: "a.vue" }),
  __isInHtmlAttribute: true,
};

const syntaxPayload = (message: string) =>
  JSON.stringify({ error: { kind: "syntax", message }, parseError: true });
const internalPayload = (message: string) =>
  JSON.stringify({ error: { kind: "internal", message } });
const successPayload = () => JSON.stringify({ doc: "ok", refs: [] });

async function runTextToDoc(sourceText: string, options = TWO_ATTEMPT_OPTIONS) {
  return withEmbeddedWarnings(async () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    await expect(textToDoc(sourceText, options as any)).rejects.toThrow();
  });
}

describe("Rust-side embedded failure classification", () => {
  const payload = JSON.stringify({ config: {}, filepath: "a.vue" });

  it("labels an unparsable expression as a syntax failure and keeps the `v-on` marker", async () => {
    const result = await jsTextToDoc("jsx", "a &&& b", payload, "expression-attribute");

    expect(JSON.parse(result!)).toStrictEqual({
      error: { kind: "syntax", message: expect.any(String) },
      parseError: true,
    });
  });

  it("labels a malformed plugin payload as an internal failure", async () => {
    const result = await jsTextToDoc("jsx", "a", "not-json", "vue-script");
    const parsed = JSON.parse(result!);

    expect(parsed.error.kind).toBe("internal");
    expect(parsed.error.message).toContain("`_oxfmtPluginOptionsJson` failed to deserialize");
    // An internal failure must never masquerade as a Babel syntax error.
    expect(parsed.parseError).toBeUndefined();
  });

  it("fails loudly on an unmapped pseudo-parser instead of formatting a full program", async () => {
    const result = await jsTextToDoc("jsx", "a", payload, "__nope_expression");
    const parsed = JSON.parse(result!);

    expect(parsed.error.kind).toBe("internal");
    expect(parsed.error.message).toContain("unmapped pseudo-parser context");
    expect(parsed.parseError).toBeUndefined();
  });
});

describe("textToDoc failure precedence", () => {
  beforeEach(() => {
    napi.impl = async () => null;
    napi.calls = 0;
  });

  it("reports a syntax failure and keeps the Babel-shaped cause for `v-on`", async () => {
    napi.impl = async () => syntaxPayload("Unexpected token");

    const { warnings } = await withEmbeddedWarnings(async () => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      await expect(textToDoc("a &&& b", TWO_ATTEMPT_OPTIONS as any)).rejects.toMatchObject({
        cause: { code: "BABEL_PARSER_SYNTAX_ERROR" },
      });
    });

    // Both attempts failed on the same text: one warning, not two.
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toMatch(/^syntax error in embedded script: Unexpected token/);
  });

  it("does not resurrect the first attempt's syntax error when the second fails internally", async () => {
    napi.impl = async () => (napi.calls === 1 ? syntaxPayload("Unexpected token") : null);

    const { warnings } = await runTextToDoc("a &&& b");

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toMatch(/^internal error:/);
    expect(warnings[0]).not.toContain("Unexpected token");
  });

  it("classifies an explicit internal payload as internal", async () => {
    napi.impl = async () => internalPayload("IR conversion exploded");

    const { warnings } = await runTextToDoc("a &&& b");

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toMatch(/^internal error: IR conversion exploded/);
  });

  it("classifies a rejected NAPI promise as internal", async () => {
    napi.impl = async () => {
      throw new Error("napi boom");
    };

    const { warnings } = await runTextToDoc("a &&& b");

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toMatch(/^internal error: napi boom/);
  });

  it("never throws a Babel-shaped error for an internal failure", async () => {
    napi.impl = async () => internalPayload("boom");

    await withEmbeddedWarnings(async () => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const thrown = await textToDoc("a &&& b", TWO_ATTEMPT_OPTIONS as any).catch(
        (error: Error) => error,
      );
      expect((thrown as { cause?: unknown }).cause).toBeUndefined();
      expect(thrown.message).toContain("(internal)");
    });
  });

  it("drops a syntax failure once another attempt formats the same text", async () => {
    // Mirrors Prettier's `v-on` fallback: the expression parse fails, then the
    // very same text is formatted as statements.
    napi.impl = async () => syntaxPayload("Unexpected token");

    const { warnings } = await withEmbeddedWarnings(async () => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      await expect(textToDoc("a++; b++", TWO_ATTEMPT_OPTIONS as any)).rejects.toThrow();
      napi.impl = async () => successPayload();
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      await textToDoc("a++; b++", { ...TWO_ATTEMPT_OPTIONS, parser: "__vue_ts_event_binding" } as any);
    });

    expect(warnings).toStrictEqual([]);
  });
});
