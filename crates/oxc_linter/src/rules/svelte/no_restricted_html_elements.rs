use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_start_tag_span, walk_svelte_elements},
};

fn no_restricted_html_elements_diagnostic(message: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(message.to_string())
        .with_help("Use a different element, or remove this entry from the rule's options.")
        .with_label(span)
}

/// One `{ "elements": [...], "message": "..." }` entry, or a bare element
/// name written as a string.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum Restriction {
    Name(String),
    Group {
        elements: Vec<String>,
        #[serde(default)]
        message: Option<String>,
    },
}

// Boxed: an inline `Vec` would blow `RuleEnum`'s 16-byte budget.
#[expect(clippy::box_collection)]
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct NoRestrictedHtmlElements(Box<Vec<Restriction>>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the HTML elements named in the rule's options.
    ///
    /// ### Why is this bad?
    ///
    /// Nothing is inherently wrong with any element; a project may want to
    /// ban some for its own reasons — `<marquee>`, a raw `<button>` where a
    /// design-system component exists, and so on.
    ///
    /// ### Examples
    ///
    /// With `["error", "marquee", { "elements": ["button"], "message": "Use <Button> instead." }]`:
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <marquee>hi</marquee>
    /// <button>click</button>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <Button>click</Button>
    /// ```
    ///
    /// ### Options
    ///
    /// An array whose entries are either an element name or an object with
    /// `elements` (required) and `message` (optional).
    ///
    /// ```json
    /// {
    ///   "svelte/no-restricted-html-elements": [
    ///     "error",
    ///     "marquee",
    ///     { "elements": ["button"], "message": "Use <Button> instead." }
    ///   ]
    /// }
    /// ```
    NoRestrictedHtmlElements,
    svelte,
    restriction,
    config = NoRestrictedHtmlElements,
    version = "1.80.0",
    short_description = "Disallow specific HTML elements.",
);

impl Rule for NoRestrictedHtmlElements {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let restrictions = match value.as_array() {
            Some(entries) => entries
                .iter()
                .map(|entry| serde_json::from_value(entry.clone()))
                .collect::<Result<Vec<Restriction>, _>>()?,
            None => Vec::new(),
        };
        Ok(Self(Box::new(restrictions)))
    }
}

impl SvelteTemplateRule for NoRestrictedHtmlElements {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        if self.0.is_empty() {
            return;
        }
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Upstream only matches plain HTML elements.
            if element.is_component_like() || element.svelte_name().is_some() {
                return;
            }
            for restriction in self.0.iter() {
                let (names, message): (&[String], Option<&str>) = match restriction {
                    Restriction::Name(name) => (std::slice::from_ref(name), None),
                    Restriction::Group { elements, message } => (elements, message.as_deref()),
                };
                if names.iter().any(|name| name == element.name) {
                    let message = message.map_or_else(
                        || format!("Unexpected use of forbidden HTML element {}.", element.name),
                        ToString::to_string,
                    );
                    diagnostics.push(no_restricted_html_elements_diagnostic(
                        &message,
                        svelte_start_tag_span(element),
                    ));
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

    use super::NoRestrictedHtmlElements;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let marquee = || Some(serde_json::json!(["marquee"]));
        let with_message = || {
            Some(serde_json::json!([
                { "elements": ["button"], "message": "Use <Button> instead." }
            ]))
        };
        let pass = vec![
            // No options: nothing is restricted.
            ("<marquee>hi</marquee>", None, None, path()),
            ("<div>hi</div>", marquee(), None, path()),
            // A component that merely shares the name is not an HTML element.
            ("<Marquee>hi</Marquee>", marquee(), None, path()),
            ("<Button>click</Button>", with_message(), None, path()),
        ];
        let fail = vec![
            ("<marquee>hi</marquee>", marquee(), None, path()),
            ("<button>click</button>", with_message(), None, path()),
            ("{#if a}<marquee>hi</marquee>{/if}", marquee(), None, path()),
        ];

        Tester::new(NoRestrictedHtmlElements::NAME, NoRestrictedHtmlElements::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
