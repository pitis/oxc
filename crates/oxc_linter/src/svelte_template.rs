//! Svelte markup linting pass.
//!
//! The loader extracts only `<script>` blocks from `.svelte` files, so the
//! script sub-host machinery never sees the markup. This pass parses the
//! whole file with `svelte_markup_parser` and runs the markup-capable rules
//! of the `svelte` plugin over the resulting AST.
//!
//! It mirrors the `.vue` template pass in `vue_template.rs` — same message
//! shape, same absolute-file-offset span contract, and the same
//! `<!-- eslint-disable … -->` comment-directive machinery (whose semantics
//! eslint-plugin-svelte's processor shares with eslint-plugin-vue's: both
//! resolve directive comments into position-ordered suppression state over
//! the reported messages). One structural difference: Svelte components are
//! markup at the top level, so the pass runs once over the whole file
//! rather than per `<template>` block, and the directive scope is the file.
//!
//! Not yet supported: fixes.

use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_span::Span;
use svelte_markup_parser::{
    ast::{BlockKind, Node},
    parse_markup,
};

use crate::{
    Linter,
    fixer::Message,
    rules::RuleEnum,
    vue_template::{
        TemplateCommentDirectives, line_start_offsets, push_messages, retain_unsuppressed,
    },
};

/// A rule that lints the parsed markup of a `.svelte` file.
///
/// Implemented in addition to (not instead of) the [`crate::rule::Rule`]
/// trait: the rule registers, resolves, and configures like any other rule
/// of the `svelte` plugin; its `Rule` impl is empty and this trait carries
/// the markup logic.
pub trait SvelteTemplateRule {
    /// `nodes` are the root nodes of the whole `.svelte` file; their spans
    /// are absolute file offsets and index into `ctx.source_text()`. Report
    /// through `ctx` with those same spans.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>);
}

/// The subset of the resolved rule set that participates in the markup pass.
pub fn as_svelte_template_rule(rule: &RuleEnum) -> Option<&dyn SvelteTemplateRule> {
    match rule {
        RuleEnum::SvelteNoAtHtmlTags(rule) => Some(rule),
        RuleEnum::SvelteNoAtDebugTags(rule) => Some(rule),
        RuleEnum::SvelteRequireEachKey(rule) => Some(rule),
        RuleEnum::SvelteNoDupeElseIfBlocks(rule) => Some(rule),
        RuleEnum::SvelteNoObjectInTextMustaches(rule) => Some(rule),
        RuleEnum::SvelteNoDynamicSlotName(rule) => Some(rule),
        RuleEnum::SvelteNoNotFunctionHandler(rule) => Some(rule),
        RuleEnum::SvelteNoUselessMustaches(rule) => Some(rule),
        RuleEnum::SvelteNoTargetBlank(rule) => Some(rule),
        RuleEnum::SvelteButtonHasType(rule) => Some(rule),
        RuleEnum::SvelteNoUselessChildrenSnippet(rule) => Some(rule),
        RuleEnum::SvelteNoDupeOnDirectives(rule) => Some(rule),
        RuleEnum::SvelteNoDupeUseDirectives(rule) => Some(rule),
        RuleEnum::SvelteNoSpacesAroundEqualSignsInAttribute(rule) => Some(rule),
        RuleEnum::SvelteNoDupeStyleProperties(rule) => Some(rule),
        RuleEnum::SvelteCommentDirective(rule) => Some(rule),
        RuleEnum::SvelteSystem(rule) => Some(rule),
        RuleEnum::SvelteNoRawSpecialElements(rule) => Some(rule),
        RuleEnum::SvelteNoShorthandStylePropertyOverrides(rule) => Some(rule),
        RuleEnum::SvelteNoUnknownStyleDirectiveProperty(rule) => Some(rule),
        RuleEnum::SvelteValidEachKey(rule) => Some(rule),
        RuleEnum::SvelteNoDomManipulating(rule) => Some(rule),
        RuleEnum::SvelteNoUnusedProps(rule) => Some(rule),
        RuleEnum::SvelteNoUnusedSvelteIgnore(rule) => Some(rule),
        RuleEnum::SvelteRequireStoreReactiveAccess(rule) => Some(rule),
        RuleEnum::SvelteNoNavigationWithoutResolve(rule) => Some(rule),
        RuleEnum::SvelteSortAttributes(rule) => Some(rule),
        RuleEnum::SveltePreferStyleDirective(rule) => Some(rule),
        RuleEnum::SvelteRequireOptimizedStyleAttribute(rule) => Some(rule),
        RuleEnum::SvelteBlockLang(rule) => Some(rule),
        RuleEnum::SvelteExperimentalRequireSlotTypes(rule) => Some(rule),
        RuleEnum::SvelteExperimentalRequireStrictEvents(rule) => Some(rule),
        RuleEnum::SvelteNoAtConstTags(rule) => Some(rule),
        RuleEnum::SvelteNoBindValueOnCheckableInputs(rule) => Some(rule),
        RuleEnum::SvelteNoInlineStyles(rule) => Some(rule),
        RuleEnum::SvelteNoNestedStyleTag(rule) => Some(rule),
        RuleEnum::SvelteNoRestrictedHtmlElements(rule) => Some(rule),
        RuleEnum::SvelteSpacedHtmlComment(rule) => Some(rule),
        _ => None,
    }
}

/// Reporting context handed to [`SvelteTemplateRule::run_on_markup`].
pub struct SvelteTemplateContext<'a> {
    /// The whole `.svelte` file source — what the AST spans index into.
    source_text: &'a str,
    diagnostics: Vec<OxcDiagnostic>,
}

impl<'a> SvelteTemplateContext<'a> {
    /// The whole `.svelte` file source (what the AST spans are relative to).
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
    /// Run the Svelte markup pass over a `.svelte` file's full source text.
    ///
    /// Returns messages with file-absolute spans, shaped exactly like rule
    /// messages from the script pass (error code, docs URL, severity).
    /// Returns an empty vec for non-`.svelte` paths and when no
    /// markup-capable rule is enabled, without parsing anything.
    pub(crate) fn run_svelte_template_rules(&self, path: &Path, source_text: &str) -> Vec<Message> {
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("svelte") {
            return Vec::new();
        }
        let resolved = self.config.resolve(path);
        let template_rules: Vec<_> = resolved
            .rules
            .iter()
            .filter_map(|(rule, severity)| {
                as_svelte_template_rule(rule).map(|template_rule| (rule, template_rule, *severity))
            })
            .collect();
        if template_rules.is_empty() {
            return Vec::new();
        }

        let nodes = parse_markup(source_text, 0);

        // The whole file is one directive scope: eslint-plugin-svelte's
        // processor state machine runs over the file's message stream with
        // no per-block `clear` events (Svelte has no top-level blocks the
        // way a `.vue` SFC does).
        let mut comments: Vec<(Span, &str)> = Vec::new();
        collect_comments(&nodes, &mut comments);
        let directives = if comments.is_empty() {
            TemplateCommentDirectives::default()
        } else {
            comments.sort_unstable_by_key(|(span, _)| span.start);
            let line_starts = line_start_offsets(source_text);
            TemplateCommentDirectives::build(
                &comments,
                &line_starts,
                &[],
                u32::try_from(source_text.len()).unwrap_or(u32::MAX),
            )
        };

        let mut messages = Vec::new();
        for (rule, template_rule, severity) in &template_rules {
            let mut ctx = SvelteTemplateContext { source_text, diagnostics: Vec::new() };
            template_rule.run_on_markup(&nodes, &mut ctx);
            let mut diagnostics = ctx.diagnostics;
            retain_unsuppressed(&mut diagnostics, &[&directives], rule);
            push_messages(&mut messages, diagnostics, rule, *severity);
        }
        messages
    }
}

/// Collect every `<!-- … -->` comment in the tree, including those nested
/// inside elements and block branches.
fn collect_comments<'a>(nodes: &[Node<'a>], out: &mut Vec<(Span, &'a str)>) {
    for node in nodes {
        match node {
            Node::Comment(comment) => out.push((comment.span, comment.content)),
            Node::Element(element) => collect_comments(&element.children, out),
            Node::Block(block) => match &block.kind {
                BlockKind::If(if_block) => {
                    for branch in &if_block.branches {
                        collect_comments(&branch.children, out);
                    }
                }
                BlockKind::Each(each) => {
                    collect_comments(&each.children, out);
                    if let Some(fallback) = &each.fallback {
                        collect_comments(fallback, out);
                    }
                }
                BlockKind::Await(await_block) => {
                    collect_comments(&await_block.pending, out);
                    if let Some(children) = &await_block.then_children {
                        collect_comments(children, out);
                    }
                    if let Some(children) = &await_block.catch_children {
                        collect_comments(children, out);
                    }
                }
                BlockKind::Key(key) => collect_comments(&key.children, out),
                BlockKind::Snippet(snippet) => collect_comments(&snippet.children, out),
                BlockKind::Unknown(unknown) => collect_comments(&unknown.children, out),
            },
            _ => {}
        }
    }
}
