//! Vue `<template>` linting pass.
//!
//! The loader extracts only `<script>` blocks from `.vue` files, so the
//! script sub-host machinery never sees the template. This pass parses the
//! `<template>` block(s) with `oxc_vue_parser` and runs the template-capable
//! rules of the `vue` plugin over the resulting AST.
//!
//! It is deliberately independent of the sub-host loop:
//! - spans are absolute file offsets (template AST spans are relative to the
//!   block content and shifted by `content_span.start` on report), and
//! - it also covers `.vue` files that have no `<script>` block at all,
//!   which the sub-host loop skips entirely.
//!
//! Not yet supported (v1): fixes, and `<!-- eslint-disable -->` HTML comment
//! directives inside templates.

use std::path::Path;

use oxc_diagnostics::OxcDiagnostic;
use oxc_vue_parser::{ast::Node, parse_sfc, parse_template};

use crate::{
    Linter,
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
    /// `nodes` are the root nodes of one `<template>` block; spans are
    /// relative to the block content. Report through `ctx` — the pass
    /// shifts spans to absolute file offsets.
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>);
}

/// The subset of the resolved rule set that participates in the template pass.
fn as_vue_template_rule(rule: &RuleEnum) -> Option<&dyn VueTemplateRule> {
    match rule {
        RuleEnum::VueRequireVForKey(rule) => Some(rule),
        RuleEnum::VueNoDuplicateAttributes(rule) => Some(rule),
        _ => None,
    }
}

/// Reporting context handed to [`VueTemplateRule::run_on_template`].
pub struct VueTemplateContext<'a> {
    /// The `<template>` block content the AST spans index into.
    source_text: &'a str,
    diagnostics: Vec<OxcDiagnostic>,
}

impl<'a> VueTemplateContext<'a> {
    /// The template block's content (what the AST spans are relative to).
    pub fn source_text(&self) -> &'a str {
        self.source_text
    }

    /// Report a violation. Label spans are relative to the template block
    /// content; the pass converts them to file offsets.
    pub fn diagnostic(&mut self, diagnostic: OxcDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

impl Linter {
    /// Run the Vue template pass over a `.vue` file's full source text.
    ///
    /// Returns messages with file-absolute spans, shaped exactly like rule
    /// messages from the script pass (error code, docs URL, severity).
    /// Returns an empty vec for non-`.vue` paths and when no template-capable
    /// rule is enabled, without parsing anything.
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
        if template_rules.is_empty() {
            return Vec::new();
        }

        let mut messages = Vec::new();
        let sfc = parse_sfc(source_text);
        for block in &sfc.blocks {
            // Only true template blocks; `<template src="...">` has no
            // inline content to lint.
            if block.name != "template" || block.attribute_value("src").is_some() {
                continue;
            }
            let nodes = parse_template(block.content);
            for (rule, template_rule, severity) in &template_rules {
                let mut ctx =
                    VueTemplateContext { source_text: block.content, diagnostics: Vec::new() };
                template_rule.run_on_template(&nodes, &mut ctx);
                for diagnostic in ctx.diagnostics {
                    // Mirror `LintContext::add_diagnostic`: error code, docs
                    // URL, severity, and rule attribution.
                    let plugin_name = rule.plugin_name();
                    let rule_name = rule.name();
                    let error = diagnostic
                        .with_error_code(plugin_name, rule_name)
                        .with_url(format!(
                            "{}/{}/{}.html",
                            crate::WEBSITE_BASE_RULES_URL,
                            plugin_name,
                            rule_name
                        ))
                        .with_severity((*severity).into());
                    let mut message =
                        Message::new(error, PossibleFixes::None).with_rule(MessageRule {
                            plugin_name: std::borrow::Cow::Borrowed(plugin_name),
                            rule_name: std::borrow::Cow::Borrowed(rule_name),
                        });
                    if block.content_span.start != 0 {
                        message.move_offset(block.content_span.start);
                    }
                    messages.push(message);
                }
            }
        }
        messages
    }
}
