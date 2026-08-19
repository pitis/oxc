import fs from "node:fs";
import type { Plugin } from "#oxlint/plugins";

// A rule that works on the whole `.svelte` file rather than on the `<script>`
// block it is handed — the shape `svelte-compiler/valid-compile` needs.
//
// It exists to pin down two things a component with no `<script>` used to get
// wrong: that the rule is invoked at all, and that it can report at a position
// outside the extracted section.
const plugin: Plugin = {
  meta: {
    name: "whole-file-plugin",
  },
  rules: {
    "report-first-tag": {
      create(context) {
        return {
          Program() {
            const source = fs.readFileSync(context.filename, "utf8");
            const start = source.indexOf("<div");
            if (start === -1) return;
            // Offsets are relative to the extracted section, which for a
            // component with no `<script>` starts at the file's start.
            const scriptStart = source.indexOf(context.sourceCode.text);
            const offset = context.sourceCode.text.length === 0 ? 0 : scriptStart;
            context.report({
              message: "found the first `<div>`",
              node: { range: [start - offset, start - offset + 4] },
            });
          },
        };
      },
    },
  },
};

export default plugin;
