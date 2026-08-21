## js-in-vue

### Option 1: 428/428 (100.00%)

```json
{"printWidth":80}
```

### Option 2: 428/428 (100.00%)

```json
{"printWidth":100,"vueIndentScriptAndStyle":true,"singleQuote":true}
```

## gql-in-js

### Option 1: 11/13 (84.62%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [edge-cases/gql-in-js/template-expression-indent.js](diffs/gql-in-js/edge-cases__gql-in-js__template-expression-indent.js.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/prettier/js/multiparser-graphql/graphql-tag.js](diffs/gql-in-js/externals__prettier__js__multiparser-graphql__graphql-tag.js.md) | Prettier moves `query Test { # c` own-line comment to next line, we keep |

### Option 2: 11/13 (84.62%)

```json
{"printWidth":100}
```

| File | Note |
| :--- | :--- |
| [edge-cases/gql-in-js/template-expression-indent.js](diffs/gql-in-js/edge-cases__gql-in-js__template-expression-indent.js.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/prettier/js/multiparser-graphql/graphql-tag.js](diffs/gql-in-js/externals__prettier__js__multiparser-graphql__graphql-tag.js.md) | Prettier moves `query Test { # c` own-line comment to next line, we keep |

## css-in-js

### Option 1: 19/21 (90.48%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [edge-cases/css-in-js/template-expression-indent.js](diffs/css-in-js/edge-cases__css-in-js__template-expression-indent.js.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/prettier/js/multiparser-css/styled-components.js](diffs/css-in-js/externals__prettier__js__multiparser-css__styled-components.js.md) | `Xxx.extend` not recognized as tag |

### Option 2: 19/21 (90.48%)

```json
{"printWidth":100}
```

| File | Note |
| :--- | :--- |
| [edge-cases/css-in-js/template-expression-indent.js](diffs/css-in-js/edge-cases__css-in-js__template-expression-indent.js.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/prettier/js/multiparser-css/styled-components.js](diffs/css-in-js/externals__prettier__js__multiparser-css__styled-components.js.md) | `Xxx.extend` not recognized as tag |

## html-in-js

### Option 1: 188/194 (96.91%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [edge-cases/html-in-js/template-expression-indent.js](diffs/html-in-js/edge-cases__html-in-js__template-expression-indent.js.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/carousel/carousel.ts](diffs/html-in-js/externals__webawesome__carousel__carousel.ts.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/color-picker/color-picker.ts](diffs/html-in-js/externals__webawesome__color-picker__color-picker.ts.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/input/input.ts](diffs/html-in-js/externals__webawesome__input__input.ts.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/number-input/number-input.styles.ts](diffs/html-in-js/externals__webawesome__number-input__number-input.styles.ts.md) | Layout-only: Prettier's fill fit-check breaks inside `var()` args in a long `calc()`; ours breaks after the operator. See crates/oxc_formatter_css/AGENTS.md |
| [externals/webawesome/page/page.styles.ts](diffs/html-in-js/externals__webawesome__page__page.styles.ts.md) | Layout-only: Prettier's fill fit-check breaks inside `::slotted()` after a long `:not(...)`; ours breaks inside `:not(...)`. See crates/oxc_formatter_css/AGENTS.md |

### Option 2: 190/194 (97.94%)

```json
{"printWidth":100,"htmlWhitespaceSensitivity":"ignore"}
```

| File | Note |
| :--- | :--- |
| [edge-cases/html-in-js/template-expression-indent.js](diffs/html-in-js/edge-cases__html-in-js__template-expression-indent.js.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/carousel/carousel.ts](diffs/html-in-js/externals__webawesome__carousel__carousel.ts.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/color-picker/color-picker.ts](diffs/html-in-js/externals__webawesome__color-picker__color-picker.ts.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |
| [externals/webawesome/input/input.ts](diffs/html-in-js/externals__webawesome__input__input.ts.md) | We match Prettier main (prettier/prettier#19725); 3.9.6 still preserves source indent non-idempotently |

## angular-in-js

### Option 1: 7/7 (100.00%)

```json
{"printWidth":80}
```

### Option 2: 7/7 (100.00%)

```json
{"printWidth":100,"htmlWhitespaceSensitivity":"ignore"}
```

## md-in-js

### Option 1: 8/8 (100.00%)

```json
{"printWidth":80}
```

### Option 2: 8/8 (100.00%)

```json
{"printWidth":100,"proseWrap":"always"}
```

## xxx-in-js-comment

### Option 1: 4/5 (80.00%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [externals/prettier/js/multiparser-comments/comment-inside.js](diffs/xxx-in-js-comment/externals__prettier__js__multiparser-comments__comment-inside.js.md) | Broken `${}` holding comments: Prettier prints the expression at root indent (drops the embed indent), we indent to the placeholder |

### Option 2: 4/5 (80.00%)

```json
{"printWidth":100}
```

| File | Note |
| :--- | :--- |
| [externals/prettier/js/multiparser-comments/comment-inside.js](diffs/xxx-in-js-comment/externals__prettier__js__multiparser-comments__comment-inside.js.md) | Broken `${}` holding comments: Prettier prints the expression at root indent (drops the embed indent), we indent to the placeholder |

## svelte

### Option 1: 77/81 (95.06%)

```json
{"printWidth":80,"svelte":{}}
```

| File | Note |
| :--- | :--- |
| [externals/plugin-svelte/declaration-tag.svelte](diffs/svelte/externals__plugin-svelte__declaration-tag.svelte.md) | Reduced port: `{let a = 1, b = 2}` keeps its spelling. A declaration tag's single declarator is formatted through the expression path, where two of them would come back as the sequence expression `(a = 1), (b = 2)` — a different declaration that does not parse as one. See crates/oxc_formatter_svelte/AGENTS.md |
| [externals/plugin-svelte/each-await-block-destructuring.svelte](diffs/svelte/externals__plugin-svelte__each-await-block-destructuring.svelte.md) | Reduced port: an `{#each … as PATTERN}` / `{:then PATTERN}` binding keeps its spelling. Prettier re-serializes it with a bespoke pattern printer (`expandNode`) that preserves literal spelling and never breaks; the fragment path here would reach it through the estree printer, which does neither. Canonical spacing already matches. See crates/oxc_formatter_svelte/AGENTS.md |
| [externals/plugin-svelte/long-mustache-value.svelte](diffs/svelte/externals__plugin-svelte__long-mustache-value.svelte.md) | Layout-only: a `{…}` whose expression breaks continues at the mustache's indent, where Prettier adds one level. Prettier prints the expression as a real estree node (whose unknown parent makes `printBinaryishExpression` indent); this goes through the JS *fragment* path, which does not. Never changes meaning |
| [externals/plugin-svelte/region-markers.svelte](diffs/svelte/externals__plugin-svelte__region-markers.svelte.md) | Not implemented: a `<!-- #endregion -->` immediately after a hoisted `<script>`/`<style>` travels with it when sections are reordered (Prettier's `extractRegionEndTrailAfterHoistedEnd`). The *leading* comment does travel; only the trailing marker does not |

### Option 2: 77/81 (95.06%)

```json
{"printWidth":120,"singleQuote":true,"htmlWhitespaceSensitivity":"ignore","bracketSameLine":true,"svelteIndentScriptAndStyle":true,"svelteSortOrder":"options-scripts-styles-markup","svelte":{"indentScriptAndStyle":true,"sortOrder":"options-scripts-styles-markup"}}
```

| File | Note |
| :--- | :--- |
| [externals/plugin-svelte/declaration-tag.svelte](diffs/svelte/externals__plugin-svelte__declaration-tag.svelte.md) | Reduced port: `{let a = 1, b = 2}` keeps its spelling. A declaration tag's single declarator is formatted through the expression path, where two of them would come back as the sequence expression `(a = 1), (b = 2)` — a different declaration that does not parse as one. See crates/oxc_formatter_svelte/AGENTS.md |
| [externals/plugin-svelte/each-await-block-destructuring.svelte](diffs/svelte/externals__plugin-svelte__each-await-block-destructuring.svelte.md) | Reduced port: an `{#each … as PATTERN}` / `{:then PATTERN}` binding keeps its spelling. Prettier re-serializes it with a bespoke pattern printer (`expandNode`) that preserves literal spelling and never breaks; the fragment path here would reach it through the estree printer, which does neither. Canonical spacing already matches. See crates/oxc_formatter_svelte/AGENTS.md |
| [externals/plugin-svelte/long-mustache-value.svelte](diffs/svelte/externals__plugin-svelte__long-mustache-value.svelte.md) | Layout-only: a `{…}` whose expression breaks continues at the mustache's indent, where Prettier adds one level. Prettier prints the expression as a real estree node (whose unknown parent makes `printBinaryishExpression` indent); this goes through the JS *fragment* path, which does not. Never changes meaning |
| [externals/plugin-svelte/region-markers.svelte](diffs/svelte/externals__plugin-svelte__region-markers.svelte.md) | Not implemented: a `<!-- #endregion -->` immediately after a hoisted `<script>`/`<style>` travels with it when sections are reordered (Prettier's `extractRegionEndTrailAfterHoistedEnd`). The *leading* comment does travel; only the trailing marker does not |

## svelte-in-md

### Option 1: 2/2 (100.00%)

```json
{"printWidth":80,"svelte":{}}
```

### Option 2: 2/2 (100.00%)

```json
{"printWidth":100,"proseWrap":"always","svelte":{}}
```

## graphql

### Option 1: 712/712 (100.00%)

```json
{"printWidth":80}
```

### Option 2: 712/712 (100.00%)

```json
{"printWidth":100}
```

## less

### Option 1: 403/409 (98.53%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [externals/ng-zorro-antd/components/style/themes/compact.less](diffs/less/externals__ng-zorro-antd__components__style__themes__compact.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/style/themes/dark.less](diffs/less/externals__ng-zorro-antd__components__style__themes__dark.less.md) | Allowed: trailing `// comment` rides a line_suffix, never counts toward print width; Prettier only treats CSS-family `//` inline and breaks the value. See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/style/themes/default.less](diffs/less/externals__ng-zorro-antd__components__style__themes__default.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md<br>Allowed: trailing `// comment` rides a line_suffix, never counts toward print width; Prettier only treats CSS-family `//` inline and breaks the value. See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/style/themes/variable.less](diffs/less/externals__ng-zorro-antd__components__style__themes__variable.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md<br>Allowed: trailing `// comment` rides a line_suffix, never counts toward print width; Prettier only treats CSS-family `//` inline and breaks the value. See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/table/style/index.less](diffs/less/externals__ng-zorro-antd__components__table__style__index.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/table/style/rtl.less](diffs/less/externals__ng-zorro-antd__components__table__style__rtl.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md |

### Option 2: 406/409 (99.27%)

```json
{"printWidth":100}
```

| File | Note |
| :--- | :--- |
| [externals/ng-zorro-antd/components/style/themes/default.less](diffs/less/externals__ng-zorro-antd__components__style__themes__default.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md<br>Allowed: trailing `// comment` rides a line_suffix, never counts toward print width; Prettier only treats CSS-family `//` inline and breaks the value. See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/style/themes/variable.less](diffs/less/externals__ng-zorro-antd__components__style__themes__variable.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md<br>Allowed: trailing `// comment` rides a line_suffix, never counts toward print width; Prettier only treats CSS-family `//` inline and breaks the value. See crates/oxc_formatter_css/AGENTS.md |
| [externals/ng-zorro-antd/components/table/style/rtl.less](diffs/less/externals__ng-zorro-antd__components__table__style__rtl.less.md) | Allowed (layout-only): nested Less math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md |

## css

### Option 1: 221/221 (100.00%)

```json
{"printWidth":80}
```

### Option 2: 221/221 (100.00%)

```json
{"printWidth":100}
```

## yaml

### Option 1: 301/302 (99.67%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [externals/aws-cloudformation-templates/RainModules/load-balancer.yml](diffs/yaml/externals__aws-cloudformation-templates__RainModules__load-balancer.yml.md) | Allowed: over-indented comment after `key: value` (Prettier breaks the pair onto two lines because of comment indentation). See crates/oxc_formatter_yaml/AGENTS.md |

### Option 2: 301/302 (99.67%)

```json
{"printWidth":100,"tabWidth":4,"proseWrap":"always"}
```

| File | Note |
| :--- | :--- |
| [externals/aws-cloudformation-templates/RainModules/load-balancer.yml](diffs/yaml/externals__aws-cloudformation-templates__RainModules__load-balancer.yml.md) | Allowed: over-indented comment after `key: value` (Prettier breaks the pair onto two lines because of comment indentation). See crates/oxc_formatter_yaml/AGENTS.md |

### Option 3: 301/302 (99.67%)

```json
{"printWidth":120,"singleQuote":true,"bracketSpacing":false,"trailingComma":"none"}
```

| File | Note |
| :--- | :--- |
| [externals/aws-cloudformation-templates/RainModules/load-balancer.yml](diffs/yaml/externals__aws-cloudformation-templates__RainModules__load-balancer.yml.md) | Allowed: over-indented comment after `key: value` (Prettier breaks the pair onto two lines because of comment indentation). See crates/oxc_formatter_yaml/AGENTS.md |

## scss

### Option 1: 203/217 (93.55%)

```json
{"printWidth":80}
```

| File | Note |
| :--- | :--- |
| [externals/gitlab/stylesheets/components/content_editor.scss](diffs/scss/externals__gitlab__stylesheets__components__content_editor.scss.md) | Allowed (layout-only): `box-shadow` with `#{}` math — Prettier's fill fit-check breaks inside the wide chunk, ours breaks the separator (biome fill). See crates/oxc_formatter_css/AGENTS.md |
| [externals/gitlab/stylesheets/framework/diffs.scss](diffs/scss/externals__gitlab__stylesheets__framework__diffs.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/framework/variables_overrides.scss](diffs/scss/externals__gitlab__stylesheets__framework__variables_overrides.scss.md) | Allowed (semantics): Prettier adds a trailing comma to non-comma-list map-item parens (`1: ($spacer * 0.5)` → 1-element list); we keep them inline. See crates/oxc_formatter_css/AGENTS.md |
| [externals/gitlab/stylesheets/highlight/conflict_colors.scss](diffs/scss/externals__gitlab__stylesheets__highlight__conflict_colors.scss.md) | Allowed: Prettier drops blank lines in SCSS maps with paren values; ours preserves (prettier/prettier#16824) |
| [externals/gitlab/stylesheets/page_bundles/_ide_theme_overrides.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles___ide_theme_overrides.scss.md) | Layout-only: Prettier's fill fit-check breaks inside `var()` args in a long `calc()`; ours breaks after the operator. See crates/oxc_formatter_css/AGENTS.md |
| [externals/gitlab/stylesheets/page_bundles/editor.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__editor.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/environments.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__environments.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/issuable_list.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__issuable_list.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/labels.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__labels.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/merge_requests.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__merge_requests.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/projects.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__projects.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/settings.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__settings.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/pages/profile.scss](diffs/scss/externals__gitlab__stylesheets__pages__profile.scss.md) | Allowed: trailing `// comment` rides a line_suffix, never counts toward print width; Prettier only treats CSS-family `//` inline and breaks the value. See crates/oxc_formatter_css/AGENTS.md |
| [externals/gitlab/stylesheets/pages/settings.scss](diffs/scss/externals__gitlab__stylesheets__pages__settings.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |

### Option 2: 204/217 (94.01%)

```json
{"printWidth":100}
```

| File | Note |
| :--- | :--- |
| [externals/gitlab/stylesheets/framework/diffs.scss](diffs/scss/externals__gitlab__stylesheets__framework__diffs.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/framework/sidebar.scss](diffs/scss/externals__gitlab__stylesheets__framework__sidebar.scss.md) | long-expr line-break position |
| [externals/gitlab/stylesheets/framework/variables_overrides.scss](diffs/scss/externals__gitlab__stylesheets__framework__variables_overrides.scss.md) | Allowed (semantics): Prettier adds a trailing comma to non-comma-list map-item parens (`1: ($spacer * 0.5)` → 1-element list); we keep them inline. See crates/oxc_formatter_css/AGENTS.md |
| [externals/gitlab/stylesheets/highlight/conflict_colors.scss](diffs/scss/externals__gitlab__stylesheets__highlight__conflict_colors.scss.md) | Allowed: Prettier drops blank lines in SCSS maps with paren values; ours preserves (prettier/prettier#16824) |
| [externals/gitlab/stylesheets/page_bundles/_ide_theme_overrides.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles___ide_theme_overrides.scss.md) | Layout-only: Prettier's fill fit-check breaks inside `var()` args in a long `calc()`; ours breaks after the operator. See crates/oxc_formatter_css/AGENTS.md |
| [externals/gitlab/stylesheets/page_bundles/editor.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__editor.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/environments.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__environments.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/issuable_list.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__issuable_list.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/labels.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__labels.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/merge_requests.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__merge_requests.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/projects.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__projects.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/page_bundles/settings.scss](diffs/scss/externals__gitlab__stylesheets__page_bundles__settings.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
| [externals/gitlab/stylesheets/pages/settings.scss](diffs/scss/externals__gitlab__stylesheets__pages__settings.scss.md) | Allowed: media-query operator spacing; Prettier can't space arithmetic ops (prettier/prettier#1811) |
