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

### The routes JavaScript goes out through

Every `{…}` leaves through the dispatcher, and which route it asks for is a layout decision, not
a parsing one. `ExpressionPosition` in `expression.rs` is the whole list:

| position                | route                         | why                                                                                                     |
| :---------------------- | :---------------------------- | :------------------------------------------------------------------------------------------------------ |
| `Braces`, `BlockHeader` | `svelte-expression`           | spliced bare between two braces, so a broken expression supplies its own indent                         |
| `QuotedAttribute`       | `svelte-attribute-expression` | the same, inside a quoted value                                                                         |
| `EachSubject`           | `svelte-each-subject`         | as `BlockHeader`, but a top-level comma there is Svelte's index, not a sequence operator                |
| `BindDirective`         | `svelte-bind-value`           | the embed site wraps it in an indent already, and a comma there is Svelte's `{get, set}` pair           |
| `SnippetSignature`      | `svelte-snippet-signature`    | a function signature: the caller wraps it as `function name(params) {}` and the JS side prints the head |

A block header is additionally flattened with `remove_lines` — it is one line however long it
gets — but flattening keeps the hard lines a broken member chain is made of, so it still needs
the route that indents.

The two comma routes are not a nicety. A sequence expression at a fragment root keeps parentheses
(`{#key a, b}` prints `{#key (a, b)}`), and Svelte spells two of its own forms with a comma in a
slot it hands over whole: `{#each expr, index}` written without `as`, and `bind:x={get, set}`.
Parenthesizing there joins two things Svelte reads separately — and for the `{#each}`, loses the
index. Adding the rule without the two routes rewrote 127 corpus files.

Four translations are worth knowing before changing anything:

- **Prettier decides layout while printing; this decides it first.** `printChildren` reaches back
  to rewrap the doc it just emitted and trims text nodes in place as it goes. Neither is
  available here, so `children.rs` takes the same decisions into a `ChildrenPlan` and the
  printing reads the plan. Every check in it must read a text value **as trimmed so far**, never
  the original — once a node has given its trailing whitespace to a neighbour, later checks see
  it as not ending in whitespace and lay the next child out accordingly.
- **A node type is not a display category, and a display category is not a layout rule.** Three
  separate predicates, and picking the wrong one is the mistake this crate has made most often:
  - `isInlineElement` / `isBlockElement` return true only for a `RegularElement`. A component, a
    `<svelte:…>`, a `<slot>` and a `<title>` are _neither_ — whitespace at their edges is
    dropped, but they do not force a break. `classify.rs::is_regular_element` is that predicate.
  - **Hugging** is what most tag-shape decisions actually key on, and an element hugs unless it
    is a block. So a component hugs, and `<span>` and `<div>` are not the pair that names the
    rule — a third kind is needed to tell "inline" from "not block" apart. `should_hug_start` /
    `should_hug_end` in `element.rs`.
  - `<pre>` and `<textarea>` need the node type _as well as_ the name: `<Textarea>` is a
    component, and components with those names are common.
- **Whether a `<pre>` encloses something is a question about ancestors**, and what sits between
  an ancestor and a text node is not always an element — a `{#if}` inside a `<pre>` has branches
  of its own. It is a flag on the context (`is_in_pre`), not an argument, which is how Prettier
  asks it (`isPreTagContent` walks the path).
- **A text run's leading break is part of its fill**, and which slot each word lands in decides
  where the run wraps. A run that follows a sibling puts that break in the sequence's first
  place, which pairs every word with a `line` item from there on, and the run then wraps late —
  Prettier's own output overruns `printWidth` by the last word's length. A blank line is _two_
  breaks and puts the words back. `text.rs` builds the same sequence Prettier's
  `splitTextToDocs` does, including the empty `Word` that stands beside a blank line, and the
  parity is the whole point of it.

## Deliberate divergences from `prettier-plugin-svelte`

Each is either a Prettier bug that corrupts a component, or a reduced port with a stated reason.
`apps/oxfmt/conformance` carries the same notes against the fixtures that show them.

**Prettier bugs, kept:**

- `<svelte:element this="h{n}" />` — Prettier drops the `{n}`. Its own fixture says
  "we don't try to fix this bug".
- `prop='"'` — Prettier rewrites it as `prop="""`, which no longer parses. A value whose text
  carries a `"` is quoted with `'` here. A value carrying _both_ a `"` and a `{…}` has no
  spelling this can produce and keeps Prettier's.
- An **unquoted `{…}` value on a `<script>` or `<style>` tag** — `<script data-c={ 1 }>`. Svelte
  does not interpolate there, so the value is text; Prettier agrees and then splits it across
  lines on its whitespace, producing markup that no longer parses. It comes back quoted here
  (`data-c="{ 1 }"`). The construct has no meaning in Svelte either way.

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

Check this list before designing around anything in it. Entries have twice turned out to be
divergences rather than facts about the printer — a `fill` that broke a word early (core `Fill`
carrying two cases Prettier does not have) and a `{…}` that continued at the mustache's own indent
(the route this crate was asking for). Both were fixable, and both had shaped decisions around
them before anyone tried.

## Verifying a change

- `cargo test -p oxc_formatter_svelte` — the inline tests, which record each divergence above
  that a unit test can reach.
- `pnpm --filter oxfmt-app conformance` — the `svelte` category runs `prettier-plugin-svelte`'s
  own 79 external cases plus this crate's own edge cases against **real Prettier**, so the number
  means something. Regenerating the snapshot needs `pnpm --filter oxfmt-app download-fixtures`
  first; a run without the externals rewrites the committed snapshot with a fraction of the
  fixtures.
- The corpus differential is at **6,673 of 6,673**. That is the bar a change has to clear now: any
  difference it reports is a regression, not a backlog item.
- **A real-world differential.** Every bug found in this crate since the fixture suite went green
  came from formatting whole open-source repositories with both this and Prettier and diffing —
  not from the fixtures, which by construction only cover what someone already thought of. Two
  rules make the number mean anything: resolve **each file's own Prettier config** (a single
  `printWidth` across repos manufactures thousands of differences that are not real), and read
  the **per-repository** breakdown rather than the total, which is what exposes a harness bug and
  which is where a long-form application diverges from a component library. `FORK-STATUS.md`
  names the corpus and carries the current figure.
- **Cluster, then reduce.** Group the differing files by what the hunk looks like and take the
  largest class first; then shrink one file to a few lines and read Prettier's _doc_ for it with
  `prettier.__debug.printToDoc`. That beats reading the plugin's source: more than once the
  plugin's builder has been the same shape as this one's and the difference was entirely in what
  followed it.
- **Idempotency.** Format a corpus twice and diff. A whitespace-sensitive printer that is not
  idempotent is broken however good its conformance number is, and two of this crate's worst
  bugs (a header growing by one space per run, an attribute value gaining a quote) showed up
  this way and no other.
- Verify an _option_ with the option set. The corpus repositories run close to the defaults, so
  they do not exercise `bracketSameLine`, `htmlWhitespaceSensitivity: ignore`, or Tailwind
  sorting at all — the conformance suite's second option set is the only thing that does, and it
  is what caught a tag-shape branch being taken in the wrong order.
- `pnpm --filter oxfmt-app test` covers what a corpus cannot: the CLI, the LSP and the API,
  where a change of _policy_ shows up (which files are formatted at all, whether a config key
  gates anything, whether import sorting reaches an embedded `<script>`).
