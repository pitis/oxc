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
//! `<!-- eslint-disable -->`-style HTML comment directives inside a
//! `<template>` block ARE honored, via [`TemplateCommentDirectives`] (see
//! there for the reproduced semantics).
//!
//! Not yet supported: fixes, and routing template/SFC diagnostics through the
//! *script*-comment (`/* oxlint-disable */`) directive machinery — see the
//! comment next to `messages.extend(template_messages)` in
//! `service/runtime.rs`.

use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_span::Span;
use oxc_vue_parser::{Sfc, ast::Node, parse_sfc, parse_template};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AllowWarnDeny, Linter,
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
///
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

        // SFC rules run once per file, over the whole parsed `Sfc` — not once
        // per `<template>` block. `sfc`'s block spans are already
        // file-absolute (see `VueSfcRule`'s docs), so `ctx.source_text` is
        // the full file source and diagnostics are pushed with no offset.
        for (rule, sfc_rule, severity) in &sfc_rules {
            let mut ctx = VueTemplateContext { source_text, diagnostics: Vec::new() };
            sfc_rule.run_on_sfc(&sfc, path, &mut ctx);
            push_messages(&mut messages, ctx.diagnostics, rule, *severity);
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
            let line_starts = line_start_offsets(source_text);
            let directives =
                TemplateCommentDirectives::collect(&nodes, &line_starts, block.content_span.end);
            for (rule, template_rule, severity) in &template_rules {
                let mut ctx = VueTemplateContext { source_text, diagnostics: Vec::new() };
                template_rule.run_on_template(&nodes, &mut ctx);
                let mut diagnostics = ctx.diagnostics;
                if !directives.is_empty() {
                    let plugin_name = rule.plugin_name();
                    let rule_name = rule.name();
                    diagnostics.retain(|diagnostic| {
                        let Some(offset) = diagnostic_start(diagnostic) else { return true };
                        !directives.suppresses(
                            plugin_name,
                            rule_name,
                            offset,
                            line_of(offset, &line_starts),
                        )
                    });
                }
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
///
/// ### Deliberate additions
///
/// `oxlint-disable`/`oxlint-enable`/`oxlint-disable-line`/
/// `oxlint-disable-next-line` are accepted as synonyms of the `eslint-`
/// spellings, matching oxlint's script-comment directives.
///
/// Rule names are matched with oxlint's own
/// [`crate::disable_directives::DisableDirectives::contains`] semantics — the
/// plugin prefix is stripped from the directive's rule name and compared
/// against the bare rule name — rather than upstream's exact `vue/x` string
/// equality, so `vue/no-v-html` and a bare `no-v-html` both work.
#[derive(Debug, Default)]
struct TemplateCommentDirectives {
    /// Half-open file-offset ranges in which *every* rule is suppressed.
    block_all: Vec<Span>,
    /// Per rule name (as written in the directive), the ranges in which it is
    /// suppressed.
    block_rules: FxHashMap<String, Vec<Span>>,
    /// 0-based line numbers (within the file) on which every rule is
    /// suppressed.
    line_all: FxHashSet<u32>,
    /// Per rule name, the lines on which it is suppressed.
    line_rules: FxHashMap<String, FxHashSet<u32>>,
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

        let mut directives = Self::default();
        let mut open_all: Option<u32> = None;
        let mut open_rules: FxHashMap<String, u32> = FxHashMap::default();

        for (span, text) in comments {
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
                    if rules.is_empty() {
                        directives.line_all.insert(line);
                    } else {
                        for rule in rules {
                            directives.line_rules.entry(rule.to_string()).or_default().insert(line);
                        }
                    }
                }
            }
        }

        // Anything still open runs to the end of the block — upstream's
        // `clear` at `templateBody.loc.end`.
        if let Some(start) = open_all {
            directives.block_all.push(Span::new(start, content_end));
        }
        for (rule, start) in open_rules {
            directives.block_rules.entry(rule).or_default().push(Span::new(start, content_end));
        }
        directives
    }

    fn is_empty(&self) -> bool {
        self.block_all.is_empty()
            && self.block_rules.is_empty()
            && self.line_all.is_empty()
            && self.line_rules.is_empty()
    }

    /// Whether a diagnostic of `plugin_name`/`rule_name` starting at file
    /// `offset` is suppressed.
    fn suppresses(&self, plugin_name: &str, rule_name: &str, offset: u32, line: u32) -> bool {
        if self.block_all.iter().any(|span| span.start <= offset && offset < span.end) {
            return true;
        }
        if self.line_all.contains(&line) {
            return true;
        }
        let matches = |directive: &String| rule_name_matches(directive, plugin_name, rule_name);
        self.block_rules.iter().any(|(directive, spans)| {
            matches(directive) && spans.iter().any(|span| span.start <= offset && offset < span.end)
        }) || self
            .line_rules
            .iter()
            .any(|(directive, lines)| matches(directive) && lines.contains(&line))
    }
}

/// oxlint's cross-plugin rule-name matching (see
/// [`crate::disable_directives::DisableDirectives::contains`]): strip the
/// plugin prefix from the directive's rule name, then compare against the bare
/// rule name. `plugin_name` is accepted (and ignored) so the signature reads
/// the same as the call site's intent.
fn rule_name_matches(directive: &str, _plugin_name: &str, rule_name: &str) -> bool {
    directive.rsplit_once('/').map_or(directive, |(_, rule)| rule) == rule_name
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
