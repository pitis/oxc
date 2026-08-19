import { defineConfig } from "#oxlint";

export default defineConfig({
  categories: {
    correctness: "off",
  },
  jsPlugins: ["./plugin.ts"],
  rules: {
    "whole-file-plugin/report-first-tag": "error",
  },
});
