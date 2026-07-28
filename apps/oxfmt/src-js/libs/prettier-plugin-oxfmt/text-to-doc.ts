import { jsTextToDoc } from "../../index";
import {
  reportEmbeddedFailure,
  reportEmbeddedSuccess,
  type EmbeddedFailureKind,
} from "../embedded-warnings";
import type { Parser, Doc } from "prettier";

/// Prettier's "known file type" check — when the filepath has one of these
/// extensions, jsx-ness comes from the extension and no sniffing happens
/// (`isKnownFileType` in language-js/parse/typescript.js).
const KNOWN_JS_TS_FILE_RE = /\.(?:js|mjs|cjs|jsx|ts|mts|cts|tsx)$/i;

/// Prettier's naive JSX sniff, verbatim (language-js/parse/utilities/
/// jsx-regexp.evaluate.js): "</" when probably not in a string, or "/>" on a
/// line not starting with "//". Decides only the ATTEMPT ORDER, not the result.
const JSX_LIKELY_RE = new RegExp(
  [
    "^[^\"'`]*</", // Contains "</" when probably not in a string
    "|",
    "^[^/]{2}.*/>", // Contains "/>" on line not starting with "//"
  ].join(""),
  "m",
);

export const textToDoc: Parser<Doc>["parse"] = async (embeddedSourceText, textToDocOptions) => {
  // `_oxfmtPluginOptionsJson` is a JSON string bundled by Rust (`oxfmtrc::inject_oxfmt_plugin_payload`),
  // carrying the typed `FormatConfig` + parent filepath for the Rust-side `oxc_formatter`.
  const { parser, parentParser, filepath, _oxfmtPluginOptionsJson } = textToDocOptions;

  // For (j|t)s-in-xxx, default `parser` is either `babel`, `babel-ts` or `typescript`
  // We need to infer `SourceType::from_extension(ext)` for `oxc_formatter`.
  // - JS: always enable JSX for js-in-xxx, it's safe
  // - TS: `typescript` (ts-in-vue|markdown|mdx) or `babel-ts` (ts-in-vue(script generic="..."))
  //   - In case of ts-in-md, `filepath` is overridden as `dummy.tx(x)` to distinguish TSX or TS
  //   - For tsx-in-vue (`<script lang="tsx">`) there is no parser-name signal.
  //     Prettier's `typescript`/`babel-ts` parsers try BOTH jsx and non-jsx
  //     parses when the filepath is not a known JS/TS extension, ordered by a
  //     naive JSX sniff (`getParseOptionsCombinations` + `jsxRegexp` in
  //     language-js/parse/typescript.js). Mirror that with a tsx/ts fallback.
  const isTS =
    parser === "typescript" ||
    parser === "babel-ts" ||
    parser === "__ts_expression" ||
    parser === "__vue_ts_expression" ||
    parser === "__vue_ts_event_binding";
  let extAttempts: string[];
  if (!isTS) {
    extAttempts = ["jsx"];
  } else if (filepath && KNOWN_JS_TS_FILE_RE.test(filepath)) {
    extAttempts = [/\.tsx$/i.test(filepath) ? "tsx" : "ts"];
  } else {
    extAttempts = JSX_LIKELY_RE.test(embeddedSourceText) ? ["tsx", "ts"] : ["ts", "tsx"];
  }

  // Detect context from the (pseudo-)parser name and Prettier's internal flags
  const parentContext = detectParentContext(parser as string, parentParser!, textToDocOptions);

  type DocPayload = {
    doc?: unknown;
    refs?: unknown[];
    parseError?: boolean;
    expressionRoot?: string;
    error?: { kind?: string; message?: string };
  };

  // SAFETY: Rust side returns Prettier's `Doc` JSON wrapped with `{ doc, refs }` for sharing.
  // Expression fragments may carry 2 extra fields:
  // - `parseError`: the text did not parse; reported as data because Prettier's
  //   `v-on` printer falls back from the expression form to the event-handler
  //   (statements) form only on a Babel-shaped syntax error
  // - `expressionRoot`: root node type for Prettier's `__onHtmlBindingRoot` hook,
  //   which drives the hug-vs-expand layout of attribute values
  // A failed call instead returns `{ error: { kind: "syntax" | "internal", message } }`
  // (plus `parseError` for the fragment kinds `v-on` falls back from).
  let payload: DocPayload | null = null;
  // Failures are remembered per kind, NOT "first response wins": an internal
  // failure means oxfmt is broken and must never be reported as (nor masquerade
  // behind) an earlier attempt's syntax error.
  let syntaxFailure: string | null = null;
  let internalFailure: string | null = null;

  for (const ext of extAttempts) {
    let attempt: DocPayload | null;
    try {
      const docJSON = await jsTextToDoc(
        ext,
        embeddedSourceText,
        _oxfmtPluginOptionsJson as string,
        parentContext,
      );
      attempt = docJSON === null ? null : (JSON.parse(docJSON) as DocPayload);
    } catch (error) {
      // A rejected NAPI promise (a Rust panic, a binding-load failure, ...)
      // never reaches the Rust-side classifier, so classify it here.
      internalFailure ??= toDetail(error);
      continue;
    }

    if (attempt === null) {
      internalFailure ??= "`oxfmt::textToDoc()` returned no payload";
      continue;
    }
    if (attempt.error) {
      const detail = attempt.error.message || "no details available";
      if (attempt.error.kind === "internal") internalFailure ??= detail;
      else syntaxFailure ??= detail;
      // Keep the failure payload: it carries the `parseError` marker below.
      payload ??= attempt;
      continue;
    }

    payload = attempt;
    break;
  }

  if (payload === null || payload.error) {
    // Internal wins: a syntax error from an earlier attempt is not the cause
    // when the run actually broke inside oxfmt.
    const kind: EmbeddedFailureKind = internalFailure === null ? "syntax" : "internal";
    const detail = internalFailure ?? syntaxFailure ?? "no details available";
    reportEmbeddedFailure(kind, embeddedSourceText, parentContext, detail);

    // Prettier swallows whatever is thrown here and emits the fragment verbatim,
    // except for the Babel-shaped syntax error its `v-on` printer falls back on
    // (`language-html/embed/vue-attributes.js`). An internal failure must NOT
    // claim to be that, so the cause is attached only for a real syntax error
    // on a fragment kind Rust marked with `parseError`.
    if (kind === "syntax" && payload?.parseError) {
      throw new Error(`\`oxfmt::textToDoc()\` failed to parse the fragment: ${detail}`, {
        cause: { code: "BABEL_PARSER_SYNTAX_ERROR" },
      });
    }
    throw new Error(`\`oxfmt::textToDoc()\` failed (${kind}): ${detail}`);
  }

  reportEmbeddedSuccess(embeddedSourceText);

  const { doc, refs, expressionRoot } = payload as {
    doc: unknown;
    refs: unknown[];
    expressionRoot?: string;
  };

  if (expressionRoot) {
    // `shouldHugJsExpression` only reads `.type`, a minimal fake AST is enough.
    (
      textToDocOptions as { __onHtmlBindingRoot?: (ast: unknown, options: unknown) => void }
    ).__onHtmlBindingRoot?.({ type: expressionRoot }, textToDocOptions);
  }

  // Fast path for no refs (common when formatting small AST fragments).
  if (refs.length === 0) return doc as Doc;

  // Sparse array sized to ref count; index = ref id.
  // Faster than `Map` for dense numeric keys and avoids hashing overhead.
  const cache: unknown[] = Array.from({ length: refs.length });
  return resolveRefs(doc, refs, cache) as Doc;
};

/** Best-effort message for a thrown value crossing the NAPI boundary. */
function toDetail(error: unknown): string {
  if (error instanceof Error) return error.message || String(error);
  return String(error);
}

/**
 * Rust emits `Interned` sub-trees once into `refs` and references them via `{ _REF: <id> }` placeholders,
 * preventing exponential JSON blowup when the same sub-tree is duplicated variants.
 *
 * Restore shared object references so Prettier sees the original (memory-shared) structure.
 * Identity does not affect output because Prettier identifies groups by their `id` field,
 * not by JS object identity.
 *
 * The `_REF` key (uppercase, prefixed) is chosen to never collide with valid Prettier Doc node keys,
 * so the `typeof obj._REF === "number"` check uniquely identifies placeholders.
 *
 * Refs are resolved on-demand with memoization.
 * A ref `i` may reference any other ref `j` (including `j < i`) because Rust caches `Interned` by pointer
 * and an earlier-encountered `Interned` (smaller id) can also appear inside a later one's content.
 * Topological / reverse-order resolution would observe `undefined` holes, so we recurse lazily.
 */
function resolveRefs(node: unknown, rawRefs: unknown[], cache: unknown[]): unknown {
  if (node === null || typeof node !== "object") return node;
  if (Array.isArray(node)) return node.map((n) => resolveRefs(n, rawRefs, cache));

  const obj = node as Record<string, unknown>;
  if (typeof obj._REF === "number") {
    const id = obj._REF;
    // Doc values are never `undefined`,
    // so `undefined` in `cache[id]` means "not yet cached" rather than "cached undefined".
    const cached = cache[id];
    if (cached !== undefined) return cached;
    const resolved = resolveRefs(rawRefs[id], rawRefs, cache);
    cache[id] = resolved;
    return resolved;
  }

  const out: Record<string, unknown> = {};
  for (const k in obj) out[k] = resolveRefs(obj[k], rawRefs, cache);
  return out;
}

/**
 * Detects the fragment mode from the (pseudo-)parser name and Prettier's internal flags.
 *
 * Prettier's HTML/Vue embeds request expression fragments through pseudo-parser names:
 * - `__(js|ts)_expression`: plain expression (`v-if`, `v-show`, `v-for` right-hand side,
 *   and the first attempt for `v-on`/`@event`)
 * - `__vue(_ts)_expression`: Vue expression (`{{ ... }}` interpolations, `v-bind`/`:prop`)
 * - `__vue(_ts)_event_binding`: `v-on`/`@event` fallback, parsed as statement(s)
 * The attribute-vs-interpolation position arrives as `__isInHtmlAttribute`.
 *
 * When Prettier formats Vue SFC templates with a regular parser, it instead
 * signals the fragment through special flags:
 * - `__isVueForBindingLeft`: v-for left-hand side (e.g., `(item, index)` in `v-for="(item, index) in items"`)
 * - `__isVueBindings`: v-slot bindings (e.g., `{ item }` in `#default="{ item }"`)
 * - `__isEmbeddedTypescriptGenericParameters`: `<script generic="...">` type parameters
 */
function detectParentContext(
  parser: string,
  parentParser: string,
  options: Record<string, unknown>,
): string {
  switch (parser) {
    case "__js_expression":
    case "__ts_expression":
      return "__isInHtmlAttribute" in options
        ? "expression-attribute"
        : "expression-interpolation";
    // The Vue expression parsers get their own kinds: they enable the
    // Vue 2 filter-sequence layout (`{{ msg | uppercase }}`), which must NOT
    // apply to `__js_expression` positions like the `v-for` right-hand side
    // (see Prettier's `isVueFilterSequenceExpression` in binaryish.js).
    case "__vue_expression":
    case "__vue_ts_expression":
      return "__isInHtmlAttribute" in options
        ? "vue-expression-attribute"
        : "vue-expression-interpolation";
    case "__vue_event_binding":
    case "__vue_ts_event_binding":
      return "event-handler";
    default:
  }

  if (parentParser === "vue") {
    if ("__isVueForBindingLeft" in options) return "vue-for-binding-left";
    if ("__isVueBindings" in options) return "vue-bindings";
    if ("__isEmbeddedTypescriptGenericParameters" in options) return "vue-script-generic";
    return "vue-script";
  }
  if (parentParser === "svelte") {
    return "svelte-script";
  }

  // Everything else is a full-program embed named after its host parser
  // ("markdown", "mdx", "html", ...). A `__`-prefixed name is one of Prettier's
  // pseudo-parsers, which always requests a *fragment*: reaching here means this
  // plugin registered a pseudo-parser the switch above never mapped. Forwarding
  // it would silently format the fragment as a full program, so let the Rust
  // side reject it as an internal error instead.
  return parser.startsWith("__") ? parser : parentParser;
}
