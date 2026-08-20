# Fork status

This is a fork of [`oxc-project/oxc`](https://github.com/oxc-project/oxc) with one goal: make
`oxlint` and `oxfmt` complete enough that a JavaScript project can delete Prettier and ESLint
outright, rather than running them alongside. Svelte first, then Vue/Nuxt, then plain Node.

Nothing here is submitted upstream; the upstream repository is read-only from this fork.

Everything below is measured, not estimated. Every figure names the command that produced it, so
a stale number can be re-derived rather than trusted. Figures were last taken **2026-08-21**
against ESLint 9.39.4 / 10.8.1, Prettier 3.9.6, `eslint-plugin-vue` 10.7.0–10.9.1 and
`eslint-plugin-svelte` 3.23.0.

## Summary

| Area                       | Lint                                                          | Format                                                |
| :------------------------- | :------------------------------------------------------------ | :---------------------------------------------------- |
| **Svelte**                 | 83 / 86 rules; `recommended` **37 / 37**                      | native Rust, no Prettier in the path                  |
| **Vue**                    | 105 / 250 rules; a stock Nuxt config is **90%** covered       | Prettier, via NAPI (Tier 3)                           |
| **TypeScript, type-aware** | 40 / 40 of `strictTypeChecked`; **99.9%** finding-for-finding | —                                                     |
| **Everything else**        | 1,014 rules, 142 more than upstream                           | native Rust for JS/TS, JSON, CSS, YAML, GraphQL, TOML |

The short version: **Svelte can drop both tools today. Vue can drop Prettier today, and drop
ESLint once the ten rules in [Vue](#vue) land. Node/NestJS backends can drop both today**,
subject to the tsconfig caveat below.

One thing that is _not_ a coverage question and bites first: **`oxlint` exits with an error when
its config names a rule it does not implement.**

```
Failed to parse oxlint configuration file.
  x Rule 'no-mutating-props' not found in plugin 'vue'
```

So there is no mechanical translation of an `eslint.config.mjs`. Each project needs an
`.oxlintrc.json` written to name only implemented rules. A missing rule is a config error, not a
silent loss of coverage — which is the safer failure, but it does mean adoption is per-project
work no matter how high the coverage figure goes.

## Svelte

**Lint — 83 of `eslint-plugin-svelte`'s 86 rules, and all 37 in `recommended`.**

The three that are absent are absent on purpose:

| Rule                                                 | Why                                                                                                                                                                                                  |
| :--------------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `svelte/valid-compile`                               | Needs `svelte/compiler` at lint time. Deliverable as a first-party oxlint JS plugin — `svelte` is already every Svelte project's dependency — but not built.                                         |
| `svelte/indent`                                      | 2,700 lines upstream, entirely redundant now that `oxfmt` formats `.svelte` natively.                                                                                                                |
| `svelte/@typescript-eslint/no-unnecessary-condition` | A re-export of the typescript-eslint rule, which exists here as `typescript/no-unnecessary-condition`. tsgolint cannot see `.svelte` files, so the Svelte-namespaced spelling has nothing to run on. |

**Format — Tier 1, native.** `oxc_formatter_svelte` prints the markup; the `<script>`, `<style>`
and `{…}` inside it reach `oxc_formatter` and `oxc_formatter_css` through the dispatcher.
`prettier-plugin-svelte` is gone from the bundle and from the runtime dependencies, and so is the
`svelte` optional peer dependency — nothing in `oxfmt` needs `svelte/compiler` any more. A
` ```svelte ` block inside Markdown or MDX gets the same formatter.

Conformance runs `prettier-plugin-svelte`'s own 80-fixture suite against **real Prettier** as the
oracle, currently 76/80, with each remaining difference recorded as a deliberate divergence in
`crates/oxc_formatter_svelte/AGENTS.md`. Before the native printer this category used
`prettier-plugin-svelte` as both implementation and oracle and reported 80/80, which measured
nothing.

Verified end to end on `svelte-number-format`: all eight Prettier/ESLint dev dependencies removed,
`svelte` and `svelte-check` kept, verdicts byte-for-byte identical, roughly 290× faster on lint.

## Vue

**Lint — 105 of `eslint-plugin-vue`'s 250 rules.** Most of the 145 absent ones are stylistic rules
no preset enables, but a project is free to enable them, so the number that matters is coverage of
a config someone actually runs:

| Config resolved for a `.vue` file               | Covered         |
| :---------------------------------------------- | :-------------- |
| `@nuxt/eslint-config` (stock, default features) | 170 / 188 (90%) |
| A production Vue 3 + Nuxt monorepo config       | 174 / 188 (93%) |
| `eslint-plugin-vue` `flat/recommended` alone    | 94 / 118        |

Ten missing rules are the substance of the gap:

`no-mutating-props`, `no-undef-components`, `no-unused-components`, `no-unused-vars`,
`no-use-computed-property-like-method`, `one-component-per-file`, `order-in-components`,
`require-explicit-emits`, `require-valid-default-prop`, `jsx-uses-vars`.

A stock Nuxt config additionally wants `attributes-order`, `first-attribute-linebreak`,
`html-end-tags`, `html-self-closing` and `no-multiple-template-root`, plus core `no-octal` and
`nuxt/prefer-import-meta`. `no-restricted-syntax` is the outlier: it needs an esquery selector
engine, which is a project of its own.

**What the gap costs in practice is close to nothing on mature code, and that is not the same as
nothing.** Enabling the missing rules under real ESLint across 891 `.vue` files produced 311
findings — but every one came from `attributes-order` (209), `html-self-closing` (60) and
`no-multiple-template-root` (25), none of which that project enables. The missing rules it _does_
enable found zero violations, because they have been enforced there for years. Switching costs no
cleanup; it does remove the net, and `no-mutating-props` catches genuine Vue bugs.

`no-template-shadow` is the worked example of what porting one of these takes, and of how it gets
checked: 21 hand-written cases and 1,602 real `.vue` files across six repositories, diffed
finding-for-finding against eslint-plugin-vue 10.9.1. Both linters report the same 12 violations at
the same columns and agree on every other file. One case only that differential could have caught:
a value binding the same name twice (`v-for="(a, a) in xs"`) is an early error in the grammar
vue-eslint-parser borrows, so upstream gives the element no variables and reports nothing —
`oxc_parser` raises early errors from its semantic pass, which these throwaway snippets never run,
so the duplicate has to be caught explicitly.

`no-ref-as-operand` is the second worked example, and it is where the verification method had to
get sharper. Both linters report **zero** findings across the 1,602 real `.vue` files — true, and
almost worthless: a rule that never fires agrees trivially with a rule that never fires, and that
result would have been identical had the port done nothing at all. Two things were needed to
measure anything.

First, a **positive control**: ten hand-written `.vue` files covering every branch — each factory
under its own name, both `defineModel` spellings, the Options API `setup(props, ctx)` and
`setup(props, { emit })` emit paths, auto-imported (`globals`-declared) `ref`, and files that must
stay silent. 20 findings, 20 shared, no divergence.

Second, a **mutation of the real corpus**: strip `.value` from every `.vue` file that has it (752
files) and diff again. That produces ~5,800 induced violations in real component code rather than
in hand-written snippets, and it is what actually found the bugs — three rounds of them.

|                   |                                    files | eslint | oxlint |    shared |
| :---------------- | ---------------------------------------: | -----: | -----: | --------: |
| real corpus       |                                    1,602 |      0 |      0 |         0 |
| positive control  |                                       10 |     20 |     20 |    **20** |
| `.value` stripped | 752 (36 unparseable by eslint, excluded) |  5,774 |  5,774 | **5,764** |

The 10 remaining differences are in two files, and neither is a rule-level divergence:

- `QrPreviewCard.vue` — both find the same nine violations, but vue-eslint-parser reports them
  **33 lines off** in a file that has both a `<script>` and a `<script setup>` block. Deleting the
  first block makes the two agree exactly, so oxlint's positions are the correct ones.
- `PhoenixUserDetailsPanel.vue` — one finding two columns apart, on a line containing an em dash.
  oxlint counts columns in UTF-8 bytes where ESLint counts characters. Reproduced with core
  `eslint(no-debugger)`, so it is a linter-wide convention, not this rule.

Three real bugs came out of that mutation run, all in the same area: what happens when a ref is
copied into another variable. Upstream re-registers _every_ reference of a binding each time it
reaches it, so the outcome depends on processing order — and the order is the **import-specifier**
order, not the factory order, which means `import { computed, ref }` and `import { ref, computed }`
genuinely disagree about the same code. On top of that, `defineChain` decides reportability, and a
global `_processedIds` set makes the _first_ source to reach an identifier the winner. All three
are reproduced, with the reasoning recorded on `register` and `define_sites` in the rule.

**Format — Tier 3.** `.vue` markup still goes to Prettier through NAPI. The `<script>` and
`<style>` blocks and the directive expressions inside the template are already native. A native
Vue printer would be the same shape of work as `oxc_formatter_svelte`, on top of the sibling
`vue_sfc_parser` crate; it is not started.

Verified on three production repos — 2,059, 2,735 and 2,530 files — with **zero** formatting
differences against Prettier 3.9.6.

## oxlint in general

1,014 rules across 17 plugins, 142 more than upstream at the same merge base (the entire `svelte`
plugin, and 59 `vue` rules).

| plugin       | rules |     | plugin     | rules |
| :----------- | ----: | :-- | :--------- | ----: |
| `eslint`     |   187 |     | `jest`     |    60 |
| `unicorn`    |   138 |     | `jsx_a11y` |    36 |
| `typescript` |   110 |     | `import`   |    33 |
| `vue`        |   105 |     | `oxc`      |    27 |
| `react`      |    85 |     | `jsdoc`    |    23 |
| `svelte`     |    83 |     | `nextjs`   |    21 |
| `vitest`     |    73 |     | others     |    33 |

`import-x/*` config keys resolve to the `import/*` rules, so an ESLint config using `eslint-plugin-import-x`
does not need rewriting on that account.

### Type-aware rules

A NestJS backend on `tseslint.configs.strictTypeChecked` enables 75 rules, 40 of them type-aware.
All 40 are registered here. On 560 `.ts` files:

|                                        |                      findings |
| :------------------------------------- | ----------------------------: |
| ESLint                                 |                         1,171 |
| oxlint `--type-aware`                  |                         1,191 |
| identical `(file, line, column, rule)` | **1,170 — 99.9% of ESLint's** |

0.5s against 7.3s, with type information in both. The single miss is one `no-unnecessary-condition`.
Fifteen of the twenty-one extras are `no-unnecessary-type-assertion`; spot checks found them real.

**The caveat that costs the most time.** Type-aware linting runs through `oxlint-tsgolint`, a
separate Go binary built on tsgo — the TypeScript 7 port — which **removed `baseUrl`**. When it
cannot load the project it does not fail loudly; it reports

```
tsconfig.json:15:5: error typescript(tsconfig-error): Option 'baseUrl' has been removed.
```

and then silently runs only the syntactic rules. That looked like 162 findings against ESLint's
1,171, with every `no-unsafe-*` rule reporting nothing. Removing `baseUrl` and making `paths`
relative took it to 1,222. Both edits are TypeScript 5 compatible, but verify the build after
making them.

`npm i -D oxlint-tsgolint` is required; it is not pulled in by `oxlint` itself.

## oxfmt in general

| Language                                          | Implementation                  |
| :------------------------------------------------ | :------------------------------ |
| JS, TS, JSX, TSX                                  | `oxc_formatter`                 |
| JSON, JSONC, `package.json`                       | `oxc_formatter_json`            |
| CSS, SCSS, Less                                   | `oxc_formatter_css`             |
| YAML                                              | `oxc_formatter_yaml`            |
| GraphQL                                           | `oxc_formatter_graphql`         |
| **Svelte**                                        | `oxc_formatter_svelte`          |
| TOML                                              | taplo (Rust)                    |
| **Vue, HTML, Angular, Markdown, MDX, Handlebars** | delegated to Prettier over NAPI |

Prettier is still bundled inside `oxfmt`'s own `dist/` for that last row, so a project's manifest
can be Prettier-free even where Prettier code still runs. Removing the bundle means writing native
printers for Markdown and Vue.

Measured parity: **9,332 of 9,332 files across nine repositories are byte-identical to Prettier
3.9.6.**

Two false alarms are worth recording, because both looked like formatter bugs and neither was:

- A long YAML flow mapping that `oxfmt` "refused to break" was `oxfmt` at its default `printWidth`
  of 100 compared against Prettier's default of 80.
- A `pnpm-lock.yaml` that "differed" was `oxfmt` correctly declining to format a lock file at all
  (`EXCLUDE_FILENAMES` in `apps/oxfmt/src/core/support.rs`).

Always confirm both sides use the same resolved config, including per-package `.prettierrc` files
(`vueIndentScriptAndStyle` and `printWidth` are the two that have caught us).

## Reproducing the numbers

Rule inventory for this fork:

```sh
{ git ls-tree -r --name-only HEAD crates/oxc_linter/src/rules/ \
    | grep -E '^crates/oxc_linter/src/rules/[a-z_0-9]+/[a-z_0-9]+\.rs$' \
    | sed 's|crates/oxc_linter/src/rules/||; s|\.rs$||' | grep -v '/mod$'
  git ls-tree -r --name-only HEAD crates/oxc_linter/src/rules/ \
    | grep -E '^crates/oxc_linter/src/rules/[a-z_0-9]+/[a-z_0-9]+/mod\.rs$' \
    | sed 's|crates/oxc_linter/src/rules/||; s|/mod\.rs$||'
} | awk -F/ '{gsub(/_/,"-",$2); print $1"/"$2}' | sort -u
```

Rules a project actually enables — always resolve them, never read the config source, because
presets and `eslint-config-prettier` turn a great many back off:

```sh
npx eslint --print-config path/to/Component.vue
```

Then diff that list against the inventory, mapping `@typescript-eslint/x` to `typescript/x` or
`eslint/x`, `import-x/x` to `import/x`, and a bare `x` to `eslint/x`.

Finding-for-finding agreement is a separate measurement from coverage and answers a different
question: run both linters over the same files with `-f json`, reduce each diagnostic to
`(file, line, column, rule)`, and intersect. It shows whether implemented rules _behave_ like
ESLint's. It cannot show a missing rule, since a rule that does not run contributes to neither
side — so always report it next to a coverage figure, never instead of one.

Isolating one rule differs between the two: ESLint takes a config that enables only it, while
`oxlint` needs its categories switched off explicitly.

```jsonc
// .oxlintrc.json — `-A all -D <rule>` does NOT work here: the CLI flag
// re-enables the rule with default options, discarding the config's.
{
  "plugins": ["vue"],
  "categories": {
    "correctness": "off",
    "suspicious": "off",
    "pedantic": "off",
    "perf": "off",
    "style": "off",
    "restriction": "off",
    "nursery": "off",
  },
  "rules": { "vue/no-template-shadow": ["error", { "allow": ["item"] }] },
}
```

## What is next

1. The ten Vue rules listed above.
2. `attributes-order`, the largest single gap for a stock Nuxt config.
3. `svelte/valid-compile` as a first-party JS plugin.
4. A native Vue printer, to move `.vue` off Prettier.
5. Native Markdown, the last thing keeping Prettier in `oxfmt`'s bundle.

One test failure is known and pre-existing on `main`, unrelated to any of the above:
`oxlint::lsp::server_linter::test::test_frameworks`. The `lint::suppression` tests that drive
`oxlint-tsgolint` also fail locally, there because the spawned binary takes a `SIGPIPE`.

`oxlint::lint::test::lint_svelte_file` used to be listed here too, on the reading that
`eslint/no-unassigned-vars` had "stopped reporting" on `export let` in a `.svelte` file. That was
wrong: 51b5719e6d deliberately made the rule skip `.svelte`/`.vue`, because the template can assign
a binding through `bind:this`/`bind:value`/`v-model` and the script pass cannot see it — the same
guard, and the same reason, as `prefer-const`. The rule was right and the CLI snapshot was simply
never regenerated. It has been.
