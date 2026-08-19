# oxlint-plugin-svelte

The Svelte lint rules that need the Svelte compiler itself, as an
[Oxlint JS plugin](https://oxc.rs/docs/guide/usage/linter/js-plugins).

Every other rule of `eslint-plugin-svelte` is implemented natively in Oxlint,
under the same `svelte/` prefix, and needs nothing installed. Only the rules
that have to run the real compiler live here, because running it means loading
the `svelte` package at lint time.

## Rules

| Rule                            | Description                                 |
| ------------------------------- | ------------------------------------------- |
| `svelte-compiler/valid-compile` | Report what the Svelte compiler warns about |

## Usage

```jsonc
// .oxlintrc.json
{
  "jsPlugins": ["oxlint-plugin-svelte"],
  "rules": {
    "svelte-compiler/valid-compile": "error",
  },
}
```

`svelte` is resolved from the project being linted, so the version you build
with is the version that decides. With no `svelte` installed, the rule reports
nothing rather than failing.

### `valid-compile`

Compiles each `.svelte` file and reports the compiler's warnings, each one
carrying its code — `Unused CSS selector ".b"(css_unused_selector)`.

```jsonc
{
  "rules": {
    // Report only what stops the component compiling, not its warnings.
    "svelte-compiler/valid-compile": ["error", { "ignoreWarnings": true }],
  },
}
```

Two codes are never reported: `missing_declaration`, which is `eslint/no-undef`'s
job and is wrong for ambient globals, and `css_unused_selector` inside a
`<style global>` block, which styles what the component cannot see.

#### Differences from `eslint-plugin-svelte`

- **`svelte-ignore` comments are left to the compiler.** Upstream strips them
  before compiling and re-applies them itself, so that it can also report the
  ones that matched nothing. Here the compiler applies them, and reporting a
  `svelte-ignore` that matched nothing is `svelte/no-unused-svelte-ignore`'s
  job — a separate rule Oxlint implements natively.
- **A `<style lang="…">` the compiler cannot read is blanked out rather than
  preprocessed.** Upstream will run `sass`, `less`, `stylus` or `postcss` over
  it when they are installed and map the positions back. Here such a block is
  replaced with whitespace before compiling, which keeps every other position
  in the file exact but means no CSS diagnostics come out of it. `lang="css"`,
  `postcss` and `pcss` are read as-is.
- **A component the compiler cannot parse still gets a diagnostic.** Upstream's
  parser fails first, so ESLint reports a fatal parse error and the rule never
  runs; here the same compiler message arrives as an ordinary rule diagnostic.
- **A component with two byte-identical `<script>` blocks is reported twice.**
  The rule runs once per extracted `<script>` block and works out which one it
  is by matching text, which cannot tell two identical blocks apart. Svelte
  allows at most one instance and one module script, so this needs both to be
  written identically.
