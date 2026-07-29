/**
 * Collector for non-fatal problems hit while formatting embedded JS/TS.
 *
 * Prettier's `printEmbeddedLanguage()` swallows everything `textToDoc()` throws
 * in production (`main/multiparser.js`): the offending `<script>` / expression is
 * emitted verbatim and the format still "succeeds". Without a side channel those
 * failures are invisible — no exit-code change, no `errors` entry, only a
 * `tracing` log behind `OXC_LOG`.
 *
 * So `prettier-plugin-oxfmt` reports them here, `formatFile()` drains them, and
 * they ride the existing `formatFile` result payload (`{ ok, code, warnings }`)
 * back to Rust, which surfaces them on the format result.
 *
 * `AsyncLocalStorage` scopes a collector to one `formatFile()` call, so
 * concurrent `format()` calls in the same process never mix warnings.
 */

import { AsyncLocalStorage } from "node:async_hooks";

/** Distinguishes the user's broken input from an oxfmt bug. */
export type EmbeddedFailureKind = "syntax" | "internal";

type Collector = {
  /** Warning message keyed by the embedded source text that produced it. */
  syntaxFailures: Map<string, string>;
  /** Embedded source texts that some later `textToDoc()` attempt did format. */
  formatted: Set<string>;
  /** Internal failures are oxfmt bugs; they are always reported. */
  internalFailures: string[];
};

const STORAGE = new AsyncLocalStorage<Collector>();

/**
 * Run `format` with a fresh collector and return the collected warnings.
 *
 * A syntax failure is dropped when the same source text was formatted
 * successfully by a later attempt. Prettier's `v-on` printer relies on exactly
 * that: `@click="a++; b++"` is first tried as an expression (which legitimately
 * fails to parse) and then re-formatted as statements. Only failures that no
 * attempt recovered from are real "left unformatted" cases.
 */
export async function withEmbeddedWarnings<T>(
  format: () => Promise<T>,
): Promise<{ value: T; warnings: string[] }> {
  const collector: Collector = {
    syntaxFailures: new Map(),
    formatted: new Set(),
    internalFailures: [],
  };
  const value = await STORAGE.run(collector, format);

  const warnings = [...collector.internalFailures];
  for (const [sourceText, message] of collector.syntaxFailures) {
    if (!collector.formatted.has(sourceText)) warnings.push(message);
  }
  return { value, warnings };
}

/**
 * Record that oxfmt could not format an embedded fragment.
 *
 * No-op outside a {@linkcode withEmbeddedWarnings} scope (e.g. when Prettier is
 * driven directly in a test), so reporting is always safe to call.
 */
export function reportEmbeddedFailure(
  kind: EmbeddedFailureKind,
  sourceText: string,
  parentContext: string,
  detail: string,
): void {
  const collector = STORAGE.getStore();
  if (collector === undefined) return;

  const where = `${parentContext} fragment left unformatted`;
  if (kind === "internal") {
    collector.internalFailures.push(
      `internal error: ${detail} (${where}: \`${toSnippet(sourceText)}\`)`,
    );
    return;
  }
  // Keyed by source text so the two `tsx`/`ts` attempts over one fragment,
  // or the same broken expression repeated in a file, warn only once.
  if (!collector.syntaxFailures.has(sourceText)) {
    collector.syntaxFailures.set(
      sourceText,
      `syntax error in embedded script: ${detail} (${where}: \`${toSnippet(sourceText)}\`)`,
    );
  }
}

/** Record that an embedded fragment did format, cancelling any earlier syntax failure for it. */
export function reportEmbeddedSuccess(sourceText: string): void {
  STORAGE.getStore()?.formatted.add(sourceText);
}

const SNIPPET_MAX_LENGTH = 60;

/** Single-line, length-capped excerpt of the fragment, since there is no span to point at. */
function toSnippet(sourceText: string): string {
  const singleLine = sourceText.trim().replace(/\s+/gu, " ");
  return singleLine.length > SNIPPET_MAX_LENGTH
    ? `${singleLine.slice(0, SNIPPET_MAX_LENGTH)}...`
    : singleLine;
}
