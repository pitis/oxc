import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

import { transformSync as babelTransformSync } from "@babel/core";
import transformReactJsx from "@babel/plugin-transform-react-jsx";
import transformTypescript from "@babel/plugin-transform-typescript";
import reactCompiler from "babel-plugin-react-compiler";
import { transformSync as oxcTransformSync } from "oxc-transform-react";

const DEFAULT_FILENAME = fileURLToPath(new URL("fixture.tsx", import.meta.url));
const DEFAULT_ITERATIONS = 100;
const DEFAULT_WARMUP_ITERATIONS = 10;

const args = process.argv.slice(2);
const printOutput = takeFlag(args, "--print");
const iterations = takeNumberOption(args, "--iterations", DEFAULT_ITERATIONS);
const warmupIterations = takeNumberOption(args, "--warmup", DEFAULT_WARMUP_ITERATIONS);
const filename = args.shift() ?? DEFAULT_FILENAME;

if (args.length > 0) {
  throw new Error(`Unexpected arguments: ${args.join(" ")}`);
}

const sourceText = await readFile(filename, "utf8");
const isTypescript = /\.[cm]?tsx?$/i.test(filename);
const isTsx = /\.tsx$/i.test(filename);

function transformWithBabel() {
  const plugins = [[reactCompiler, {}]];
  if (isTypescript) {
    plugins.push([
      transformTypescript,
      {
        allExtensions: true,
        isTSX: isTsx,
      },
    ]);
  }
  plugins.push([transformReactJsx, { runtime: "automatic" }]);

  const result = babelTransformSync(sourceText, {
    babelrc: false,
    comments: true,
    configFile: false,
    filename,
    plugins,
    sourceMaps: false,
    sourceType: "unambiguous",
  });

  assert(result?.code, "Babel did not produce code");
  assert.match(result.code, /react\/compiler-runtime/, "Babel did not run React Compiler");
  return result.code;
}

function transformWithOxc() {
  const result = oxcTransformSync(filename, sourceText);
  const errors = result.errors.filter(({ severity }) => severity === "Error");
  assert.deepEqual(errors, [], errors.map(({ message }) => message).join("\n"));
  assert(result.code, "oxc-transform-react did not produce code");
  assert.match(result.code, /react\/compiler-runtime/, "Oxc did not run React Compiler");
  return result.code;
}

const babelOutput = transformWithBabel();
const oxcOutput = transformWithOxc();

const results = [
  benchmark("babel-plugin-react-compiler", transformWithBabel),
  benchmark("oxc-transform-react", transformWithOxc),
];
const [babelResult, oxcResult] = results;

console.log(`Input: ${filename} (${Buffer.byteLength(sourceText)} bytes)`);
console.table(
  results.map(({ name, meanMilliseconds, operationsPerSecond, outputBytes }) => ({
    compiler: name,
    "mean (ms)": meanMilliseconds.toFixed(3),
    "operations/s": operationsPerSecond.toFixed(1),
    "output bytes": outputBytes,
  })),
);
console.log(
  `oxc-transform-react speedup: ${(babelResult.meanMilliseconds / oxcResult.meanMilliseconds).toFixed(2)}x`,
);

if (printOutput) {
  console.log("\n--- babel-plugin-react-compiler ---\n");
  console.log(babelOutput);
  console.log("\n--- oxc-transform-react ---\n");
  console.log(oxcOutput);
}

function benchmark(name, transform) {
  for (let index = 0; index < warmupIterations; index++) {
    transform();
  }

  const start = performance.now();
  let output = "";
  for (let index = 0; index < iterations; index++) {
    output = transform();
  }
  const elapsed = performance.now() - start;
  const meanMilliseconds = elapsed / iterations;

  return {
    name,
    meanMilliseconds,
    operationsPerSecond: 1000 / meanMilliseconds,
    outputBytes: Buffer.byteLength(output),
  };
}

function takeFlag(values, flag) {
  const index = values.indexOf(flag);
  if (index === -1) {
    return false;
  }
  values.splice(index, 1);
  return true;
}

function takeNumberOption(values, option, defaultValue) {
  const index = values.indexOf(option);
  if (index === -1) {
    return defaultValue;
  }

  const value = Number(values[index + 1]);
  if (!Number.isInteger(value) || value <= 0) {
    throw new Error(`${option} must be a positive integer`);
  }
  values.splice(index, 2);
  return value;
}
