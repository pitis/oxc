# Fork status

This is a fork of [`oxc-project/oxc`](https://github.com/oxc-project/oxc) with one goal: make
`oxlint` and `oxfmt` complete enough that a JavaScript project can delete Prettier and ESLint
outright, rather than running them alongside. Svelte first, then Vue/Nuxt, then plain Node.

Nothing here is submitted upstream; the upstream repository is read-only from this fork.

Everything below is measured, not estimated. Every figure names the command that produced it, so
a stale number can be re-derived rather than trusted. Figures were last taken **2026-08-21**, and
the Svelte, Vue and JS/TS formatting ones **2026-08-22**, against ESLint 9.39.4 / 10.8.1,
Prettier 3.9.6, `eslint-plugin-vue` 10.7.0–10.9.1 and `eslint-plugin-svelte` 3.23.0.

## Summary

| Area                       | Lint                                                          | Format                                              |
| :------------------------- | :------------------------------------------------------------ | :-------------------------------------------------- |
| **Svelte**                 | 83 / 86 rules; `recommended` **37 / 37**                      | native Rust; **100%** byte-identical on 6,673 files |
| **Vue**                    | 118 / 250 rules; a stock Nuxt config is **100%** covered      | native Rust; **100%** byte-identical on 5,245 files |
| **TypeScript, type-aware** | 40 / 40 of `strictTypeChecked`; **99.9%** finding-for-finding | —                                                   |
| **Everything else**        | 1,029 rules, 157 more than upstream                           | native Rust; JS/TS **99.9%** on 8,205 files         |

The short version: **Svelte can drop both today — every one of 6,673 real-world files comes back
byte-identical to Prettier, and the three constructs where this printer deliberately differs are
recorded below and appear in none of them. Vue can drop both today for any config this fork covers
— a stock Nuxt config is now fully covered. Node/NestJS backends can drop both today**, subject to
the tsconfig caveat below.

That Svelte figure is new, and it began as a correction: until it was measured the file claimed
Svelte could drop both tools, on the strength of one 21-file library and a fixture suite. What the
corpus found was four differences that changed what a component means or renders — a dropped
`then`, text re-wrapped inside a `<pre>`, a `{#snippet}` header parenthesized into a different
signature, and a `generics` type mangled — none of which any fixture reached, and a long tail of
layout differences behind them. All of it is fixed. See
[The Svelte printer against a real-world corpus](#the-svelte-printer-against-a-real-world-corpus).

One thing that is _not_ a coverage question and bites first: **`oxlint` exits with an error when
its config names a rule it does not implement.**

```
Failed to parse oxlint configuration file.
  x Rule 'no-unused-components' not found in plugin 'vue'
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

Conformance runs `prettier-plugin-svelte`'s own fixture suite, plus edge cases of this fork's own,
against **real Prettier** as the oracle — currently 90/93 at both option sets, with each remaining difference recorded
as a deliberate divergence in `crates/oxc_formatter_svelte/AGENTS.md`. Before the native printer
this category used `prettier-plugin-svelte` as both implementation and oracle and reported 80/80,
which measured nothing.

Verified end to end on `svelte-number-format`: all eight Prettier/ESLint dev dependencies removed,
`svelte` and `svelte-check` kept, verdicts byte-for-byte identical, roughly 290× faster on lint.

### The Svelte printer against a real-world corpus

Until 2026-08-21 that end-to-end check was the _only_ real-world evidence for the Svelte printer —
one library, 21 files, against Vue's 1,602. The conformance suite is the stronger of the two
signals and Svelte has it, but the breadth check that found most of the Vue printer's bugs had
never been run. It has now been, over **6,673 `.svelte` files in six open-source repositories**,
each file formatted under **its own repo's Prettier config** resolved per file:

| Repository                 | Files |         Identical |
| :------------------------- | ----: | ----------------: |
| `skeletonlabs/skeleton`    |   686 |      686 (100.0%) |
| `huntabyte/bits-ui`        |   617 |      617 (100.0%) |
| `carbon-components-svelte` |  1408 |     1408 (100.0%) |
| `huntabyte/shadcn-svelte`  |  1681 |     1681 (100.0%) |
| `immich-app/immich` (web)  |   415 |      415 (100.0%) |
| `windmill-labs/windmill`   |  1866 |     1866 (100.0%) |
| **Total**                  |  6673 | **6673 (100.0%)** |

Neither tool failed on any of them, and every one of them now comes back byte-identical. The first
measurement had **windmill — the one large application — at 86.5%** against component libraries in
the high nineties. Component libraries have short markup;
an application has long `{#if …}` and `{#each …}` headers and long prose, and both are where the
printer diverged. It is the same shape as the Vue result, where a uniform corpus concealed what a
varied one exposed. `htmlWhitespaceSensitivity` was ruled out as a cause early on: forcing windmill
to `css` moved the count by one file.

**The block-header class is fixed**, which is where the 94.7% → 95.9% came from — 83 files, and
zero files that agreed beforehand now differ. A block header is one line however long it gets, so
its expression is flattened with `remove_lines`; that pass was shallow, and a call's arguments and
a member chain are printed as `BestFitting`, whose variants live in slices of their own. So
`{#if a && b}` flattened and `{#each Object.entries(x) as y}` broke inside its call. It now
descends, and resolves two things the printer would otherwise decide by measuring — conditional
content, or an over-long flattened call comes back wearing the broken form's trailing comma as
`f(a,)`; and `BestFitting`, by flattening each variant rather than choosing one.

Groups are deliberately left unmarked, though `Group::set_flat` exists for it and Prettier's own
`removeLines` clears a group's break flag. The two are not equivalent: Prettier still re-measures
and breaks a cleared group that does not fit, whereas a flat-marked group short-circuits that in
`print_best_fitting`. Measured both ways over the corpus — identical at 6,402 — and marking them
flat cost a conformance fixture, because Prettier does break a member chain in a block header.

**A `{…}` now indents its own continuation**, which took 95.9% → 97.5% — another 104 files, again
with nothing regressing. Prettier gives most embedded expressions a `JsExpressionRoot` parent,
which suppresses `printBinaryishExpression`'s indent because the host supplies one; Vue's embed
sites do exactly that. `prettier-plugin-svelte` does not — it splices the expression between two
braces and leaves the layout to it — so a broken `a || b` there indents itself, and ours was
continuing at the mustache's own column. Svelte's `{…}` and its attribute values now take routes of
their own (`svelte-expression`, `svelte-attribute-expression`) that say the host adds no indent;
the Vue routes are untouched, which is why js-in-vue stayed at 428/428. Only `bind:` values keep
the ordinary route, since they carry an indent from the embed site already. Block headers kept it
at first too, on the reasoning that a flattened expression has no continuation to indent — see
below for why that was wrong.

**An empty inline element keeps its attributes on the tag's line**, worth another 20 files. When
there is nothing between the tags, an inline element's closing tag borrows the opening `>` rather
than let a break introduce whitespace it would render; the two then move down together. Building
them as a group of their own is also what makes the attribute measurement right — it stops at that
break instead of counting the `></span>` that was never going to share the line. A block element
borrows nothing, so its `>` follows the attributes directly and the measurement is right without
any of this; applying the same shape there cost 28 files, which is how the distinction was found.
Prettier draws it the same way, visible in its own doc: `group([softline, group([">", "</span"])])`
for `<span>`, a plain `">", "</div>"` for `<div>`.

Reading Prettier's doc directly — `prettier.__debug.printToDoc` with the plugin loaded — is worth
more than reading its source for questions like this. The plugin's `openingTag` builder is the same
shape as this printer's, and the difference was entirely in what followed it.

**A flattened call keeps the room the broken form would have used**, worth 9 more files. A block
header is one line however long it gets, so its expression is flattened — and a call whose
arguments do not fit is a `BestFitting` whose last variant puts every argument on a line of its
own. Prettier writes the breaks around that list as `line`s; this printer wrote them as soft ones.
The two are the same wherever that variant is actually used, because it is always expanded and
either kind breaks then. They differ only under flattening, where a `line` becomes a space and a
soft one becomes nothing — so Prettier comes back with `filter( (c) => … )` and this printer came
back with `filter((c) => …)`. The conformance suite is unchanged by the swap, which is what
confirms the two are otherwise interchangeable.

**Inlineness turned out to be the wrong test** for it, worth 17 more files. `<span>` borrows and
`<div>` does not, and both readings of that fit the pair — but what the plugin actually asks is
whether the element hugs its content on both sides, and an element hugs unless it is a block. A
component is neither inline nor block, so it hugs and borrows; so does every element once
`htmlWhitespaceSensitivity` is `strict` and nothing is a block any more. Prettier prints
`<Skeleton class="…"` with its attributes on the tag line and `></Skeleton>` beneath, exactly as it
does for `<span>`. Two elements picked as a contrasting pair are not enough to name the rule that
separates them; the third kind is what settled it.

**A text run that follows a sibling wraps late**, worth 94 files and 97.5% → 99.2%. This one is a
Prettier behaviour that reads as a bug, and matching it is still the job. A run of text is printed
by filling a sequence of words and the breaks between them, and the decision at each break is made
by measuring the word, the break, and the word after it. But a text node keeps its own leading
break, so a run that follows a sibling puts that break in the sequence's _first_ place — and from
there on it is the breaks that sit where the words are measured. Each is measured on its own, one
column, so the line runs on until it is already full and the word that takes it past the width
stays where it is. The same paragraph one place earlier in an element wraps at the width; three
places on, after `<br /><br />`, it wraps at the width again, because a blank line is _two_ breaks
and the pair puts everything back. Prettier's own output overruns `printWidth` by however long that
last word is — 108 columns at a width of 100 is ordinary in windmill's markup.

Reproducing it took a change in the shared printer rather than in the Svelte one. Both build the
same sequence; the printer was the half that differed. `Fill` had two cases Prettier does not have,
asking whether the _separator_ fits on its own and moving an item that fits down to the next line
when it did not — so a sequence whose items were all breaks wrapped sensibly instead of the way
Prettier wraps it. It now decides a pair the way Prettier does, on the item and on the
item/separator/next-item triple and nothing else, which is three cases where there were five.

Two details had to land with it. A blank line was one element where Prettier writes two breaks,
which shifted every following word by one place; it is now the blank line plus an empty word beside
it. And an entry that adds no width of its own — a lone break, an empty one — measured as fitting
at any column, so the fill would never break at all. What decides that is one column: Prettier's
fits walk defers a flat break's space exactly as this printer does, and so refuses only a line that
is already _past_ the width, not one that ends on it. Read as "at the width" instead, a paragraph
whose last word lands exactly on the column wraps a word early — 20 files, and the one column is
what separated them.

The whole conformance suite — every language, several thousand fixtures — is unchanged by those
printer edits except for one Svelte fixture that now passes. That is the evidence the extra cases
were not carrying anything: they only ever differed for a separator with no break in it, which is a
shape only text produces.

**A `{#snippet}` header is a function signature**, worth 10 more files, and this one was wrong
rather than merely differently laid out. The header was formatted as an expression, where
`name(params)` is a call — so `(tightTop = false)` came back as `((tightTop = false))`, because an
assignment in argument position is parenthesized, and `({ node, level }: { node: X })` did not
parse at all and was kept as the author wrote it, tabs and all. Prettier wraps the header into
`function name(params) {}`, formats that, and drops the keyword and the body off the printed
document. Doing the same here — a `FunctionSignature` fragment context, and a `signature_only`
option on the function printer that leaves those two pieces out rather than slicing them off —
gives the parameters a signature's layout and a parameter's meaning. Quote style had to come with
it: every fragment context other than an expression is treated as sitting inside an attribute,
where single quotes are preferred, and a `{#snippet …}` header is delimited by braces instead.

**Text under a `<pre>` is kept as written at any depth**, which is the last of these that changed
what a component renders rather than how it reads. A `{#if}` inside a `<pre>` has branches of its
own, and their text was being laid out again — words re-wrapped, newlines collapsed to spaces — in
whitespace-significant content. The element knew it was a `<pre>`; the branches did not, because
what sits between an ancestor and a text node is not always an element and the flag was an
argument threaded through elements only. It is now a flag on the context, which is the question
Prettier asks by walking up the path. The trims still apply: a branch takes the whitespace at its
own edges and prints it as the break before `{:else}`, since a marker is markup the browser never
shows.

**A `<script>` tag's attributes are text**, which is the last of these and also a small correction
to what this printer thought an attribute value was. Svelte does not interpolate in a `<script>` or
`<style>` tag's attributes, so a `{…}` there is not an expression — and `generics="Item extends
{ label?: string; value: any }"` is a type parameter list, which came back as
`{label?: string; value: any}` because it had been read as an object and then failed to parse as
one. The value is now printed as written, with only its quoting normalized, which is what Prettier
does with it.

**A block header supplies its own indent too**, worth the last four. Flattening a header does not
remove the breaks the author cannot get rid of: a member chain long enough to break does so through
hard lines, which `remove_lines` keeps, as Prettier's `removeLines` does. What is left then sits
under the indent the binaryish chain around it supplies — and the header had been routed as a
fragment whose host indents it, on the reasoning that a flattened expression has no continuation to
indent. It has one exactly when flattening does not reach it.

A component called `<Textarea>` was being taken for the HTML element, along with `<Pre>` — the name
was the whole test, and Svelte tells an element from a component by the capital. Its content was
kept as written and its tag was laid out as whitespace-significant. Prettier asks for a
`RegularElement` as well as the name, and so does this now.

**A comma is not always a sequence**, and the last two of these came from either side of that. A
sequence expression at a fragment's root keeps the parentheses that tell it from an argument list,
`{#key a, b}` coming back `{#key (a, b)}` in Vue as in Svelte — but Svelte spells two of its own
forms with a comma in a slot it hands over whole, `{#each expr, index}` written without `as` and
`bind:x={get, set}`, and parenthesizing there joins two things Svelte reads separately. The first
run of the change, before those two had routes of their own, rewrote 127 files and would have
changed what several of them do; it is in now with the distinction the routes carry. Alongside it,
an `if` / `while` clause whose test is a negated logical hugs its parentheses instead of taking a
break of its own — `if (!(a || b))` — which also moved a Prettier fixture from 61% to 88%.

**A cast's parentheses are a node Prettier has and this printer does not**, which is what the
carbon repository's one difference came down to. `/** @type {T} */ (event).shiftKey` was breaking
inside the parentheses where Prettier breaks at the `.`, because two rules read the object and both
saw a bare identifier: the member lookup glues to `a.b` and never breaks, and an assignment breaks
after its operator to protect a chain it thinks is poorly breakable. Prettier's object in both is a
`ParenthesizedExpression`, so neither rule fires; here the cast classification says the same thing.
Both halves were needed — either alone leaves the line wrong in a different way — and the identical
shape with ordinary parentheses, `(event || window).shiftKey`, already read correctly, which is what
said where to look.

**A comment before a union type travels with the union.** An end-of-line comment after `?:` stays
beside the operator while the members do, and moves down with them the moment they take an indent
of their own — this printer only moved it for a single-member union, so a broken one left the
comment stranded on the `?:` line. Two of Prettier's own TypeScript fixtures now pass that did not
(`property-signature/consistent-with-flow/union.ts` and `union/5849.ts`), and a third went from 86%
to 97%, which is a better witness than the corpus file that found it.

**A tab is worth nothing, and the printer has to agree with itself about that.** The last file was
an element inside a `<pre>`, where the text is kept as written and the tags between it are still
laid out — so the column an element starts at is the one the preserved text left the line on. The
width measurement counted a literal tab as zero, as Prettier's `string-width` does; the printer
counted it as a tab stop. Two levels of disagreement about where the line was, on exactly the lines
the printer cannot re-indent, and elements that fit came back broken. Nothing else in the
conformance suite moved when the two were reconciled.

Nothing is left. The three differences the conformance suite still records are deliberate — a
declaration tag with two declarators, an `{#each … as PATTERN}` binding, and a `#endregion` marker
travelling with a hoisted section — each with its reason in
`crates/oxc_formatter_svelte/AGENTS.md`, and none of the three occurs anywhere in the corpus.

Five findings are bugs rather than layout, and all five are what a corpus is for — no fixture
reached any of them. Three are printer bugs described above with the class they were found in:
text under a `<pre>` re-wrapped at depth, a `{#snippet}` header read as a call, and a `generics`
type read as an object. The other two were in the parser, and are worth the detail because the
shape of each is the shape of the next one:

- **A bare `then` was dropped from an await block** — `{#await p then}` came back as
  `{#await p}`. Those are different components: with `then` and no pending block the body renders
  only after the promise resolves, and without it the body _is_ the pending block. The first
  difference found that changes what a component _does_ rather than how it reads, and no fixture
  covered the form.

  **Fixed** in `svelte_markup_parser` 0.2.3. The cause was in the header splitter, which reported
  only the _pattern_ after `then`/`catch`: a shorthand binding nothing was indistinguishable from
  no shorthand at all, so the body was filed as the pending branch. `{#await p then v}` and
  `{#await p catch e}` were never affected, which is why every fixture passed. Confirmed against
  `svelte/compiler`, which puts the body under `then` for this form, and against
  `prettier-plugin-svelte` for all six spellings — including `{#await p then}{/await}`, where
  Prettier drops the empty branch too and oxfmt now matches. Guarded by
  `edge-cases/svelte/await-block-branch-shorthands.svelte` and by unit tests in both crates. It
  accounted for 3 of the 356 differing files.

- **A regex literal ending in an escaped slash made oxfmt refuse the file.**
  `{x.replace(/^u\//, '')}` was rejected as not-well-formed: `scan_js` in `svelte_markup_parser`
  did not recognise regex literals, so the `//` that `\/` and the closing delimiter spell read as a
  line comment, and following it to end of line swallowed the closing brace. A regex holding a
  brace, a quote or a bracket failed the same way. Refusing is the safe failure — nothing was
  rewritten — but it is still a file the tool cannot format.

  **Fixed** in `svelte_markup_parser` 0.2.4, which scans regex literals whole and settles the
  regex-or-division ambiguity from the preceding token. Anything still ambiguous is read as
  **division**, deliberately: an unrecognised regex degrades to the old behaviour, while a division
  mistaken for a regex would consume up to the next `/` and could swallow the brace that ends the
  construct. A `/` at the start of a scanned slice is division for the same reason — those slices
  begin just after a `{`, where a leading `/` is a block closer like `{/each}`. That case is not
  hypothetical: assuming otherwise broke three existing tests. Verified against `svelte/compiler`
  on both readings and by re-parsing the corpus with 0 coverage violations; guarded by
  `edge-cases/svelte/regex-literals-in-expressions.svelte`.

Reproduce by cloning those six repositories and formatting every `.svelte` file twice — once
through `prettier` with only `prettier-plugin-svelte` loaded, once through `oxfmt`'s napi `format()`
— with **both sides given the same explicit options**, starting from Prettier's declared defaults
and overlaid with `prettier.resolveConfig(file)`. Skipping that last step is not a detail: oxfmt's
default `printWidth` is 100 and Prettier's is 80, so the two repositories that ship no config
reported 1,490 differences that were purely the defaults disagreeing.

## Vue

**Lint — 118 of `eslint-plugin-vue`'s 250 rules.** Most of the 132 absent ones are stylistic rules
no preset enables, but a project is free to enable them, so the number that matters is coverage of
a config someone actually runs:

| Config resolved for a `.vue` file               | Covered              |
| :---------------------------------------------- | :------------------- |
| `@nuxt/eslint-config` (stock, default features) | **185 / 185 (100%)** |
| A production Vue 3 + Nuxt monorepo config       | 174 / 188 (93%)      |
| `eslint-plugin-vue` `flat/recommended` alone    | 94 / 118             |

Nothing a stock config enables is missing any more. `no-undef-components` is the one rule from
the original gap list still absent, and no stock config turns it on; `no-restricted-syntax`
remains out of reach because it needs an esquery selector engine, which is a project of its own.

Core `no-octal` and `nuxt/prefer-import-meta` landed too, the latter in a new `nuxt` plugin
namespace. `no-restricted-syntax` is the outlier: it needs an esquery selector
engine, which is a project of its own.

**What the gap costs in practice is close to nothing on mature code, and that is not the same as
nothing.** Enabling the missing rules under real ESLint across 891 `.vue` files produced 311
findings — but every one came from `attributes-order` (209), `html-self-closing` (60) and
`no-multiple-template-root` (25), none of which that project enables. The missing rules it _does_
enable found zero violations, because they have been enforced there for years. Switching costs no
cleanup; it does remove the net.

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

`no-mutating-props` is the third, and the first rule here that spans both passes: an ordinary
`Rule` for the `<script>` half (`this.foo`, `setup(props)`, `defineProps` destructuring) and a
`VueTemplateRule` for the template half, joined by a new `needs_script_props()` hook that hands
the template the component's own prop names. Nothing forbade a rule doing both; no rule had.

Its verification needed a mutation that produces _prop_ mutations, and there is a neat one:
rewrite every `:attr="expr"` binding to `:attr.sync="expr"`. `.sync` is two-way, so any binding
whose expression roots at a prop becomes a reported mutation — real prop names, real expressions,
one token changed, 11,602 bindings across 1,397 files.

|                                        | files | eslint | oxlint |    shared |
| :------------------------------------- | ----: | -----: | -----: | --------: |
| real corpus, unmodified                | 1,602 |      0 |      0 |         0 |
| upstream's own suite (default options) |    42 |     71 |     71 |    **71** |
| upstream's own suite (`shallowOnly`)   |     2 |     10 |     10 |    **10** |
| `:attr` → `:attr.sync`                 | 1,397 |  1,232 |  1,232 | **1,232** |

100% on all three, message text included. The mutation run is what earned it: the first pass sat
at 97.33%, and both defects it exposed were in prop _discovery_ rather than in mutation detection,
which is where the risk actually lives on real code.

- A prop destructured **with a default** (`const { quantity = 5 } = defineProps<Props>()`) was
  lost. Upstream drops prop names that are also module-scope bindings, then adds the destructured
  names back; a destructured prop is always a module-scope binding, so implementing only the first
  half silently deletes every defaulted prop. 31 missed findings.
- `:src.sync="currentSrc!"` was reported when it should not be. Upstream's `getMemberChaining`
  unwraps optional chaining and (implicitly) parentheses, but not TypeScript wrappers, so a
  `TSNonNullExpression` root stops it. `get_inner_expression()` unwraps all three and must not be
  used here — the same parens-yes/TS-no distinction `no-ref-as-operand` needs.

The five template-level rules a stock Nuxt config wanted — `attributes-order`,
`html-self-closing`, `first-attribute-linebreak`, `html-end-tags`, `one-component-per-file` —
landed as one batch, verified together against the 1,602-file corpus:

| rule                        | eslint | oxlint |  shared |
| :-------------------------- | -----: | -----: | ------: |
| `attributes-order`          |    819 |    819 | **819** |
| `html-self-closing`         |    324 |    324 | **324** |
| `first-attribute-linebreak` |    286 |    286 | **286** |
| `html-end-tags`             |      0 |      0 |       0 |
| `one-component-per-file`    |      0 |      0 |       0 |

Zero rule-level divergence. The six positions that differ are one finding each, reported at a
column shifted by exactly the line's byte-minus-character count — the linter-wide column
convention, not a rule difference.

That run also found a bug one layer down, in `vue_sfc_parser`: void and raw-text element names
were matched case-**insensitively**, so `<Link>` was treated as the void `<link>` and its children
were silently reparented as siblings. HTML tag names are case-insensitive, but a Vue template
resolves a capitalised name to a component, and vue-eslint-parser parses the children of `<Link>`
and `<Textarea>`. Fixed in the parser (v0.1.1) rather than worked around here, because the wrong
tree was reaching every template rule, not just this one.

The five script-level rules a stock Nuxt config wanted landed as a second batch —
`order-in-components`, `require-valid-default-prop`, `no-use-computed-property-like-method`,
core `no-octal`, and `nuxt/prefer-import-meta`, the last of which required a new `nuxt` plugin
namespace.

These are the rules the corpus cannot verify: across all 1,602 files they produce **one** finding
between them, and that one — a `withDefaults(defineProps<Props>(), { borderWidth: 1 })` against a
`string`-typed prop — is the only real-world evidence available. So the verification leans on a
hand-written control instead:

|                                  | files | eslint | oxlint | shared |
| :------------------------------- | ----: | -----: | -----: | -----: |
| real corpus                      | 1,602 |      1 |      1 |  **1** |
| positive control, all five rules |     6 |     18 |     18 | **18** |
| `import.meta.x` → `process.x`    |    35 |     15 |     15 | **15** |

Zero divergence. Adding `no-octal` — an ESLint core `correctness` rule, so on by default — moved
the rule count printed in oxlint's own CLI summary line, which is why this batch also rewrites 62
`apps/oxlint` snapshots. Every one of them differs in that line and nothing else, checked
mechanically rather than by eye.

The last four — `no-unused-vars`, `no-unused-components`, `require-explicit-emits` and
`jsx-uses-vars` — close the gap. Each spans both passes, so this batch also generalised the
template context: a `<template>` rule can now ask for the component's props and emits, its
`computed` names, or its registered components, each collected once per file and only when an
enabled rule wants it.

`jsx-uses-vars` is implemented as a deliberate no-op, and that is the faithful port: upstream
reports nothing either, existing only to call ESLint's `markVariableAsUsed` so `no-unused-vars`
stops flagging a component referenced only from JSX. `oxc` resolves JSX identifiers as ordinary
references, so `eslint/no-unused-vars` already counts them — verified directly. The rule exists
so a config naming it resolves rather than failing.

|                                                | files | eslint | oxlint |  shared |
| :--------------------------------------------- | ----: | -----: | -----: | ------: |
| real corpus                                    | 1,602 |      0 |      0 |       0 |
| positive control                               |     5 |      9 |      9 |   **9** |
| unused `v-for` alias injected, `emits` emptied |   310 |    554 |    554 | **554** |

Zero divergence, message text included. The corpus was silent again, so the signal came from the
mutation: adding an `unusedIdx` alias to every simple `v-for` and emptying every `emits`
declaration produced 554 findings in real templates. It caught one real defect — a `<script setup>`
block exposes the binding `defineEmits()` returns to the template, so `@x="emit('y')"` is an emit
call just as `$emit('y')` is, and looking only for `$emit` missed it.

**Format — Tier 1, native.** `oxc_formatter_vue` is the only printer for `.vue`; Prettier is no
longer in that path. Built on the sibling `vue_sfc_parser` crate.

It was made the default, put back, and made the default again, which is the useful part of the
story — see [what the corpus could not tell us](#what-the-corpus-could-not-tell-us) below.

It is a port of Prettier's own HTML printer rather than of `prettier-plugin-svelte`, because
that is what a `.vue` file actually goes through: `oxc_formatter_svelte`'s architecture carried
over, its layout rules did not. The whole file is printed as one HTML document — the SFC's
top-level blocks are just its root elements — over a preprocessed node tree that mirrors
Prettier's own pipeline (whitespace extraction, CSS display, space sensitivity), and every
embedded language goes out through the existing dispatcher: `<script>` to `oxc_formatter`,
`<style>` to `oxc_formatter_css`, and each `{{ … }}`, `:prop`, `@click`, `v-for` and `v-slot`
value to the JS formatter under the fragment context Prettier uses for it.

Measured against Prettier 3.9.6 over the same 1,602 `.vue` files, at matched `printWidth`:

|                                    | files |    byte-identical |
| :--------------------------------- | ----: | ----------------: |
| SFC skeleton, template as written  | 1,602 |   576 (**36.0%**) |
| \+ the markup printer              | 1,602 | 1,233 (**77.0%**) |
| \+ attribute and expression values | 1,602 | 1,597 (**99.7%**) |
| \+ the `style` attribute           | 1,602 |  1,602 (**100%**) |

Speed, for scale: 1,602 files in 50ms, against a NAPI round-trip per file today.

The last five files were the `style` attribute, whose value Prettier sends to the CSS printer.
`oxc_formatter_css` now takes a `CssFragmentKind`, which says whether the input is a whole
stylesheet, a css-in-js template, or an attribute's declaration list; the attribute kind separates
its declarations with a break that renders as a space, and writes the final `;` only when that
break is taken — so `style="color: red; margin: 0"` stays on the line and the broken form gets a
`;` per line. The parser already reported a top-level declaration as a recoverable error that the
css-in-js path tolerated; the attribute kind tolerates the same one. The formatter conformance
suite is unchanged by it: CSS stays 221/221, SCSS and Less unmoved.

### What the corpus could not tell us

The printer is byte-identical with Prettier on **1,602 real-world components**, and on
**891 / 891** of chatlyn-ui at that repo's own per-directory config. On the strength of that it
was made the default — and Prettier's **own** Vue fixtures put it back for a while:

| Suite                         | Prettier path |   Native printer |
| :---------------------------- | ------------: | ---------------: |
| 1,602 real-world `.vue`       |             — | 1,602 (**100%**) |
| `js-in-vue` conformance (428) |  427 (99.77%) |   428 (**100%**) |

Eighteen fixtures regressed, in seven classes. The worst emitted **invalid markup**:
`:id="'&quot;' + id"` came back as `:id="'"' + id"`, because the value is unescaped so it can be
parsed and was never re-escaped on the way out. That one is fixed — `escape_double_quotes`
rewrites every `"` in the value's IR, reaching through `Interned` and `BestFitting` subtrees by
rebuilding them, and is applied at the attribute boundary and nowhere else, since an interpolation
has no delimiter to protect.

Four more are files the printer _refuses_ that Prettier formats, all from one root cause:
Prettier's Vue parser treats every top-level block except `<template>` as raw text, so
`const foo = "</"` inside a `<custom>` block is a string, while `vue_sfc_parser` reads it as markup
and then reports an unclosed element.

The lesson is the one this file keeps re-learning: **a real-world corpus is uniform, and
uniformity is what hides bugs.** 1,602 hand-written components exercise a narrow band of what the
format allows; 428 fixtures written by the people who defined the format do not, on purpose.
Neither substitutes for the other, and the conformance suite is the one that gates a default.
Run `pnpm --filter oxfmt-app download-fixtures` first — a conformance run without the externals
silently measures a fraction of the suite.

**None remain: js-in-vue is 428/428 at both option sets.** The last one, `api-component.vue`, was
never this printer's — `oxc_formatter` dropped the disambiguating comma in `<T = any,>() => {}`.

An earlier revision of this file said that difference was reproducible in a plain `.ts` file. **It
is not**, and finding that out is what settled the rest. Prettier decides the comma from the
_file's_ extension rather than the script's, so all four of these hold the same TypeScript and only
two keep it:

| host                                | Prettier     |
| :---------------------------------- | :----------- |
| `a.ts`                              | `<T = any>`  |
| a ` ```ts ` fence in Markdown       | `<T = any>`  |
| `<script lang="ts">` in a `.vue`    | `<T = any,>` |
| `<script lang="ts">` in a `.svelte` | `<T = any,>` |

oxfmt's standalone `.ts` output was therefore right all along. "Embedded ⇒ force the comma" would
have been wrong too: a Markdown fence _is_ embedded and does not get it, because Prettier formats
it under a `.ts` filepath of its own. Only a component script differs, so only a component script
can be the thing that says so.

Upstream oxc diverges here knowingly — the rule's own doc comment names ts-in-vue, and argues that
dropping the comma should key on the grammar the source is consumed as rather than on the host's
path. That is the better rule. It is not the rule the world's Prettier-formatted code is written
to, and a file carrying `<T = any,>` today would silently lose a character on its first oxfmt run,
so the fork honours the wart instead. A `<script>` now tells the JS formatter it is not a file of
its own through `parent_context`, the dispatcher channel that already existed for parent→child
facts (`ScriptInComponentFile`); the JS side receives it as
`TypeParameterAmbiguity::NeedsTrailingComma`. Markdown is untouched, because its fences never send
it. The Svelte half was verified against prettier-plugin-svelte 4.1.1 rather than assumed from Vue,
and it keeps the comma the same way.

Two others used to keep that fixture company, and both were in shared code rather than here.

`slogan.vue` was `oxc_formatter_core` expanding a tab to `indentWidth` columns when measuring
text. Prettier measures through `string-width`, which strips control characters, so a tab there
counts as nothing — confirmed by binary-searching the width at which Prettier breaks a string
with 0, 1, 2 and 5 tabs in it, which is the same width every time. What the two disagreed about
was never indentation: the printer emits that itself as `Indent`, and `from_text` only ever sees
_content_ — a tab inside a string literal, a comment, an attribute value. Correcting it moved
nothing anywhere else in the suite, which is the tell that the expansion was wrong rather than
load-bearing: JS, TS, CSS, SCSS, Less, YAML, GraphQL, Markdown and Svelte are all unchanged to the
fixture. `TextWidth::from_text` lost its `indent_width` parameter with it, and so did the six
call sites that had been threading one through for it.

`preferences-drawer.vue` used to be a third. `oxc-css-parser` parses a pseudo-class argument as a
selector only for `not`/`is`/`where`/`matches`/`has`/`global` and hands back opaque tokens for
everything else, so `:deep(...)` printed byte for byte — no combinator spacing, no comma spacing,
no quote or case normalisation. In a Vue project that is most of a scoped stylesheet, since
`:deep()` and `:slotted()` are how a component reaches outside its own scope. Prettier's parser
has no such list, so `oxc_formatter_css` now re-parses any argument the parser could not read as a
selector and prints it as one when that succeeds; an argument that is not a selector still prints
verbatim. The parser is handed the argument preceded by its own offset in spaces, so its spans are
real-source offsets — the selector printer reads the source through those spans throughout, and
spans from a bare substring would point it at the wrong bytes.

Closed: the attribute-value escaping (1); a blank line that blocks nothing could format gained
after their open tag, because the unformatted body was spliced back raw instead of going through
the text path that trims it (5); parser inference, which now reads `type` as well as `lang` and
only defaults a `<script>` to JavaScript when it declares neither (3); the raw-text rule above
(4); an interpolation that is not an expression, which is now kept exactly rather than reflowed
into a shape this printer invented for it (1); and the grammar template expressions are read in
(1).

That last one reads like fidelity and is not. A component says with its `<script lang>` which
grammar its template is in, and reading a plain-JavaScript component's template as TypeScript is
not a harmless superset: the two disagree about whether `foo < bar > (baz)` is a call with type
arguments or two comparisons, so it can change the layout of valid JavaScript.

Both `lang` and `type` follow Prettier's JavaScript truthiness, where `lang=""` declares nothing at
all — getting that wrong turned a plain `<script lang="">` into an unformattable one, which the
same conformance run caught in the same minute.

**A Tailwind bug this work found and fixed, in the layer every language shares.** Merging an
embedded child's IR used to renumber its `TailwindClass` indices into the parent's space, and that
rewrite could not reach inside an `Interned` subtree — those are shared arena slices with no owner
to rewrite through. With `sortTailwindcss` on, any component holding a Tailwind function call hit
it: debug builds panicked, release builds silently printed the wrong classes. It was **not** new
and not Vue's — the same input in a `.svelte` file panicked identically — but moving `.vue` onto
the native printer would have given it a second host.

The fix is the one `TailwindCollector`'s own doc comment described as deferred: the class space
now lives on the `FormatSession`, beside the `GroupId` space it exactly parallels, so a child
allocates the parent's indices directly and there is no renumbering left to fall short. The
`EmbeddedIr` / `DispatchPayload` class fields, the remap, its `debug_assert` and the
`TailwindCollector` trait are all gone with it.

**One known deviation, and it is in the shared printer, not in this crate.** `oxc_formatter_core`
records a group's print mode while _measuring_ whether an earlier group fits; Prettier's `fits`
never writes to its group-mode map. A conditional keyed on a not-yet-printed group therefore
resolves to "broken" here and to "flat" in Prettier, which can move a line break to a different —
equally valid, identically rendering — position. It cost one corpus file before the hug layout
was settled in advance where the tag provably cannot break (`element.rs`); it is not otherwise
worked around, because the fix belongs in the printer every language shares.

Note that finishing Vue does _not_ by itself remove Prettier from `oxfmt`'s bundle: Markdown,
HTML, Angular and Handlebars still need it.

**End to end on one production repo, at that repo's own config.** The corpus figure above uses
Prettier's defaults for every file, which is the weaker claim: it shows the printer matches
Prettier-at-defaults, not that switching a real project would be a no-op. Re-run against
`chatlyn-ui`'s own `.prettierrc.mjs`, resolved per directory the way Prettier resolves it —
which matters, because `apps/webchat` has a second config setting `printWidth: 120` and
`vueIndentScriptAndStyle: true` — all **891 / 891 files are byte-identical**, and formatting
twice changes nothing.

Two things that only showed up by doing it that way, both worth keeping in mind for the next
repo:

- A single flat scratch directory scored 93.6%, and 11 of those 57 differences were the harness
  applying one `printWidth` to files whose own config sets another. **Match the config
  per-directory, not per-run.**
- Against the Prettier the repo actually has pinned — 3.8.3 — four files differ, all one
  TypeScript union layout. That is not a defect: Prettier changed it in 3.9, and this fork's
  union printer deliberately follows 3.9 (see the header of
  `crates/oxc_formatter/src/print/union_type.rs`). Prettier 3.9.6 produces exactly what the fork
  produces. A project switching from an older Prettier will see that reformat once.

`prettier-plugin-organize-imports`, which the repo also runs, changes none of the 891 files —
its imports are already ordered — so it is not a blocker here, though a repo where it does reorder
would need `oxfmt`'s own import sorting turned on.

**On idempotency:** three files in the 1,602 change on a second pass. All three change under
Prettier too, at the same lines, in the same direction, and this printer's second pass is
byte-identical to Prettier's second pass. So the instability is Prettier's own layout, faithfully
reproduced, rather than something to fix here.

The first run of this measurement said 20.8%, and the difference was not the printer: `oxfmt`
defaults to `printWidth` 100 and Prettier to 80, so the two were formatting to different widths.
That is the same trap recorded under [oxfmt in general](#oxfmt-in-general); it is worth
re-reading before trusting any formatter comparison — and the flat-directory mistake above is the
same trap wearing a different hat.

Verified on three production repos — 2,059, 2,735 and 2,530 files — with **zero** formatting
differences against Prettier 3.9.6.

### The Vue printer against an open-source corpus

The 1,602 files above are one organisation's code, which is the uniformity this file keeps warning
about. So the Svelte check was run again for Vue, over **5,245 `.vue` files in six open-source
repositories**, each formatted under its own repo's Prettier config resolved per file:

| Repository                  | Files |       Identical |
| :-------------------------- | ----: | --------------: |
| `primefaces/primevue`       |  2615 |   2615 (100.0%) |
| `element-plus/element-plus` |  1008 |   1008 (100.0%) |
| `nuxt/ui`                   |   731 |    729 (100.0%) |
| `vbenjs/vue-vben-admin`     |   692 |    692 (100.0%) |
| `vuejs/vitepress`           |   100 |     99 (100.0%) |
| `epicmaxco/vuestic-admin`   |    99 |     99 (100.0%) |
| **Total**                   |  5245 | **5242 (100%)** |

Three files are missing from that count because **Prettier cannot parse them**, and what happens to
those is the more interesting half:

- Two are Prettier bugs. `{{ 'nuxt-ui make locale --code <code>' }}` is an interpolation whose
  string contains `<code>`, and Prettier's HTML tokenizer reads it as a tag; the other is a `slot`
  it decides was already closed. This printer formats both correctly.
- One is a `.vitepress/template/` scaffolding file whose script tag is an EJS expression —
  `<script setup<%= useTs ? ' lang="ts"' : '' %>>`. Prettier refuses it. **This printer formatted
  it, and corrupted it**: the EJS parsed as attributes and came back with an invented `=""` and its
  final `>` on a line of its own. A `<` inside a tag is not markup — the HTML tokenizer reports one
  as a parse error — so the component is now refused, the same answer the Svelte printer gives to a
  parse it had to recover.

The first run of the differential was 5,238 of 5,242, and all four differences were one class: an
interface whose heritage clause carries a comment. `interface A extends /** @vue-ignore */ Omit<…>`
broke before `extends` where Prettier keeps the clause on the name's line and breaks the type
arguments. The rule counted a comment anywhere between the name and the heritage type, and the
range spans the `extends` keyword — but a comment _after_ the keyword leads the type rather than
following the name. What tells the two apart without finding the keyword is whether everything from
the name to the comment is whitespace. Nothing else in the conformance suite moved, and the Svelte
corpus stayed at 6,673.

## The JS/TS printer against an open-source corpus

Svelte and Vue both got a real-world differential; plain JS/TS is the surface underneath both and
had only Prettier's own fixture suites (773/810 js, 640/659 ts). So the same check was run over
**8,211 `.js`/`.ts`/`.tsx`/… files in six open-source repositories** — vite, astro, TanStack Query,
NestJS, axios and vue core — each formatted under its own resolved Prettier config:

| Repository        | Comparable |         Identical |
| :---------------- | ---------: | ----------------: |
| `withastro/astro` |       2881 |   2881 (**100%**) |
| `nestjs/nest`     |       1904 |   1904 (**100%**) |
| `vitejs/vite`     |       1560 |  1559 (**99.9%**) |
| `TanStack/query`  |       1091 |  1088 (**99.7%**) |
| `vuejs/core`      |        527 |    527 (**100%**) |
| `axios/axios`     |        242 |    242 (**100%**) |
| **Total**         |       8205 | **8201 (99.95%)** |

Six files are excluded because Prettier cannot parse them; oxfmt refuses none of the 8,211.
`.prettierignore` is deliberately _not_ honoured — astro's ignores every `.ts` in the repo because
it formats with Biome instead, and dropping those would have removed a third of the corpus for a
reason that has nothing to do with whether two formatters agree. Only generated and vendored trees
are skipped, by directory name.

The first run was 8,177, and the differences fell into three classes worth fixing:

- **A union inside parentheses expanded one member per line.** `(A | B | C)[]` and `(A | B) & {}`
  came back with a leading `|` per member where Prettier keeps the members on one line between the
  broken parentheses. The members need a group of their own inside the parentheses' break — the
  same split the unparenthesized path already made, and for the same reason. Six files, and
  Prettier's `typescript/union/union-parens.ts` went from 97.7% to 99.1%.
- **A conditional type's branch broke after the `?`.** `? | keyof O` came back as `?` then the
  union indented under it. A branch's union takes no indent of its own: the conditional already
  aligns both branches two columns past the operator. Four files, and
  `typescript/union/consistent-with-flow/conditional.ts` went from 54.5% to **passing**.
- **A negated clause hugged its parentheses past a leading comment** — `if (// why\n!(a || b))`.
  That one was this fork's own, introduced with the clause-hug port: Prettier refuses to hug any
  test carrying a comment, leading ones included, and the guard only looked inside the test's span.
  `js/if/condition-break/unary-expression.js` went from 87.5% to 91.2%.

That run left seventeen files, and working through them fixed ten more classes. Four were the
fork's own idea of what Prettier does rather than Prettier's:

- **Vitest's test-call spellings.** Upstream oxc extends Prettier's `testCallCalleePatterns` with
  `bench`, `Deno.test`, `.skipIf`, `.runIf`, `.concurrent`, `.sequential`, `.todo`, `.fails`,
  `.extend` and `.shuffle` (`feat(formatter): Support Vitest test functions`). That is the better
  default for a vitest project and a layout difference a project migrating off Prettier would see;
  this fork exists so that migration is byte-identical, so the list is Prettier's. Three files.
- **A cast in `new` callee position took no group of its own.** Prettier once left `new` out of
  that test (prettier#18406) and the code carried a note saying so; as of 3.9.6 it does not, so
  `new (X[Y] as any)()` breaks at the cast's parentheses rather than inside the member expression.
  Prettier's own `typescript/cast/18406.ts` — named for the issue — now passes, and ts conformance
  went 639 → **640/659**.
- **A JSDoc `@type` cast is keyed on the parser, not the language.** Prettier keeps the
  parentheses under `babel` and `babel-ts` and drops them under `typescript`, and every embedded
  fragment goes to a babel parser — so a `.ts` file drops them, a `<script lang="ts">` block drops
  them, and a `{…}` in Svelte markup or a `{{…}}` in a Vue template keeps them even when the
  component declares `lang="ts"`. Keying it on the source type instead silently cost the Svelte
  corpus two files, because markup expressions are parsed as TypeScript whatever the script says.
- **The assignment layout tested its left-hand-side rules too early.** Prettier's `chooseLayout`
  tests `isComplexDestructuringTarget` before the operator but the complex-annotation and
  arrow-declarator rules after it, so an own-line comment after the `=` beats an annotated arrow.

The other six were ordinary porting gaps: an arrow chain indents its whole signature list rather
than each signature after the first (which left a breaking _first_ signature's parameters a level
short); a type assertion hugs an expression that breaks, as Prettier's `conditionalGroup` does
when the group is already marked broken; a `return`'s trailing **line** comment is deferred to the
end of the line rather than measured into the statement, which ASI can put lines away from the
comment; a `prettier-ignore`d variable declaration still takes its semicolon; parentheses are kept
only around a _constrained_ `infer`; and a block comment's first line is no longer trimmed.

**Four files are left.** One is not a difference: `vitejs/vite`'s `create-vite/src/index.ts`
carries `// oxfmt-ignore`, which Prettier does not recognise, so the two disagree by design. Of the
rest, two are the same `vi.fn(function (this: X) {…})` call-argument layout — Prettier hugs the
call and breaks the function's parameters where this printer breaks the argument list, and telling
the two apart needs a real port of how `printArgumentsList`'s `conditionalGroup` interacts with the
object-property context rather than a guessed rule. The last has a diagnosis and no fix:

- A complex-parameter type alias with a union right-hand side, at _exactly_ 80 columns. This writes
  `" = "` where Prettier writes `" ="` and a `line`, because here the union owns its indent and in
  Prettier the assignment does. Inverting that ownership was tried and reverted: it costs
  `typescript/union/inlining.ts` and two `single-type` fixtures, ts 640 → 638.

The css-in-js interpolation that used to be the fifth is fixed, and it was two faults at once. A
value whose components all join without a break built a fill holding exactly one entry, and a
one-entry fill is not free: the entry's fit is measured on its own, without whatever shares its
line after the value — here the `;` — so a declaration one column over the margin stayed flat at
81 columns. That isolation is wanted for a sass interpolation and wrong otherwise. The
interpolation then broke at the wrong column, because Prettier anchors an embedded `${expr}` to
the column its line starts at _in the source_ (`addAlignmentToDoc` dedents to the root and rebuilds
it) rather than letting the CSS printer's own indent stack on top.

That anchoring is the one place this fork now deliberately reproduces a Prettier 3.9.6 wart: an
interpolation written far out stays far out for a pass, so re-formatting moves it again. 3.9.6 is
unstable there in exactly the same way, prettier/prettier#19725 drops the anchoring on Prettier
main, and `template-expression-indent.js` records which way it goes and what flips it back. Every
figure in this file is measured against 3.9.6, and a real file needs it. css-in-js conformance
went 19/21 → **20/21**.

## oxlint in general

1,029 rules across 18 plugins, 157 more than upstream at the same merge base (the entire `svelte`
plugin, the `nuxt` plugin, and 72 `vue` rules).

| plugin       | rules |     | plugin     | rules |
| :----------- | ----: | :-- | :--------- | ----: |
| `eslint`     |   187 |     | `jest`     |    60 |
| `unicorn`    |   138 |     | `jsx_a11y` |    36 |
| `typescript` |   110 |     | `import`   |    33 |
| `vue`        |   118 |     | `oxc`      |    27 |
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

| Language                                     | Implementation                  |
| :------------------------------------------- | :------------------------------ |
| JS, TS, JSX, TSX                             | `oxc_formatter`                 |
| JSON, JSONC, `package.json`                  | `oxc_formatter_json`            |
| CSS, SCSS, Less                              | `oxc_formatter_css`             |
| YAML                                         | `oxc_formatter_yaml`            |
| GraphQL                                      | `oxc_formatter_graphql`         |
| **Svelte**                                   | `oxc_formatter_svelte`          |
| **Vue**                                      | `oxc_formatter_vue`             |
| TOML                                         | taplo (Rust)                    |
| **HTML, Angular, Markdown, MDX, Handlebars** | delegated to Prettier over NAPI |

Prettier is still bundled inside `oxfmt`'s own `dist/` for that last row, so a project's manifest
can be Prettier-free even where Prettier code still runs. Removing the bundle means writing a
native Markdown printer.

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

The native Vue printer and `attributes-order` both used to head this list, and both are done. What
is left, in the order it costs the most:

1. **Native Markdown**, and after it HTML, Angular and Glimmer — the four languages still routed to
   Prettier, and the reason `prettier` is still a runtime dependency of `oxfmt` rather than a
   build-time one. Markdown is the one that matters: nearly every repository has `.md` files, so
   in practice it is what keeps the Prettier sidecar loading at all.
2. **The Tailwind class sorter.** Even with all four languages ported, `prettier-plugin-tailwindcss`
   still pulls Prettier in for anyone who turns class sorting on. Porting the printer without it
   moves the dependency rather than removing it.
3. `svelte/valid-compile` as a first-party JS plugin — 1 of the 3 absent Svelte rules, and the only
   one worth building.
4. `vue/no-undef-components` and `vue/no-multiple-template-root`, the two named gap rules still
   absent. No stock config enables either, which is why they have stayed at the bottom.
5. The four conformance differences that are _not_ recorded as deliberate: `Xxx.extend` unrecognised
   as a styled-components tag (css-in-js), an own-line comment Prettier moves and this does not
   (gql-in-js), `<!-- #endregion -->` after a hoisted `<script>`/`<style>` (svelte), and one SCSS
   long-expression break position. Everything else in the suite is annotated as allowed,
   layout-only, a reduced port, or a Prettier 3.9.6 bug already fixed on Prettier main.

One test failure is known and pre-existing on `main`, unrelated to any of the above:
`oxlint::lsp::server_linter::test::test_frameworks`. The `lint::suppression` tests that drive
`oxlint-tsgolint` also fail locally, there because the spawned binary takes a `SIGPIPE`.

`oxlint::lint::test::lint_svelte_file` used to be listed here too, on the reading that
`eslint/no-unassigned-vars` had "stopped reporting" on `export let` in a `.svelte` file. That was
wrong: 51b5719e6d deliberately made the rule skip `.svelte`/`.vue`, because the template can assign
a binding through `bind:this`/`bind:value`/`v-model` and the script pass cannot see it — the same
guard, and the same reason, as `prefer-const`. The rule was right and the CLI snapshot was simply
never regenerated. It has been.
