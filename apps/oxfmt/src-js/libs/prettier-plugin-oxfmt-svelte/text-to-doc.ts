import { jsTextToDoc } from "../../index";
import { reportEmbeddedFailure, reportEmbeddedSuccess } from "../embedded-warnings";
import type { Parser, Doc } from "prettier";

/**
 * Format an embedded Svelte component with `oxc_formatter_svelte` and return
 * its Prettier `Doc`.
 *
 * The Rust entry is shared with the JS/TS one: `source_ext` selects the
 * formatter, and `"svelte"` means "a whole component" rather than a parse
 * grammar. The payload contract and the `{ doc, refs }` reply are the same,
 * so the ref-resolving walk lives in one place — see
 * `prettier-plugin-oxfmt/text-to-doc.ts`, whose helpers this reuses.
 */
export const svelteTextToDoc: Parser<Doc>["parse"] = async (
  embeddedSourceText,
  textToDocOptions,
) => {
  const { parentParser, _oxfmtSveltePluginOptionsJson } = textToDocOptions;

  let docJSON: string | null = null;
  try {
    docJSON = await jsTextToDoc(
      "svelte",
      embeddedSourceText,
      _oxfmtSveltePluginOptionsJson as string,
      // The host parser name, as every full-document embed reports it.
      (parentParser as string) ?? "",
    );
  } catch (error) {
    docJSON = null;
    reportEmbeddedFailure("internal", embeddedSourceText, "svelte", toDetail(error));
    throw new Error(`\`oxfmt::svelteTextToDoc()\` failed (internal): ${toDetail(error)}`);
  }

  const payload = docJSON === null ? null : (JSON.parse(docJSON) as SvelteDocPayload);
  if (payload === null || payload.error) {
    const kind = payload?.error?.kind === "internal" ? "internal" : "syntax";
    const detail = payload?.error?.message || "no details available";
    reportEmbeddedFailure(kind, embeddedSourceText, "svelte", detail);
    // Prettier swallows this and emits the block verbatim, which is what a
    // component the Svelte compiler would reject should get.
    throw new Error(`\`oxfmt::svelteTextToDoc()\` failed (${kind}): ${detail}`);
  }

  reportEmbeddedSuccess(embeddedSourceText);

  const { doc, refs, hasRootDedent } = payload;
  if (refs.length === 0) {
    if (hasRootDedent) restoreRootDedents(doc);
    return doc as Doc;
  }
  const cache: unknown[] = Array.from({ length: refs.length });
  return resolveRefs(doc, refs, cache) as Doc;
};

type SvelteDocPayload = {
  doc: unknown;
  refs: unknown[];
  hasRootDedent: boolean;
  error?: { kind?: string; message?: string };
};

/** Best-effort message for a thrown value crossing the NAPI boundary. */
function toDetail(error: unknown): string {
  if (error instanceof Error) return error.message || String(error);
  return String(error);
}

// The two helpers below are the same transport fix-ups the JS/TS plugin does;
// see `prettier-plugin-oxfmt/text-to-doc.ts` for why each is needed.

function restoreRootDedents(node: unknown): void {
  if (node === null || typeof node !== "object") return;
  if (Array.isArray(node)) {
    for (const child of node) restoreRootDedents(child);
    return;
  }
  const obj = node as Record<string, unknown>;
  if (obj.type === "align" && obj.n === null) obj.n = Number.NEGATIVE_INFINITY;
  for (const k in obj) restoreRootDedents(obj[k]);
}

function resolveRefs(node: unknown, rawRefs: unknown[], cache: unknown[]): unknown {
  if (node === null || typeof node !== "object") return node;
  if (Array.isArray(node)) return node.map((n) => resolveRefs(n, rawRefs, cache));

  const obj = node as Record<string, unknown>;
  if (typeof obj._REF === "number") {
    const id = obj._REF;
    const cached = cache[id];
    if (cached !== undefined) return cached;
    const resolved = resolveRefs(rawRefs[id], rawRefs, cache);
    cache[id] = resolved;
    return resolved;
  }

  const out: Record<string, unknown> = {};
  for (const k in obj) out[k] = resolveRefs(obj[k], rawRefs, cache);
  if (out.type === "align" && out.n === null) out.n = Number.NEGATIVE_INFINITY;
  return out;
}
