use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn style_attribute_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Found disallowed style attribute.")
        .with_help("Move the declarations into the component's `<style>` element.")
        .with_label(span)
}

fn style_directive_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Found disallowed style directive.")
        .with_help("Move the declaration into the component's `<style>` element, e.g. with a `class:` directive.")
        .with_label(span)
}

fn transition_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Found disallowed transition.")
        .with_help(
            "Express the animation in CSS instead of a `transition:` / `in:` / `out:` directive.",
        )
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoInlineStyles {
    /// Whether `transition:` / `in:` / `out:` directives, which also produce
    /// inline styles, are allowed. Defaults to `true`.
    allow_transitions: bool,
}

impl Default for NoInlineStyles {
    fn default() -> Self {
        Self { allow_transitions: true }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows attributes and directives that produce inline styles.
    ///
    /// ### Why is this bad?
    ///
    /// Inline styles bypass the component's scoped stylesheet: they cannot be
    /// overridden without `!important`, they are invisible to a Content
    /// Security Policy that forbids `style-src 'unsafe-inline'`, and they
    /// scatter presentation across the markup.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div style="color: red"></div>
    /// <div style:color="red"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class="danger"></div>
    ///
    /// <style>
    ///   .danger { color: red }
    /// </style>
    /// ```
    ///
    /// ### Options
    ///
    /// `allowTransitions` (default `true`): when `false`, `transition:`,
    /// `in:` and `out:` directives are reported too, since they also set
    /// inline styles while running.
    ///
    /// ```json
    /// {
    ///   "svelte/no-inline-styles": ["error", { "allowTransitions": false }]
    /// }
    /// ```
    NoInlineStyles,
    svelte,
    restriction,
    config = NoInlineStyles,
    version = "1.80.0",
    short_description = "Disallow attributes and directives that produce inline styles.",
);

impl Rule for NoInlineStyles {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for NoInlineStyles {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Upstream only checks plain HTML elements; a `style` prop on a
            // component is an ordinary prop.
            if element.is_component_like() || element.svelte_name().is_some() {
                return;
            }
            for attribute in &element.attributes {
                match &attribute.kind {
                    AttributeKind::Plain { name, .. } if *name == "style" => {
                        diagnostics.push(style_attribute_diagnostic(attribute.span));
                    }
                    AttributeKind::Directive(directive)
                        if directive.kind == DirectiveKind::Style =>
                    {
                        diagnostics.push(style_directive_diagnostic(attribute.span));
                    }
                    AttributeKind::Directive(directive)
                        if !self.allow_transitions
                            && matches!(
                                directive.kind,
                                DirectiveKind::Transition | DirectiveKind::In | DirectiveKind::Out
                            ) =>
                    {
                        diagnostics.push(transition_diagnostic(attribute.span));
                    }
                    _ => {}
                }
            }
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoInlineStyles;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<div class=\"danger\"></div>", None, None, path()),
            // Transitions are allowed by default.
            ("<div transition:fade></div>", None, None, path()),
            ("<div in:fade out:fade></div>", None, None, path()),
            // A `style` prop on a component is an ordinary prop.
            ("<Widget style=\"color: red\" />", None, None, path()),
            (
                "<div transition:fade></div>",
                Some(serde_json::json!([{ "allowTransitions": true }])),
                None,
                path(),
            ),
        ];
        let fail = vec![
            ("<div style=\"color: red\"></div>", None, None, path()),
            ("<div style:color=\"red\"></div>", None, None, path()),
            ("<div style={css}></div>", None, None, path()),
            (
                "<div transition:fade></div>",
                Some(serde_json::json!([{ "allowTransitions": false }])),
                None,
                path(),
            ),
            (
                "<div in:fade></div>",
                Some(serde_json::json!([{ "allowTransitions": false }])),
                None,
                path(),
            ),
        ];

        Tester::new(NoInlineStyles::NAME, NoInlineStyles::PLUGIN, pass, fail).test_and_snapshot();
    }
}
