import fs from "node:fs";
import { createRequire } from "node:module";

/**
 * Warning codes the rule never reports.
 *
 * `missing_declaration` fires for anything the component does not declare
 * itself, which for a linter is `eslint/no-undef`'s job and is wrong for
 * ambient globals. `eslint-plugin-svelte` drops it for the same reason.
 */
const IGNORED_CODES = new Set(["missing-declaration", "missing_declaration"]);

/** Codes that a `<style global>` block makes meaningless. */
const CSS_UNUSED_SELECTOR_CODES = new Set(["css-unused-selector", "css_unused_selector"]);

/** `lang` values the Svelte compiler can read without a preprocessor. */
const NATIVE_STYLE_LANGS = new Set(["css", "postcss", "pcss"]);

/**
 * Run the Svelte compiler over the component and report what it warns about.
 *
 * @type {import('@oxlint/plugins').Rule}
 */
export default {
  meta: {
    docs: {
      description: "disallow warnings when compiling",
    },
    schema: [
      {
        type: "object",
        properties: { ignoreWarnings: { type: "boolean" } },
        additionalProperties: false,
      },
    ],
    messages: {},
    type: "problem",
  },

  create(context) {
    if (!context.filename.endsWith(".svelte")) return {};

    const ignoreWarnings = Boolean(context.options?.[0]?.ignoreWarnings);

    return {
      Program() {
        let source;
        try {
          source = fs.readFileSync(context.filename, "utf8");
        } catch {
          // The file went away between the lint run reading it and now.
          return;
        }

        // A JS plugin rule is dispatched once per `<script>` block, and the
        // AST it is handed is that block alone. The compiler works on the
        // whole file, so run only on the first block — or on the empty
        // section a component with no `<script>` gets — and translate every
        // position back into that block's coordinate space.
        const offset = subHostOffset(source, context.sourceCode.text);
        if (offset === null) return;

        const result = compileComponent(source);
        if (result === null) return;
        const { warnings, isError } = result;
        if (ignoreWarnings && !isError) return;

        const globalStyleRanges = findGlobalStyleRanges(source);
        for (const warning of warnings) {
          if (shouldSkip(warning, globalStyleRanges)) continue;
          const [start, end] = positionOf(warning, source);
          context.report({
            message: warning.code ? `${warning.message}(${warning.code})` : warning.message,
            // `loc` would be bounds-checked against this block's own text;
            // a `range` is taken as-is and offset by the block's start, which
            // is what lets a markup position be reported from here.
            node: { range: [start - offset, end - offset] },
          });
        }
      },
    };
  },
};

/**
 * Where in `source` the `<script>` block the rule was invoked for begins, or
 * `null` when this is not the block that should do the work.
 *
 * A component can have two `<script>` blocks, and reporting from both would
 * report every warning twice.
 */
function subHostOffset(source, blockText) {
  const blocks = findScriptContentRanges(source);
  // No `<script>` at all: the section is empty and starts at the file's start.
  if (blocks.length === 0) return blockText.length === 0 ? 0 : null;
  // The extracted text can start a little after the tag — the loader drops the
  // line break that follows `>` — so locate it rather than assuming.
  const [start, end] = blocks[0];
  const found = source.indexOf(blockText, start);
  return found !== -1 && found + blockText.length <= end ? found : null;
}

/** The content ranges of every `<script …>…</script>` block, in order. */
function findScriptContentRanges(source) {
  const ranges = [];
  const openTag = /<script(\s[^>]*)?>/giu;
  let match;
  while ((match = openTag.exec(source)) !== null) {
    const contentStart = match.index + match[0].length;
    const contentEnd = source.indexOf("</script", contentStart);
    if (contentEnd === -1) break;
    ranges.push([contentStart, contentEnd]);
    openTag.lastIndex = contentEnd;
  }
  return ranges;
}

/**
 * Compile the component, returning its warnings — or the error it failed
 * with, as a single warning, which is how `eslint-plugin-svelte` reports it.
 *
 * Returns `null` when the compiler is not installed, so a project that does
 * not have `svelte` as a dependency just gets no diagnostics from this rule
 * rather than a crash.
 */
function compileComponent(source) {
  const compile = loadCompile();
  if (compile === null) return null;
  // A `<style lang="…">` the compiler cannot read has to go before it sees
  // the file; a preprocessor would normally have turned it into CSS by now.
  const text = blankUnreadableStyles(source);
  try {
    return { warnings: compile(text, { generate: false }).warnings, isError: false };
  } catch (error) {
    return {
      warnings: [
        {
          code: error.code,
          message: error.message,
          start: error.start,
          end: error.end,
          position: error.position,
        },
      ],
      isError: true,
    };
  }
}

/**
 * `svelte/compiler`'s `compile`, resolved from the linted project rather than
 * from this package, so the project's own Svelte version is the one that
 * decides. `null` once we know it is not installed.
 */
let cachedCompile;
function loadCompile() {
  if (cachedCompile === undefined) {
    try {
      cachedCompile = createRequire(`${process.cwd()}/`)("svelte/compiler").compile;
    } catch {
      cachedCompile = null;
    }
  }
  return cachedCompile;
}

/**
 * Replace the body of every `<style lang="…">` the compiler cannot read with
 * spaces, so every offset after it still lines up with the original file.
 */
function blankUnreadableStyles(source) {
  const openTag = /<style(\s[^>]*)?>/giu;
  let text = source;
  let match;
  while ((match = openTag.exec(source)) !== null) {
    const lang = /\blang\s*=\s*["']?([\w-]+)/iu.exec(match[1] ?? "")?.[1];
    if (lang === undefined || NATIVE_STYLE_LANGS.has(lang.toLowerCase())) continue;
    const contentStart = match.index + match[0].length;
    const contentEnd = source.indexOf("</style", contentStart);
    if (contentEnd === -1) break;
    text =
      text.slice(0, contentStart) +
      blankOut(source.slice(contentStart, contentEnd)) +
      text.slice(contentEnd);
    openTag.lastIndex = contentEnd;
  }
  return text;
}

/** The same text with everything but its line breaks turned into spaces. */
function blankOut(text) {
  return text.replace(/[^\n]/gu, " ");
}

/** The character range of every `<style global>` block. */
function findGlobalStyleRanges(source) {
  const ranges = [];
  const openTag = /<style(\s[^>]*)?>/giu;
  let match;
  while ((match = openTag.exec(source)) !== null) {
    const closing = source.indexOf("</style", match.index);
    if (closing === -1) break;
    if (/\bglobal\b/u.test(match[1] ?? "")) {
      ranges.push([match.index, source.indexOf(">", closing) + 1]);
    }
    openTag.lastIndex = closing;
  }
  return ranges;
}

function shouldSkip(warning, globalStyleRanges) {
  if (!warning.code) return false;
  if (IGNORED_CODES.has(warning.code)) return true;
  // A `<style global>` block styles things this component cannot see, so
  // "unused selector" says nothing.
  if (!CSS_UNUSED_SELECTOR_CODES.has(warning.code)) return false;
  const start = warning.position?.[0] ?? warning.start?.character;
  const end = warning.position?.[1] ?? warning.end?.character;
  if (start === undefined || end === undefined) return false;
  return globalStyleRanges.some(([from, to]) => from <= start && end <= to);
}

/** A warning's character range, falling back to the file's start. */
function positionOf(warning, source) {
  const start = warning.position?.[0] ?? warning.start?.character;
  const end = warning.position?.[1] ?? warning.end?.character;
  if (typeof start !== "number") return [0, Math.min(1, source.length)];
  return [start, typeof end === "number" ? end : start];
}
