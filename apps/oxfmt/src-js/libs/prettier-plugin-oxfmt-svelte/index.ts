/**
 * Prettier plugin that teaches Prettier the `svelte` language and hands it to
 * `oxc_formatter_svelte`.
 *
 * A `.svelte` file never reaches Prettier at all — oxfmt formats it directly.
 * This exists for the one place Prettier still owns a Svelte component: the
 * ` ```svelte ` code blocks a Markdown or MDX file may contain, which the
 * Markdown printer formats by asking `textToDoc()` for the `svelte` parser.
 *
 * The `languages` entry is what makes Prettier's `inferParser()` recognise a
 * fence tagged `svelte` in the first place; without it the block is emitted
 * verbatim.
 */

import { svelteTextToDoc } from "./text-to-doc";
import type { Parser, Printer, Doc, SupportLanguage, SupportOptions } from "prettier";

// NOTE: Custom options must be declared here,
// or Prettier's normalization drops them before they reach `textToDoc()`.
export const options: SupportOptions = {
  // A key of its own: `_oxfmtPluginOptionsJson` is also what makes the host
  // install the JavaScript plugin, which must not happen just because a
  // Markdown file may contain a Svelte block.
  _oxfmtSveltePluginOptionsJson: {
    category: "JavaScript",
    type: "string",
    default: "{}",
    description: "Bundled JSON string for the oxfmt Svelte plugin's options",
  },
};

export const languages: Partial<SupportLanguage>[] = [
  {
    name: "svelte",
    parsers: ["svelte"],
    extensions: [".svelte"],
    vscodeLanguageIds: ["svelte"],
  },
];

export const parsers: Record<string, Parser> = {
  svelte: {
    parse: svelteTextToDoc,
    astFormat: "OXFMT_SVELTE",
    // Not used but required
    locStart: () => -1,
    locEnd: () => -1,
  },
};

export const printers: Record<string, Printer<Doc>> = {
  OXFMT_SVELTE: {
    print: ({ node }) => node,
  },
};
