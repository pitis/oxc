import validCompile from "./rules/valid-compile.js";

/**
 * Oxlint plugin for the `svelte/*` rules that need the Svelte compiler itself.
 *
 * Everything else in `eslint-plugin-svelte` is implemented natively in oxlint,
 * under the same `svelte/` prefix. Only the rules that have to run the real
 * compiler live here, because that needs the `svelte` package at lint time.
 *
 * @type {import('@oxlint/plugins').Plugin}
 */
export default {
  meta: { name: "svelte-compiler" },
  rules: {
    "valid-compile": validCompile,
  },
};
