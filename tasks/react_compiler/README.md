# React Compiler comparison

Compares the published `babel-plugin-react-compiler` pipeline with the local
`oxc-transform-react` NAPI package. Both pipelines run React Compiler first,
remove TypeScript syntax, and lower JSX with the automatic runtime.

Build the native binding and run the default TSX fixture:

```sh
pnpm --dir napi/transform-react build-test
pnpm --filter react_compiler compare
```

Pass another JavaScript, JSX, TypeScript, or TSX file and tune the benchmark:

```sh
pnpm --filter react_compiler compare ./path/to/component.tsx \
  --iterations 1000 \
  --warmup 20 \
  --print
```

`--print` prints both generated outputs after the timing summary.
