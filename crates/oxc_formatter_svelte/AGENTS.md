# Coding agent guides for `crates/oxc_formatter_svelte`

Follow @../oxc_formatter_core/FORMATTER_POLICY.md , this file holds only the Svelte-specific rules and translations.

## Overview

Prettier compatible `.svelte` formatter (`oxfmt`'s Tier 1 backend), replacing `prettier-plugin-svelte`.

- Built on `oxc_formatter_core` for the language-agnostic IR + Printer + builders
  - See `crates/oxc_formatter_core/AGENTS.md` for the IR/pipeline details
- Entry points:
  - `format()`: standalone, on a service-less session — every embed stays verbatim
  - `format_with_session()`: on the caller's `FormatSession`, which is what lets
    `<script>` reach `oxc_formatter`, `<style>` reach `oxc_formatter_css`, and `{…}`
    reach the JS expression fragment path

oxfmt reaches it two ways, and both are `format_with_session`: a `.svelte` file is
`FileKind::OxcFormatterSvelte`, and a ` ```svelte ` code block in Markdown or MDX arrives
through `jsTextToDoc` with `source_ext: "svelte"`, whose IR is converted to a Prettier `Doc`
for the Markdown printer to embed (`src-js/libs/prettier-plugin-oxfmt-svelte`).

This crate prints **markup only**. Everything in another language goes out through the
dispatcher (`DispatchRequest`) and comes back as IR to splice, so there is exactly one
JavaScript formatter and one CSS formatter in the process.

### Parser

`svelte_markup_parser` (sibling crate `pitis/svelte-markup-parser`, pinned by git tag in the
workspace `Cargo.toml`). Two of its guarantees are what make a printer possible:

- **Total span coverage** — every byte of the source belongs to exactly one node, so anything
  can be re-emitted verbatim by span.
- **A `recovered` flag** — set whenever the parser had to guess at markup the Svelte compiler
  would reject. `format_with_session` refuses outright when it is set: reprinting a guess
  changes what the component means.

A bug found in this crate that turns out to be a _parse_ bug belongs upstream in that crate.
Validate a parser change two ways: `cargo run --release --example parse_corpus -- <svelte
checkout>` for the coverage invariant, and a differential against `svelte/compiler`'s own
accept/reject verdict **and attribute lists** over the same corpus.

## Shape

`print/` mirrors what Prettier's plugin decides, one file per decision:

| file            | what it owns                                                                               |
| :-------------- | :----------------------------------------------------------------------------------------- |
| `mod.rs`        | the top level: section ordering (`svelteSortOrder`), the children dispatch, `write_source` |
| `children.rs`   | `printChildren`: the layout of one sibling list, and `prettier-ignore`                     |
| `classify.rs`   | block vs inline vs neither, `<pre>` content, raw-text elements                             |
| `element.rs`    | one element: hug decisions, separators, the open/close tag shapes                          |
| `attribute.rs`  | attributes, shorthand, quoting, the `class` rules                                          |
| `text.rs`       | a text run as words joined by breaks                                                       |
| `expression.rs` | `{…}`, and the declaration tag `{const …}`                                                 |
| `block.rs`      | `{#if}` / `{#each}` / `{#await}` / `{#key}` / `{#snippet}` and the `{@…}` tags             |
| `raw_text.rs`   | `<script>` and `<style>`: their tags here, their bodies dispatched                         |

Two translations are worth knowing before changing anything:

- **Prettier decides layout while printing; this decides it first.** `printChildren` reaches back
  to rewrap the doc it just emitted and trims text nodes in place as it goes. Neither is
  available here, so `children.rs` takes the same decisions into a `ChildrenPlan` and the
  printing reads the plan. Every check in it must read a text value **as trimmed so far**, never
  the original — once a node has given its trailing whitespace to a neighbour, later checks see
  it as not ending in whitespace and lay the next child out accordingly.
- **A node type is not a display category.** Prettier's `isInlineElement` / `isBlockElement`
  return true only for a `RegularElement`. A component, a `<svelte:…>`, a `<slot>` and a
  `<title>` are _neither_: the whitespace at their edges is dropped, but they do not force a
  break. `classify.rs::is_regular_element` is that predicate.

## Deliberate divergences from `prettier-plugin-svelte`

Each is either a Prettier bug that corrupts a component, or a reduced port with a stated reason.
`apps/oxfmt/conformance` carries the same notes against the fixtures that show them.

**Prettier bugs, kept:**

- `<svelte:element this="h{n}" />` — Prettier drops the `{n}`. Its own fixture says
  "we don't try to fix this bug".
- `prop='"'` — Prettier rewrites it as `prop="""`, which no longer parses. A value whose text
  carries a `"` is quoted with `'` here. A value carrying _both_ a `"` and a `{…}` has no
  spelling this can produce and keeps Prettier's.

**Reduced ports:**

- `{let a = 1, b = 2}` keeps its spelling. A declaration tag's single declarator is formatted
  through the expression path; two of them would come back as the sequence expression
  `(a = 1), (b = 2)`, a different declaration that does not parse as one.
- An `{#each … as PATTERN}` / `{:then PATTERN}` binding keeps its spelling. Prettier
  re-serializes it with a bespoke pattern printer (`expandNode`) that preserves literal spelling
  and never breaks lines; the fragment path here would reach it through the estree printer,
  which does neither. Canonical spacing already matches.
- A `<!-- #endregion -->` immediately after a hoisted `<script>`/`<style>` does not travel with
  it when sections are reordered (Prettier's `extractRegionEndTrailAfterHoistedEnd`). The
  _leading_ comment does.
- `@format` / `requirePragma` / `insertPragma`: oxfmt supports these for no language.
- `svelteSortOrder` must name all four sections. Prettier requires only `options` and silently
  drops whatever is left out, which deletes that section's content rather than moving it.

**Core printer, shared by every oxc formatter (never changes meaning):**

- A space that would start a line is dropped, where Prettier keeps it. `LineMode::SoftOrSpace`
  only sets a pending space when the line already has content.
- A `fill` breaks one word earlier than Prettier's does; see the same note in
  `crates/oxc_formatter_css/AGENTS.md`.
- A `{…}` whose expression breaks continues at the mustache's indent, where Prettier adds one
  level: Prettier prints the expression as a real estree node whose unknown parent makes
  `printBinaryishExpression` indent, and this goes through the JS _fragment_ path, which does
  not.

## Verifying a change

- `cargo test -p oxc_formatter_svelte` — the inline tests, which record each divergence above
  that a unit test can reach.
- `pnpm --filter oxfmt-app conformance` — the `svelte` category runs
  `prettier-plugin-svelte`'s own 80-fixture suite against **real Prettier**, so the number means
  something. Regenerating the snapshot needs `pnpm --filter oxfmt-app download-fixtures` first;
  a run without the externals rewrites the committed snapshot with a fraction of the fixtures.
- **Idempotency.** Format a corpus twice and diff. A whitespace-sensitive printer that is not
  idempotent is broken however good its conformance number is, and two of this crate's worst
  bugs (a header growing by one space per run, an attribute value gaining a quote) showed up
  this way and no other.
- Verify an _option_ with the option set. The default-config corpus does not exercise
  `bracketSameLine`, `htmlWhitespaceSensitivity`, or Tailwind sorting at all.
- `pnpm --filter oxfmt-app test` covers what a corpus cannot: the CLI, the LSP and the API,
  where a change of _policy_ shows up (which files are formatted at all, whether a config key
  gates anything, whether import sorting reaches an embedded `<script>`).
