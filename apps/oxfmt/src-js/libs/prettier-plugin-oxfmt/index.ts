/**
 * Prettier plugin that uses `oxc_formatter` for (j|t)s-in-xxx part.
 *
 * When Prettier formats Vue/HTML (which can embed JS/TS code inside) files,
 * it calls the `embed()` function for each block.
 *
 * By default, it uses the `babel` or `typescript` parser and `estree` printer.
 * Therefore, by overriding these internally, we can use `oxc_formatter` instead.
 * e.g. Now it's possible to apply our builtin sort-imports for JS/TS code inside Vue `<script>`.
 */

import { textToDoc } from "./text-to-doc";
import type { Parser, Printer, Doc, SupportOptions } from "prettier";

// NOTE: Custom options must be declared here,
// or Prettier's normalization drops them before they reach `textToDoc()`.
export const options: SupportOptions = {
  _oxfmtPluginOptionsJson: {
    category: "JavaScript",
    type: "string",
    default: "{}",
    description: "Bundled JSON string for oxfmt-plugin options",
  },
  _oxfmtVueScriptLang: {
    category: "JavaScript",
    type: "string",
    default: "",
    description:
      'Effective `lang` of Vue SFC `<script>` blocks when it needs disambiguation (currently only "tsx"), scanned by the host',
  },
};

const oxfmtParser: Parser<Doc> = {
  parse: textToDoc,
  astFormat: "OXFMT",
  // Not used but required
  locStart: () => -1,
  locEnd: () => -1,
};

export const parsers: Record<string, Parser> = {
  // Override default JS/TS parsers
  babel: oxfmtParser,
  "babel-ts": oxfmtParser,
  typescript: oxfmtParser,
  // Override the pseudo-parsers Prettier uses for embedded expressions:
  // Vue `v-if`/`v-show`-style directives and the `v-for` right-hand side...
  __js_expression: oxfmtParser,
  __ts_expression: oxfmtParser,
  // ...`{{ ... }}` interpolations and `v-bind`/`:prop` values...
  __vue_expression: oxfmtParser,
  __vue_ts_expression: oxfmtParser,
  // ...and `v-on`/`@event` values that don't parse as a single expression.
  __vue_event_binding: oxfmtParser,
  __vue_ts_event_binding: oxfmtParser,
};

export const printers: Record<string, Printer<Doc>> = {
  OXFMT: {
    print: ({ node }) => node,
  },
};
