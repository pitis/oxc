//! Vue `<template>` linting pass.
//!
//! The loader extracts only `<script>` blocks from `.vue` files, so the
//! script sub-host machinery never sees the template. This pass parses the
//! `<template>` block(s) with `oxc_vue_parser` and runs the template-capable
//! rules of the `vue` plugin over the resulting AST.
//!
//! It is deliberately independent of the sub-host loop:
//! - spans are absolute file offsets (`parse_template` is handed the block's
//!   `content_span.start` as its `base_offset`, so the AST it returns is
//!   already file-relative), and
//! - it also covers `.vue` files that have no `<script>` block at all,
//!   which the sub-host loop skips entirely.
//!
//! Suppression works on three fronts, mirroring eslint-plugin-vue:
//! - `<!-- eslint-disable -->`-style HTML comment directives *inside* a
//!   `<template>` block, via [`TemplateCommentDirectives`] (see there for the
//!   reproduced semantics);
//! - the same directives written at the *top level* of the file (outside every
//!   block), which upstream's `vue/comment-directive` rule also collects (its
//!   `extractTopLevelDocumentFragmentComments`) and which are the only way to
//!   silence a `VueSfcRule` that reports at the very start of the file;
//! - `/* eslint-disable … */` comments inside a `<script>` block, applied by
//!   [`filter_by_script_directives`] to the messages this pass anchors inside
//!   that script.
//!
//! Not yet supported: fixes.

use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_span::Span;
use oxc_vue_parser::{Sfc, ast::Comment, ast::Node, parse_sfc, parse_template};
use rustc_hash::FxHashMap;

use crate::{
    AllowWarnDeny, Linter,
    context::ContextSubHost,
    fixer::{Message, MessageRule, PossibleFixes},
    rules::RuleEnum,
};

/// A rule that lints the parsed `<template>` AST of a `.vue` file.
///
/// Implemented in addition to (not instead of) the [`crate::rule::Rule`]
/// trait: the rule registers, resolves, and configures like any other rule of
/// the `vue` plugin; its `Rule` impl is empty and this trait carries the
/// template logic.
pub trait VueTemplateRule {
    /// `nodes` are the root nodes of one `<template>` block; their spans are
    /// absolute file offsets, and index into `ctx.source_text()` (the whole
    /// `.vue` file). Report through `ctx` with those same spans.
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>);
}

/// A rule that lints the whole `.vue` SFC (its blocks, not a single
/// `<template>`'s parsed content) — e.g. rules that check the relationship
/// between `<template>` and `<script>` blocks, or block-level structure.
///
/// Runs once per file, not once per `<template>` block: a `.vue` file with no
/// `<template>` block at all still runs its `VueSfcRule`s.
///
/// [`Sfc`] block spans (`SfcBlock::span`, `SfcBlock::content_span`) are
/// file-absolute, like [`VueTemplateRule`]'s node spans, and `ctx` is the
/// same: `ctx.source_text()` is the full file source and reported spans must
/// be file-absolute.
pub trait VueSfcRule {
    fn run_on_sfc<'a>(&self, sfc: &Sfc<'a>, path: &Path, ctx: &mut VueTemplateContext<'a>);
}

/// The subset of the resolved rule set that participates in the template pass.
fn as_vue_template_rule(rule: &RuleEnum) -> Option<&dyn VueTemplateRule> {
    match rule {
        RuleEnum::VueRequireVForKey(rule) => Some(rule),
        RuleEnum::VueNoDuplicateAttributes(rule) => Some(rule),
        RuleEnum::VueNoTemplateKey(rule) => Some(rule),
        RuleEnum::VueNoTextareaMustache(rule) => Some(rule),
        RuleEnum::VueRequireComponentIs(rule) => Some(rule),
        RuleEnum::VueNoLoneTemplate(rule) => Some(rule),
        RuleEnum::VueNoUselessTemplateAttributes(rule) => Some(rule),
        RuleEnum::VueNoVForTemplateKeyOnChild(rule) => Some(rule),
        RuleEnum::VueNoChildContent(rule) => Some(rule),
        RuleEnum::VueValidVIf(rule) => Some(rule),
        RuleEnum::VueValidVElse(rule) => Some(rule),
        RuleEnum::VueValidVElseIf(rule) => Some(rule),
        RuleEnum::VueValidVShow(rule) => Some(rule),
        RuleEnum::VueValidVCloak(rule) => Some(rule),
        RuleEnum::VueValidVOnce(rule) => Some(rule),
        RuleEnum::VueValidVPre(rule) => Some(rule),
        RuleEnum::VueValidVHtml(rule) => Some(rule),
        RuleEnum::VueValidVText(rule) => Some(rule),
        RuleEnum::VueValidVBind(rule) => Some(rule),
        RuleEnum::VueValidVOn(rule) => Some(rule),
        RuleEnum::VueValidVFor(rule) => Some(rule),
        RuleEnum::VueValidVMemo(rule) => Some(rule),
        RuleEnum::VueValidVIs(rule) => Some(rule),
        RuleEnum::VueValidVModel(rule) => Some(rule),
        RuleEnum::VueValidVSlot(rule) => Some(rule),
        RuleEnum::VueValidAttributeName(rule) => Some(rule),
        RuleEnum::VueNoParsingError(rule) => Some(rule),
        RuleEnum::VueNoVHtml(rule) => Some(rule),
        RuleEnum::VueNoVTextVHtmlOnComponent(rule) => Some(rule),
        RuleEnum::VueUseVOnExact(rule) => Some(rule),
        RuleEnum::VueNoDupeVElseIf(rule) => Some(rule),
        RuleEnum::VueRequireToggleInsideTransition(rule) => Some(rule),
        RuleEnum::VueNoUseVIfWithVFor(rule) => Some(rule),
        RuleEnum::VueAttributeHyphenation(rule) => Some(rule),
        RuleEnum::VueVOnEventHyphenation(rule) => Some(rule),
        RuleEnum::VueVBindStyle(rule) => Some(rule),
        RuleEnum::VueVOnStyle(rule) => Some(rule),
        RuleEnum::VueVSlotStyle(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedScopeAttribute(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedSlotAttribute(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedSlotScopeAttribute(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedVBindSync(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedVIs(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedVOnNativeModifier(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedVOnNumberModifiers(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedHtmlElementIs(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedInlineTemplate(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedRouterLinkTagProp(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedFilter(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedDollarListenersApi(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedDollarScopedslotsApi(rule) => Some(rule),
        RuleEnum::VueThisInTemplate(rule) => Some(rule),
        _ => None,
    }
}

/// The subset of the resolved rule set that participates in the SFC pass.
fn as_vue_sfc_rule(rule: &RuleEnum) -> Option<&dyn VueSfcRule> {
    match rule {
        RuleEnum::VueValidTemplateRoot(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedFunctionalTemplate(rule) => Some(rule),
        RuleEnum::VueMultiWordComponentNames(rule) => Some(rule),
        RuleEnum::VueBlockOrder(rule) => Some(rule),
        _ => None,
    }
}

/// Reporting context handed to [`VueTemplateRule::run_on_template`].
pub struct VueTemplateContext<'a> {
    /// The whole `.vue` file source — what the AST spans index into.
    source_text: &'a str,
    diagnostics: Vec<OxcDiagnostic>,
}

impl<'a> VueTemplateContext<'a> {
    /// The whole `.vue` file source (what the AST spans are relative to).
    pub fn source_text(&self) -> &'a str {
        self.source_text
    }

    /// Report a violation. Label spans are absolute file offsets, exactly as
    /// they come off the AST.
    pub fn diagnostic(&mut self, diagnostic: OxcDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

impl Linter {
    /// Run the Vue template + SFC pass over a `.vue` file's full source text.
    ///
    /// Returns messages with file-absolute spans, shaped exactly like rule
    /// messages from the script pass (error code, docs URL, severity).
    /// Returns an empty vec for non-`.vue` paths and when no template-capable
    /// or SFC-capable rule is enabled, without parsing anything.
    pub(crate) fn run_vue_template_rules(&self, path: &Path, source_text: &str) -> Vec<Message> {
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("vue") {
            return Vec::new();
        }
        let resolved = self.config.resolve(path);
        let template_rules: Vec<_> = resolved
            .rules
            .iter()
            .filter_map(|(rule, severity)| {
                as_vue_template_rule(rule).map(|template_rule| (rule, template_rule, *severity))
            })
            .collect();
        let sfc_rules: Vec<_> = resolved
            .rules
            .iter()
            .filter_map(|(rule, severity)| {
                as_vue_sfc_rule(rule).map(|sfc_rule| (rule, sfc_rule, *severity))
            })
            .collect();
        // A `.vue` file with only SFC rules enabled (no template-capable rule)
        // must still be parsed and run — SFC rules don't depend on there
        // being a `<template>` block at all.
        if template_rules.is_empty() && sfc_rules.is_empty() {
            return Vec::new();
        }

        let mut messages = Vec::new();
        let sfc = parse_sfc(source_text);

        // Only the directive machinery needs the line table; skip building it
        // when nothing can consult it.
        let line_starts = if sfc.top_level_comments.is_empty() && template_rules.is_empty() {
            Vec::new()
        } else {
            line_start_offsets(source_text)
        };
        // File-scoped directives: the `<!-- eslint-disable … -->` comments that
        // sit outside every block. Upstream ends each of these at the end of
        // the next top-level element (its `clear` pseudo-messages), so pass the
        // block ends as the clear points.
        let document_directives = if sfc.top_level_comments.is_empty() {
            TemplateCommentDirectives::default()
        } else {
            let clear_offsets: Vec<u32> = sfc.blocks.iter().map(|block| block.span.end).collect();
            TemplateCommentDirectives::collect_top_level(
                &sfc.top_level_comments,
                &line_starts,
                &clear_offsets,
                u32::try_from(source_text.len()).unwrap_or(u32::MAX),
            )
        };

        // SFC rules run once per file, over the whole parsed `Sfc` — not once
        // per `<template>` block. `sfc`'s block spans are already
        // file-absolute (see `VueSfcRule`'s docs), so `ctx.source_text` is
        // the full file source and diagnostics are pushed with no offset.
        //
        // Only the file-scoped directives can apply here: an SFC rule anchors
        // its report at the start of the file or at a block's opening tag,
        // both of which come *before* any comment inside a `<template>`, and a
        // directive never reaches backwards.
        for (rule, sfc_rule, severity) in &sfc_rules {
            let mut ctx = VueTemplateContext { source_text, diagnostics: Vec::new() };
            sfc_rule.run_on_sfc(&sfc, path, &mut ctx);
            let mut diagnostics = ctx.diagnostics;
            retain_unsuppressed(&mut diagnostics, &[&document_directives], rule);
            push_messages(&mut messages, diagnostics, rule, *severity);
        }

        // Nothing below can report without a template rule, and the directive
        // collection below reads `line_starts`, which is only built when a
        // template rule (or a top-level comment) needs it.
        if template_rules.is_empty() {
            return messages;
        }

        for block in &sfc.blocks {
            // Only true template blocks; `<template src="...">` has no
            // inline content to lint.
            if block.name != "template" || block.attribute_value("src").is_some() {
                continue;
            }
            // A non-HTML template language (`<template lang="pug">`,
            // `lang="jade"`, `lang="haml"`, …) must not be parsed as HTML.
            // vue-eslint-parser only builds a `templateBody` when the
            // template language is HTML — for any other `lang` (absent a
            // configured `templateTokenizer` for it) it leaves `templateBody`
            // null, and `defineTemplateBodyVisitor` then registers no
            // template visitor at all, so *every* template rule is silently a
            // no-op there. Reproduce that: an explicit `lang` other than
            // `html` skips the block entirely. A missing `lang` is HTML.
            if block.lang().is_some_and(|lang| !lang.eq_ignore_ascii_case("html")) {
                continue;
            }
            // `base_offset` makes every span in `nodes` a file offset, so no
            // post-hoc shifting of the reported diagnostics is needed.
            let nodes = parse_template(block.content, block.content_span.start);
            // Directive comment spans, diagnostic spans, and therefore the
            // line table they are all resolved against, are file offsets.
            let directives =
                TemplateCommentDirectives::collect(&nodes, &line_starts, block.content_span.end);
            for (rule, template_rule, severity) in &template_rules {
                let mut ctx = VueTemplateContext { source_text, diagnostics: Vec::new() };
                template_rule.run_on_template(&nodes, &mut ctx);
                let mut diagnostics = ctx.diagnostics;
                retain_unsuppressed(&mut diagnostics, &[&document_directives, &directives], rule);
                push_messages(&mut messages, diagnostics, rule, *severity);
            }
        }
        messages
    }
}

/// The `<!-- eslint-disable … -->` comment directives of ONE `<template>`
/// block, resolved into "which rule is suppressed where".
///
/// ### Reproduced semantics
///
/// eslint-plugin-vue implements this as a two-stage trick: its
/// `vue/comment-directive` rule turns every directive comment in the
/// `templateBody` into a pseudo-*message* at the comment's own location, and
/// its processor's `postprocess` then walks the (location-sorted) message list
/// as a state machine, dropping every real message that a currently-open
/// disable covers. This type collapses that into the equivalent ranges, from
/// `dist/rules/comment-directive.js` + `dist/processor.js` (10.10.0):
///
/// - `<!-- eslint-disable -->` opens a block suppression at the comment's
///   offset; `<!-- eslint-enable -->` closes it. With a rule list
///   (`<!-- eslint-disable vue/no-v-html, vue/no-lone-template -->`) only the
///   listed rules are suppressed, and only a matching
///   `<!-- eslint-enable vue/no-v-html -->` closes those.
/// - **Quirk, faithfully reproduced:** a rule-less `eslint-enable` clears only
///   the "disable everything" state (`disableAllKeys`), NOT per-rule
///   suppressions (`disableRuleKeys`) — so `<!-- eslint-disable vue/x -->` …
///   `<!-- eslint-enable -->` leaves `vue/x` suppressed to the end of the
///   block. Symmetrically, `<!-- eslint-enable vue/x -->` does not close a
///   rule-less `eslint-disable`.
/// - `<!-- eslint-disable-line -->` suppresses the comment's own line;
///   `<!-- eslint-disable-next-line -->` the following one. (Upstream models
///   these as a disable at `{line, column: -1}` plus an enable at
///   `{line + 1, column: -1}`, which is exactly "that whole line".) Both are
///   ignored when the comment itself spans more than one line, matching
///   upstream's `comment.loc.start.line === comment.loc.end.line` guard.
/// - Everything is scoped to the block: upstream reports a `clear` pseudo
///   message at `templateBody.loc.end`, which resets all state. Building one
///   of these per `<template>` block gives that for free.
/// - A trailing `-- description` (two or more dashes, surrounded by
///   whitespace) is stripped before parsing, per `stripDirectiveComment`.
/// - Rule lists are separated by commas and/or whitespace.
/// - **Rule name matching is exact-string, full `plugin/rule` equality** —
///   not oxlint's own cross-plugin
///   [`crate::disable_directives::DisableDirectives::contains`] semantics
///   (which strips the plugin prefix from *both* sides and compares bare
///   names, deliberately letting e.g. `jest/no-only-tests` and
///   `vitest/no-only-tests` cross-suppress). Upstream's processor
///   (`dist/processor.js`) keys a `Map<string, string[]>` by the directive's
///   listed rule ID verbatim and looks it up with the *reported* message's
///   `ruleId` — which, for a plugin rule, ESLint always sets to the full
///   `"<plugin>/<rule>"` string:
///   `state.block.disableRuleKeys.get(message.ruleId)`. That is ordinary
///   `Map` key equality, so `<!-- eslint-disable vue/no-v-html -->`
///   suppresses `vue/no-v-html` and nothing else — a bare `no-v-html` does
///   NOT match (the reported `ruleId` is never bare), and a
///   different-plugin `typo/no-v-html` does NOT match (different string).
///
/// ### Deliberate additions
///
/// `oxlint-disable`/`oxlint-enable`/`oxlint-disable-line`/
/// `oxlint-disable-next-line` are accepted as synonyms of the `eslint-`
/// spellings, matching oxlint's script-comment directives.
#[derive(Debug, Default)]
struct TemplateCommentDirectives {
    /// Half-open file-offset ranges in which *every* rule is suppressed.
    block_all: Vec<Span>,
    /// Per rule name (as written in the directive), the ranges in which it is
    /// suppressed.
    block_rules: FxHashMap<String, Vec<Span>>,
    /// Half-open file-offset ranges — one per `…-disable-line` /
    /// `…-disable-next-line` directive, normally the whole target line — in
    /// which every rule is suppressed. See [`line_suppression_span`] for why
    /// these are ranges rather than bare line numbers.
    line_all: Vec<Span>,
    /// Per rule name, the same ranges.
    line_rules: FxHashMap<String, Vec<Span>>,
}

/// One directive comment's parsed shape.
enum DirectiveKind {
    DisableBlock,
    EnableBlock,
    /// Suppress the whole line at this 0-based line index.
    DisableLine(u32),
}

impl TemplateCommentDirectives {
    /// `nodes` are one `<template>` block's parsed root nodes, whose spans are
    /// file offsets; `line_starts` is the whole file's line table (see
    /// [`line_start_offsets`]) and `content_end` the file offset at which the
    /// block's content stops.
    fn collect(nodes: &[Node<'_>], line_starts: &[u32], content_end: u32) -> Self {
        let mut comments: Vec<(Span, &str)> = Vec::new();
        collect_comments(nodes, &mut comments);
        // The AST is walked depth-first, which is already source order for a
        // well-formed tree; sort anyway so recovery from malformed markup
        // can't silently reorder directives.
        comments.sort_unstable_by_key(|(span, _)| span.start);
        // The whole block is one scope: upstream's only `clear` for a
        // `templateBody` is at its end, which `content_end` already models.
        Self::build(&comments, line_starts, &[], content_end)
    }

    /// The file-scoped counterpart: the `<!-- eslint-disable … -->` comments
    /// that sit outside every block (upstream's
    /// `extractTopLevelDocumentFragmentComments`).
    ///
    /// `clear_offsets` are the file offsets at which all open block
    /// suppressions are closed — upstream reports a `clear` pseudo message at
    /// every top-level element's end, so a file-scoped disable reaches only to
    /// the end of the next block, not to the end of the file. They must be
    /// ascending, which they are as `Sfc::blocks` is in source order.
    fn collect_top_level(
        comments: &[Comment<'_>],
        line_starts: &[u32],
        clear_offsets: &[u32],
        file_end: u32,
    ) -> Self {
        if comments.is_empty() {
            return Self::default();
        }
        let comments: Vec<(Span, &str)> =
            comments.iter().map(|comment| (comment.span, comment.content)).collect();
        Self::build(&comments, line_starts, clear_offsets, file_end)
    }

    /// Walk `comments` (ascending by start offset) and `clear_offsets`
    /// (ascending) as one merged event stream — upstream's `postprocess` state
    /// machine over the location-sorted message list — and resolve the open
    /// suppressions into ranges. Anything still open at `end` runs to there.
    fn build(
        comments: &[(Span, &str)],
        line_starts: &[u32],
        clear_offsets: &[u32],
        end: u32,
    ) -> Self {
        let mut directives = Self::default();
        let mut open_all: Option<u32> = None;
        let mut open_rules: FxHashMap<String, u32> = FxHashMap::default();
        let mut clears = clear_offsets.iter().copied().peekable();

        for &(span, text) in comments {
            // Every `clear` at or before this comment closes what is open.
            while let Some(clear) = clears.next_if(|&clear| clear <= span.start) {
                directives.close_open(&mut open_all, &mut open_rules, clear);
            }
            let stripped = strip_description(text);
            let Some((kind, rules)) = parse_directive(stripped, span, line_starts) else {
                continue;
            };
            match kind {
                DirectiveKind::DisableBlock => {
                    if rules.is_empty() {
                        open_all.get_or_insert(span.start);
                    } else {
                        for rule in rules {
                            open_rules.entry(rule.to_string()).or_insert(span.start);
                        }
                    }
                }
                DirectiveKind::EnableBlock => {
                    if rules.is_empty() {
                        if let Some(start) = open_all.take() {
                            directives.block_all.push(Span::new(start, span.start));
                        }
                    } else {
                        for rule in rules {
                            if let Some(start) = open_rules.remove(rule) {
                                directives
                                    .block_rules
                                    .entry(rule.to_string())
                                    .or_default()
                                    .push(Span::new(start, span.start));
                            }
                        }
                    }
                }
                DirectiveKind::DisableLine(line) => {
                    let range = line_suppression_span(line, line_starts, clear_offsets, end);
                    if rules.is_empty() {
                        directives.line_all.push(range);
                    } else {
                        for rule in rules {
                            directives.line_rules.entry(rule.to_string()).or_default().push(range);
                        }
                    }
                }
            }
        }

        // Trailing `clear`s (blocks that come after the last directive
        // comment), then the scope's end — upstream's `clear` at
        // `templateBody.loc.end` for the per-block case.
        for clear in clears {
            directives.close_open(&mut open_all, &mut open_rules, clear);
        }
        directives.close_open(&mut open_all, &mut open_rules, end);
        directives
    }

    /// Upstream's `clear` pseudo message: close every open block suppression
    /// at `at`.
    fn close_open(
        &mut self,
        open_all: &mut Option<u32>,
        open_rules: &mut FxHashMap<String, u32>,
        at: u32,
    ) {
        if let Some(start) = open_all.take() {
            self.block_all.push(Span::new(start, at));
        }
        for (rule, start) in open_rules.drain() {
            self.block_rules.entry(rule).or_default().push(Span::new(start, at));
        }
    }

    fn is_empty(&self) -> bool {
        self.block_all.is_empty()
            && self.block_rules.is_empty()
            && self.line_all.is_empty()
            && self.line_rules.is_empty()
    }

    /// Whether a diagnostic of `plugin_name`/`rule_name` starting at file
    /// `offset` is suppressed.
    fn suppresses(&self, plugin_name: &str, rule_name: &str, offset: u32) -> bool {
        let covers =
            |spans: &[Span]| spans.iter().any(|span| span.start <= offset && offset < span.end);
        if covers(&self.block_all) || covers(&self.line_all) {
            return true;
        }
        let matches = |directive: &String| rule_name_matches(directive, plugin_name, rule_name);
        self.block_rules
            .iter()
            .chain(self.line_rules.iter())
            .any(|(directive, spans)| matches(directive) && covers(spans))
    }
}

/// The file-offset range a `…-disable-line` / `…-disable-next-line` directive
/// targeting 0-based `line` actually suppresses.
///
/// Upstream models a line directive as a `disableLine` pseudo message at
/// `{line, column: -1}` plus an `enableLine` at `{line + 1, column: -1}` — so
/// "that whole line" — but its processor's `case 'clear'` resets `state.line`
/// (`disableAllKeys` *and* `disableRuleKeys`) just like it resets
/// `state.block`. A `clear` is reported at the end of every top-level element,
/// and messages are filtered in location order, so a `clear` landing *within*
/// the target line cuts the suppression short there: in
/// `<!-- eslint-disable-next-line vue/block-order -->` followed by
/// `<style>…</style><script>…</script>` on one line, the `clear` at the end of
/// `<style>` fires before the report anchored at `<script>`, which is
/// therefore NOT suppressed. Representing the directive as a range instead of
/// a line number is what reproduces that; with no `clear` inside the line
/// (the overwhelmingly common case, and always so for a directive inside a
/// `<template>` block, whose only `clear` is at the block's end) the range is
/// exactly the whole line, as before.
///
/// `clear_offsets` must be ascending, as [`TemplateCommentDirectives::build`]
/// already requires; `end` is the scope's own final `clear`.
fn line_suppression_span(line: u32, line_starts: &[u32], clear_offsets: &[u32], end: u32) -> Span {
    let index = line as usize;
    let start = line_starts.get(index).copied().unwrap_or(end).min(end);
    let mut stop = line_starts.get(index + 1).copied().unwrap_or(end).min(end);
    if let Some(&clear) = clear_offsets.iter().find(|&&clear| clear >= start) {
        stop = stop.min(clear);
    }
    Span::new(start, stop.max(start))
}

/// Upstream's rule-ID matching: exact equality against the full
/// `"<plugin>/<rule>"` string (see the "Reproduced semantics" section on
/// [`TemplateCommentDirectives`] for the processor.js evidence). A directive
/// missing the plugin prefix, or naming a different plugin, does not match.
fn rule_name_matches(directive: &str, plugin_name: &str, rule_name: &str) -> bool {
    directive
        .strip_prefix(plugin_name)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|rest| rest == rule_name)
}

/// `stripDirectiveComment`: drop everything from the first ` -- ` (two or more
/// dashes surrounded by whitespace) onwards.
fn strip_description(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let mut dash = index + 1;
        while dash < bytes.len() && bytes[dash] == b'-' {
            dash += 1;
        }
        if dash - index >= 3 && dash < bytes.len() && bytes[dash].is_ascii_whitespace() {
            return &text[..index];
        }
        index += 1;
    }
    text
}

/// Match one directive comment's (description-stripped) text against the two
/// upstream patterns, and split off its rule list.
///
/// Upstream: `^\s*(eslint-(?:en|dis)able)(?:\s+|$)` for the block form and
/// `^\s*(eslint-disable(?:-next)?-line)(?:\s+|$)` for the line form. The
/// trailing `(?:\s+|$)` is what keeps `eslint-disable-line` from also matching
/// the block pattern.
fn parse_directive<'t>(
    text: &'t str,
    span: Span,
    line_starts: &[u32],
) -> Option<(DirectiveKind, Vec<&'t str>)> {
    let rest = text.trim_start();
    let (keyword, tail) = split_keyword(rest)?;
    let keyword = keyword.strip_prefix("eslint-").or_else(|| keyword.strip_prefix("oxlint-"))?;

    let kind = match keyword {
        "disable" => DirectiveKind::DisableBlock,
        "enable" => DirectiveKind::EnableBlock,
        "disable-line" | "disable-next-line" => {
            // Upstream ignores a line directive whose comment spans multiple
            // lines (`comment.loc.start.line === comment.loc.end.line`).
            let start_line = line_of(span.start, line_starts);
            if line_of(span.end, line_starts) != start_line {
                return None;
            }
            let offset = u32::from(keyword == "disable-next-line");
            DirectiveKind::DisableLine(start_line + offset)
        }
        _ => return None,
    };
    Some((kind, collect_rule_names(tail)))
}

/// Split `text` into its leading keyword and the rest, requiring the keyword to
/// be followed by whitespace or end-of-text (upstream's `(?:\s+|$)`).
fn split_keyword(text: &str) -> Option<(&str, &str)> {
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    Some((&text[..end], &text[end..]))
}

/// Upstream's `/([^\s,]+)[\s,]*/g` over the remainder of the directive.
fn collect_rule_names(text: &str) -> Vec<&str> {
    text.split([',', ' ', '\t', '\n', '\r']).filter(|rule| !rule.is_empty()).collect()
}

fn collect_comments<'a>(nodes: &[Node<'a>], out: &mut Vec<(Span, &'a str)>) {
    for node in nodes {
        match node {
            Node::Comment(comment) => out.push((comment.span, comment.content)),
            Node::Element(element) => collect_comments(&element.children, out),
            _ => {}
        }
    }
}

/// Byte offsets at which each line of `text` starts (index 0 is always `0`).
fn line_start_offsets(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(u32::try_from(index + 1).unwrap_or(u32::MAX));
        }
    }
    starts
}

/// The offset a diagnostic is anchored at — its primary label's start, the
/// same one [`Message::new`] derives its span from (and therefore the same
/// position the reported line/column comes from, which is what upstream's
/// message-position-driven filtering keys off).
fn diagnostic_start(diagnostic: &OxcDiagnostic) -> Option<u32> {
    diagnostic
        .labels
        .iter()
        .find(|label| label.primary())
        .or_else(|| diagnostic.labels.first())
        .map(oxc_diagnostics::LabeledSpan::offset)
}

/// Drop from `diagnostics` everything any of `directive_sets` suppresses.
fn retain_unsuppressed(
    diagnostics: &mut Vec<OxcDiagnostic>,
    directive_sets: &[&TemplateCommentDirectives],
    rule: &RuleEnum,
) {
    if directive_sets.iter().all(|directives| directives.is_empty()) {
        return;
    }
    let plugin_name = rule.plugin_name();
    let rule_name = rule.name();
    diagnostics.retain(|diagnostic| {
        let Some(offset) = diagnostic_start(diagnostic) else { return true };
        !directive_sets
            .iter()
            .any(|directives| directives.suppresses(plugin_name, rule_name, offset))
    });
}

/// Apply the `<script>` blocks' `/* eslint-disable … */` comment directives to
/// this pass's messages.
///
/// eslint-plugin-vue runs on a single ESLint `Program` covering the whole
/// `.vue` file, so a script directive comment is an ordinary ESLint core
/// directive and suppresses any message positioned after it — including
/// messages from the rules this module dispatches. oxlint instead builds
/// [`crate::disable_directives::DisableDirectives`] per `<script>` sub-host,
/// over that sub-host's *extracted* source, so its intervals are sub-host
/// relative and stop at the end of the script block.
///
/// This maps a message back into a sub-host's coordinates only when the
/// message lies entirely inside that sub-host's slice of the file, which is
/// exactly where the two models agree. The deliberate consequence is that the
/// "…and everything after it" tail of a script-block `eslint-disable` is not
/// honored for messages anchored *outside* the script block; representing that
/// would mean clamping file offsets into sub-host space, which would also make
/// an `eslint-disable-next-line` on the script's last line swallow the rest of
/// the file. The tail is unreachable for three of the four `VueSfcRule`s
/// anyway (they report on a block's opening tag or inside `<template>`), and
/// upstream cannot suppress `vue/multi-word-component-names`' filename report
/// from a script comment either — that one is reported at line 1, column 0,
/// ahead of any comment inside a block.
#[expect(
    clippy::redundant_pub_crate,
    reason = "mod vue_template is itself private, so pub(crate) is redundant, but it documents \
              the intended crate-wide (not module-local) visibility explicitly"
)]
pub(crate) fn filter_by_script_directives(
    messages: &mut Vec<Message>,
    sub_hosts: &[ContextSubHost<'_>],
) {
    if messages.is_empty() || sub_hosts.is_empty() {
        return;
    }
    messages.retain(|message| {
        let Some(rule) = &message.rule else { return true };
        !sub_hosts.iter().any(|sub_host| {
            let offset = sub_host.source_text_offset();
            let Some(start) = message.span.start.checked_sub(offset) else { return false };
            let Some(end) = message.span.end.checked_sub(offset) else { return false };
            let len = u32::try_from(sub_host.semantic().source_text().len()).unwrap_or(u32::MAX);
            if end > len {
                return false;
            }
            sub_host.disable_directives().contains(&rule.rule_name, Span::new(start, end))
        })
    });
}

/// The 0-based line `offset` falls on, given [`line_start_offsets`].
fn line_of(offset: u32, line_starts: &[u32]) -> u32 {
    let index = line_starts.partition_point(|&start| start <= offset).saturating_sub(1);
    u32::try_from(index).unwrap_or(0)
}

/// Mirror `LintContext::add_diagnostic`: attach error code, docs URL,
/// severity, and rule attribution to every diagnostic, and push the resulting
/// [`Message`]s onto `messages`. Spans are already file-absolute — both
/// passes report against the full file source — so nothing is shifted here.
fn push_messages(
    messages: &mut Vec<Message>,
    diagnostics: Vec<OxcDiagnostic>,
    rule: &RuleEnum,
    severity: AllowWarnDeny,
) {
    let plugin_name = rule.plugin_name();
    let rule_name = rule.name();
    for diagnostic in diagnostics {
        let error = diagnostic
            .with_error_code(plugin_name, rule_name)
            .with_url(format!(
                "{}/{}/{}.html",
                crate::WEBSITE_BASE_RULES_URL,
                plugin_name,
                rule_name
            ))
            .with_severity(severity.into());
        let message = Message::new(error, PossibleFixes::None).with_rule(MessageRule {
            plugin_name: std::borrow::Cow::Borrowed(plugin_name),
            rule_name: std::borrow::Cow::Borrowed(rule_name),
        });
        messages.push(message);
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use crate::{
        rule::{RuleMeta, RuleRunFunctionsImplemented, RuleRunner},
        rules::{vue::block_order::BlockOrder, vue::valid_v_text::ValidVText},
        tester::Tester,
    };

    /// Template and SFC rules have an empty `impl Rule`, so they must be classified as
    /// implementing no run function at all. Otherwise the node-visiting runner puts them
    /// in the "run on every AST node" bucket and pays a virtual call per node, per rule,
    /// for rules that can never report from there.
    #[test]
    fn externally_dispatched_rules_implement_no_run_functions() {
        assert_eq!(<ValidVText as RuleRunner>::RUN_FUNCTIONS, RuleRunFunctionsImplemented::None);
        assert_eq!(<BlockOrder as RuleRunner>::RUN_FUNCTIONS, RuleRunFunctionsImplemented::None);

        for run_functions in
            [<ValidVText as RuleRunner>::RUN_FUNCTIONS, <BlockOrder as RuleRunner>::RUN_FUNCTIONS]
        {
            assert!(!run_functions.is_run_implemented());
            assert!(!run_functions.is_run_once_implemented());
            assert!(!run_functions.is_run_on_jest_node_implemented());
        }
    }

    /// Nothing may be classified as "implements no run function" unless something other
    /// than the node-visiting runner dispatches it: the Vue template/SFC pass in this
    /// module, or tsgolint for the type-aware rules.
    #[test]
    fn run_less_rules_are_dispatched_elsewhere() {
        for rule in crate::rules::RULES.iter() {
            if rule.run_info() != RuleRunFunctionsImplemented::None {
                continue;
            }
            assert!(
                rule.is_tsgolint_rule()
                    || super::as_vue_template_rule(rule).is_some()
                    || super::as_vue_sfc_rule(rule).is_some(),
                "rule {}/{} implements no run function and is dispatched by nothing, so it can never report",
                rule.plugin_name(),
                rule.name()
            );
        }
    }

    /// ...and they must still report, because they are dispatched by this module rather
    /// than by the node-visiting runner.
    #[test]
    fn template_rules_still_report_on_vue_files() {
        Tester::new(
            ValidVText::NAME,
            ValidVText::PLUGIN,
            vec![(
                r#"<template><div v-text="message" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            )],
            vec![(
                r"<template><div v-text /></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            )],
        )
        .test();
    }

    /// Same guard for the SFC pass, which uses a different dispatch entry point.
    #[test]
    fn sfc_rules_still_report_on_vue_files() {
        Tester::new(
            BlockOrder::NAME,
            BlockOrder::PLUGIN,
            vec![(
                r"<script>1</script><template><div/></template><style>.a{}</style>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            )],
            vec![(
                r"<style>.a{}</style><script>1</script><template><div/></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            )],
        )
        .test();
    }
}

/// Unit tests for the comment-directive state machine itself. The `Tester`
/// cases in `rules/vue/*` exercise it end to end through real `.vue` sources;
/// these pin the private pieces directly so a refactor of the state machine
/// fails here, next to the code, rather than in some rule's snapshot.
#[cfg(test)]
mod directive_tests {
    use oxc_span::Span;

    use super::{
        DirectiveKind, TemplateCommentDirectives, collect_rule_names, line_of, line_start_offsets,
        line_suppression_span, parse_directive, rule_name_matches, strip_description,
    };

    /// [`TemplateCommentDirectives::build`] with a trivial line table — block
    /// directives never consult it.
    fn build(
        comments: &[(Span, &str)],
        clear_offsets: &[u32],
        end: u32,
    ) -> TemplateCommentDirectives {
        TemplateCommentDirectives::build(comments, &[0], clear_offsets, end)
    }

    #[test]
    fn disable_then_enable_closes_a_block_range() {
        let directives = build(
            &[(Span::new(0, 14), "eslint-disable"), (Span::new(50, 63), "eslint-enable")],
            &[],
            100,
        );
        assert_eq!(directives.block_all, [Span::new(0, 50)]);
        assert!(directives.suppresses("vue", "no-v-html", 25));
        assert!(!directives.suppresses("vue", "no-v-html", 75));
    }

    #[test]
    fn unclosed_disable_runs_to_the_scope_end() {
        let directives = build(&[(Span::new(10, 24), "eslint-disable")], &[], 100);
        assert_eq!(directives.block_all, [Span::new(10, 100)]);
    }

    /// Upstream's `clear` pseudo message (reported at every top-level
    /// element's end) resets `state.block`, so a file-scoped disable reaches
    /// only to the end of the next block.
    #[test]
    fn a_clear_between_directives_closes_the_open_suppression() {
        let directives = build(
            &[(Span::new(0, 14), "eslint-disable"), (Span::new(60, 73), "eslint-enable")],
            &[30],
            100,
        );
        assert_eq!(directives.block_all, [Span::new(0, 30)]);
        assert!(!directives.suppresses("vue", "no-v-html", 45));
    }

    #[test]
    fn a_trailing_clear_closes_what_the_last_directive_left_open() {
        let directives = build(&[(Span::new(0, 14), "eslint-disable")], &[40], 100);
        assert_eq!(directives.block_all, [Span::new(0, 40)]);
    }

    #[test]
    fn a_disable_after_a_clear_opens_a_fresh_range() {
        let directives = build(&[(Span::new(50, 64), "eslint-disable")], &[30], 100);
        assert_eq!(directives.block_all, [Span::new(50, 100)]);
    }

    #[test]
    fn per_rule_disable_only_suppresses_that_rule() {
        let directives = build(&[(Span::new(0, 30), "eslint-disable vue/no-v-html")], &[], 100);
        assert!(directives.block_all.is_empty());
        assert!(directives.suppresses("vue", "no-v-html", 50));
        assert!(!directives.suppresses("vue", "no-v-text", 50));
    }

    #[test]
    fn per_rule_enable_closes_only_the_named_rule() {
        let directives = build(
            &[
                (Span::new(0, 40), "eslint-disable vue/no-v-html, vue/no-v-text"),
                (Span::new(50, 80), "eslint-enable vue/no-v-html"),
            ],
            &[],
            100,
        );
        assert!(!directives.suppresses("vue", "no-v-html", 60));
        assert!(directives.suppresses("vue", "no-v-text", 60));
    }

    /// `or_insert` semantics: a second disable while one is already open must
    /// not restart (and thereby shrink) the suppression.
    #[test]
    fn a_repeated_disable_keeps_the_first_start() {
        let directives = build(
            &[
                (Span::new(0, 14), "eslint-disable"),
                (Span::new(20, 34), "eslint-disable"),
                (Span::new(50, 63), "eslint-enable"),
            ],
            &[],
            100,
        );
        assert_eq!(directives.block_all, [Span::new(0, 50)]);
    }

    #[test]
    fn an_enable_without_a_matching_disable_is_a_no_op() {
        let directives = build(&[(Span::new(0, 13), "eslint-enable")], &[], 100);
        assert!(directives.is_empty());
    }

    /// Suppression ranges are half-open: a diagnostic anchored exactly at an
    /// `eslint-enable` is no longer suppressed.
    #[test]
    fn suppression_ranges_are_half_open() {
        let directives = build(
            &[(Span::new(10, 24), "eslint-disable"), (Span::new(20, 33), "eslint-enable")],
            &[],
            100,
        );
        assert!(!directives.suppresses("vue", "no-v-html", 9));
        assert!(directives.suppresses("vue", "no-v-html", 10));
        assert!(directives.suppresses("vue", "no-v-html", 19));
        assert!(!directives.suppresses("vue", "no-v-html", 20));
    }

    /// `build` strips ` -- description` before parsing, so a documented
    /// directive still takes effect.
    #[test]
    fn a_directive_description_does_not_break_parsing() {
        let directives =
            build(&[(Span::new(0, 45), "eslint-disable vue/no-v-html -- migration")], &[], 100);
        assert!(directives.suppresses("vue", "no-v-html", 50));
    }

    #[test]
    fn line_directives_land_in_the_line_buckets() {
        let line_starts = line_start_offsets("aaaa\nbbbb\ncccc\n");
        let directives = TemplateCommentDirectives::build(
            &[(Span::new(0, 4), "eslint-disable-next-line vue/no-v-html")],
            &line_starts,
            &[],
            15,
        );
        assert!(directives.block_all.is_empty() && directives.block_rules.is_empty());
        assert!(directives.suppresses("vue", "no-v-html", 7));
        assert!(!directives.suppresses("vue", "no-v-html", 12));
        assert!(!directives.suppresses("vue", "no-v-text", 7));
    }

    /// Upstream compares full reported `ruleId`s, which always carry the
    /// plugin prefix: bare or differently-prefixed names never match.
    #[test]
    fn rule_ids_match_exactly() {
        assert!(rule_name_matches("vue/no-v-html", "vue", "no-v-html"));
        assert!(!rule_name_matches("no-v-html", "vue", "no-v-html"));
        assert!(!rule_name_matches("typo/no-v-html", "vue", "no-v-html"));
        assert!(!rule_name_matches("vue/no-v-htm", "vue", "no-v-html"));
        assert!(!rule_name_matches("vue/no-v-html-x", "vue", "no-v-html"));
    }

    #[test]
    fn the_oxlint_prefix_is_a_synonym() {
        assert!(matches!(
            parse_directive("oxlint-disable", Span::new(0, 14), &[0]),
            Some((DirectiveKind::DisableBlock, _))
        ));
        assert!(matches!(
            parse_directive("oxlint-enable", Span::new(0, 13), &[0]),
            Some((DirectiveKind::EnableBlock, _))
        ));
    }

    #[test]
    fn disable_line_targets_its_own_line_and_next_line_the_following() {
        let line_starts = line_start_offsets("a\nb\nc\n");
        let span = Span::new(2, 3); // on line 1
        assert!(matches!(
            parse_directive("eslint-disable-line", span, &line_starts),
            Some((DirectiveKind::DisableLine(1), _))
        ));
        assert!(matches!(
            parse_directive("eslint-disable-next-line", span, &line_starts),
            Some((DirectiveKind::DisableLine(2), _))
        ));
    }

    /// Upstream requires a line directive's comment to start and end on the
    /// same line.
    #[test]
    fn multi_line_line_directives_are_ignored() {
        let line_starts = line_start_offsets("a\nb\nc\n");
        assert!(parse_directive("eslint-disable-line", Span::new(0, 3), &line_starts).is_none());
    }

    /// The keyword must be followed by whitespace or end-of-text (upstream's
    /// `(?:\s+|$)`), so extended spellings are not directives.
    #[test]
    fn unknown_keywords_are_not_directives() {
        assert!(parse_directive("eslint-disable-everything", Span::new(0, 5), &[0]).is_none());
        assert!(parse_directive("disable", Span::new(0, 5), &[0]).is_none());
        assert!(parse_directive("just a comment", Span::new(0, 5), &[0]).is_none());
    }

    /// `stripDirectiveComment`: two-or-more dashes surrounded by whitespace
    /// start the description; anything less binds to the rule list.
    #[test]
    fn descriptions_after_double_dashes_are_stripped() {
        assert_eq!(strip_description("eslint-disable a -- why"), "eslint-disable a");
        assert_eq!(strip_description("eslint-disable a --- why"), "eslint-disable a");
        assert_eq!(strip_description("eslint-disable a--b"), "eslint-disable a--b");
        assert_eq!(strip_description("eslint-disable a - b"), "eslint-disable a - b");
    }

    #[test]
    fn rule_lists_split_on_commas_and_whitespace() {
        assert_eq!(collect_rule_names(" vue/a, vue/b\tvue/c "), ["vue/a", "vue/b", "vue/c"]);
        assert!(collect_rule_names("  ").is_empty());
    }

    #[test]
    fn a_line_directive_normally_covers_the_whole_line() {
        let line_starts = line_start_offsets("aaaa\nbbbb\ncccc\n");
        assert_eq!(line_suppression_span(1, &line_starts, &[], 15), Span::new(5, 10));
    }

    /// The `clear` reset applies to line suppressions too: a top-level
    /// element ending *within* the target line cuts the suppression there.
    #[test]
    fn a_clear_inside_the_target_line_cuts_the_suppression_short() {
        let line_starts = line_start_offsets("aaaa\nbbbb\ncccc\n");
        assert_eq!(line_suppression_span(1, &line_starts, &[7], 15), Span::new(5, 7));
    }

    #[test]
    fn a_clear_after_the_target_line_changes_nothing() {
        let line_starts = line_start_offsets("aaaa\nbbbb\ncccc\n");
        assert_eq!(line_suppression_span(1, &line_starts, &[12], 15), Span::new(5, 10));
    }

    /// `…-disable-next-line` on the scope's last line targets a line that
    /// does not exist: the range collapses to empty instead of panicking or
    /// swallowing the tail.
    #[test]
    fn a_line_past_the_end_collapses_to_an_empty_span_at_the_scope_end() {
        let line_starts = line_start_offsets("aaaa\n");
        assert_eq!(line_suppression_span(7, &line_starts, &[], 5), Span::new(5, 5));
    }

    #[test]
    fn line_of_maps_offsets_at_line_starts_to_that_line() {
        let line_starts = line_start_offsets("aa\nbb\n");
        assert_eq!(line_of(0, &line_starts), 0);
        assert_eq!(line_of(2, &line_starts), 0);
        assert_eq!(line_of(3, &line_starts), 1);
        assert_eq!(line_of(6, &line_starts), 2);
    }
}
