use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{
        default_true, get_plain_attribute, has_spread_attribute, svelte_start_tag_span,
        walk_svelte_elements,
    },
};

fn missing_type_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Missing an explicit type attribute for button.")
        .with_help(
            "An untyped `<button>` defaults to `type=\"submit\"` and can submit an enclosing form unexpectedly; write `type=\"button\"`, `type=\"submit\"`, or `type=\"reset\"`.",
        )
        .with_label(span)
}

fn empty_type_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("A value must be set for button type attribute.")
        .with_help("Give the `type` attribute one of the values `button`, `submit`, or `reset`.")
        .with_label(span)
}

fn invalid_type_diagnostic(value: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("{value} is an invalid value for button type attribute."))
        .with_help("Valid button types are `button`, `submit`, and `reset`.")
        .with_label(span)
}

fn forbidden_type_diagnostic(value: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("{value} is a forbidden value for button type attribute."))
        .with_help("This button type is disallowed by the rule's options.")
        .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ButtonHasType {
    /// Whether `type="button"` is allowed.
    #[serde(default = "default_true")]
    button: bool,
    /// Whether `type="submit"` is allowed.
    #[serde(default = "default_true")]
    submit: bool,
    /// Whether `type="reset"` is allowed.
    #[serde(default = "default_true")]
    reset: bool,
}

impl Default for ButtonHasType {
    fn default() -> Self {
        Self { button: true, submit: true, reset: true }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `<button>` elements to carry an explicit, valid `type`
    /// attribute: `"button"`, `"submit"`, or `"reset"`.
    ///
    /// ### Why is this bad?
    ///
    /// A `<button>` without a `type` defaults to `type="submit"`. Inside a
    /// form, clicking it submits the form — behavior that is rarely intended
    /// for buttons that just trigger some script, and that silently changes
    /// when markup is moved into or out of a form. An explicit type states
    /// the intent and pins the behavior.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <button>Hello world</button>
    /// <button type="">Hello world</button>
    /// <button type="foo">Hello world</button>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <button type="button">Hello world</button>
    /// <button type="submit">Hello world</button>
    /// <button type="reset">Hello world</button>
    /// ```
    ///
    /// ### Options
    ///
    /// This rule takes an object whose `button`, `submit`, and `reset`
    /// properties (each defaulting to `true`) control which of the valid
    /// types are allowed; a type set to `false` is reported as forbidden.
    ///
    /// ```json
    /// {
    ///   "svelte/button-has-type": ["error", { "button": true, "submit": true, "reset": false }]
    /// }
    /// ```
    ButtonHasType,
    svelte,
    restriction,
    config = ButtonHasType,
    version = "1.80.0",
    short_description = "Disallow usage of button without an explicit type attribute.",
);

impl Rule for ButtonHasType {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for ButtonHasType {
    // Ports eslint-plugin-svelte's `button-has-type`.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Only the HTML `<button>` element; `<Button>` components have
            // their own semantics.
            if element.name != "button" {
                return;
            }
            if let Some((attribute, value)) = get_plain_attribute(element, "type") {
                match value {
                    // `<button type>` and `<button type="">` both carry no
                    // value parts; upstream reports `emptyTypeAttribute`.
                    None => diagnostics.push(empty_type_diagnostic(attribute.span)),
                    Some(value) if value.parts.is_empty() => {
                        diagnostics.push(empty_type_diagnostic(attribute.span));
                    }
                    Some(value) => {
                        // Dynamic values (`type={expr}`, `type="a{b}"`) are
                        // not statically checkable; upstream skips them.
                        if let Some(text) = value.as_static_text() {
                            let allowed = match text {
                                "button" => Some(self.button),
                                "submit" => Some(self.submit),
                                "reset" => Some(self.reset),
                                _ => None,
                            };
                            match allowed {
                                None => {
                                    diagnostics.push(invalid_type_diagnostic(text, attribute.span));
                                }
                                Some(false) => diagnostics
                                    .push(forbidden_type_diagnostic(text, attribute.span)),
                                Some(true) => {}
                            }
                        }
                    }
                }
                return;
            }
            // A `bind:type` directive counts as an explicit type. Upstream
            // reports `emptyTypeAttribute` only when the binding has no
            // expression, which Svelte's parser never produces (`bind:type`
            // shorthand binds the `type` variable), so no report here.
            if element.attributes.iter().any(|attribute| {
                matches!(&attribute.kind, AttributeKind::Directive(directive)
                    if directive.kind == DirectiveKind::Bind && directive.name == "type")
            }) {
                return;
            }
            // The `{type}` shorthand is a (dynamic) type attribute.
            if element.attributes.iter().any(|attribute| {
                matches!(&attribute.kind, AttributeKind::Shorthand { name, .. } if *name == "type")
            }) {
                return;
            }
            // A spread may supply `type`; like upstream, don't report missing.
            if has_spread_attribute(element) {
                return;
            }
            diagnostics.push(missing_type_diagnostic(svelte_start_tag_span(element)));
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ButtonHasType;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<button type="button">Hello</button>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<button type="submit">Hello</button>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<button type="reset">Hello</button>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Dynamic types cannot be checked statically.
            ("<button type={type}>Hello</button>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button {type}>Hello</button>", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<button bind:type={t}>Hello</button>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A spread may carry the type.
            (
                "<button {...attributes}>Hello</button>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Components are not HTML buttons.
            ("<Button>Hello</Button>", None, None, Some(PathBuf::from("test.svelte"))),
            // Allowed set narrowed by options.
            (
                r#"<button type="button">Hello</button>"#,
                Some(serde_json::json!([{ "submit": false }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            ("<button>Hello</button>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button />", None, None, Some(PathBuf::from("test.svelte"))),
            // Empty values: bare attribute and empty string.
            ("<button type>Hello</button>", None, None, Some(PathBuf::from("test.svelte"))),
            (r#"<button type="">Hello</button>"#, None, None, Some(PathBuf::from("test.svelte"))),
            // Invalid static value.
            (
                r#"<button type="foo">Hello</button>"#,
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Valid HTML type forbidden by options.
            (
                r#"<button type="button">Hello</button>"#,
                Some(serde_json::json!([{ "button": false }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<button type="submit">Hello</button>"#,
                Some(serde_json::json!([{ "submit": false }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                r#"<button type="reset">Hello</button>"#,
                Some(serde_json::json!([{ "reset": false }])),
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside blocks.
            ("{#if a}<button>Hello</button>{/if}", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        Tester::new(ButtonHasType::NAME, ButtonHasType::PLUGIN, pass, fail).test_and_snapshot();
    }
}
