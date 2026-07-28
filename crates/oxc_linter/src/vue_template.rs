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
//! Not yet supported (v1): fixes, and `<!-- eslint-disable -->` HTML comment
//! directives inside templates.

use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_vue_parser::{Sfc, ast::Node, parse_sfc, parse_template};

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
        _ => None,
    }
}

/// The subset of the resolved rule set that participates in the SFC pass.
///
fn as_vue_sfc_rule(rule: &RuleEnum) -> Option<&dyn VueSfcRule> {
    match rule {
        RuleEnum::VueValidTemplateRoot(rule) => Some(rule),
        RuleEnum::VueNoDeprecatedFunctionalTemplate(rule) => Some(rule),
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
            // `base_offset` makes every span in `nodes` a file offset, so no
            // post-hoc shifting of the reported diagnostics is needed.
            let nodes = parse_template(block.content, block.content_span.start);
            for (rule, template_rule, severity) in &template_rules {
                let mut ctx = VueTemplateContext { source_text, diagnostics: Vec::new() };
                template_rule.run_on_template(&nodes, &mut ctx);
                push_messages(&mut messages, ctx.diagnostics, rule, *severity);
            }
        }
        messages
    }
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
