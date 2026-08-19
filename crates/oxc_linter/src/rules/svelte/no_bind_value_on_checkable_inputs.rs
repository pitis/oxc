use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, DirectiveKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn checkbox_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`bind:value` does not work on checkbox inputs.")
        .with_help("Use `bind:checked` for a single checkbox, or `bind:group` for a set of them.")
        .with_label(span)
}

fn radio_diagnostic(span: Span) -> OxcDiagnostic {
    // The Svelte compiler rejects `bind:checked` on a radio, so `bind:group`
    // is the only suggestion upstream offers here.
    OxcDiagnostic::warn("`bind:value` does not work on radio inputs.")
        .with_help("Use `bind:group` to bind the selected radio of a group.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoBindValueOnCheckableInputs;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `bind:value` on `<input type="checkbox">` and
    /// `<input type="radio">`.
    ///
    /// ### Why is this bad?
    ///
    /// Checkable inputs do not report their state through `value`, so
    /// `bind:value` silently does nothing: the binding never updates when the
    /// user checks the box. `bind:checked` (a single checkbox) or
    /// `bind:group` (a set of checkboxes or radios) is what actually binds.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <input type="checkbox" bind:value={agreed} />
    /// <input type="radio" bind:value={choice} />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <input type="checkbox" bind:checked={agreed} />
    /// <input type="radio" bind:group={choice} value="a" />
    /// <input type="text" bind:value={name} />
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream also resolves a `type={…}` written as an expression when the
    /// expression is a compile-time constant. oxlint only reads a literal
    /// `type="…"`, so `type={'checkbox'}` is not matched.
    NoBindValueOnCheckableInputs,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow `bind:value` on checkbox and radio inputs.",
);

impl Rule for NoBindValueOnCheckableInputs {}

impl SvelteTemplateRule for NoBindValueOnCheckableInputs {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            if !element.name.eq_ignore_ascii_case("input") {
                return;
            }
            let Some(input_type) = element.attributes.iter().find_map(|attribute| match &attribute
                .kind
            {
                AttributeKind::Plain { name, value, .. } if name.eq_ignore_ascii_case("type") => {
                    value.as_ref().and_then(AttributeValue::as_static_text)
                }
                _ => None,
            }) else {
                return;
            };
            let is_checkbox = input_type.eq_ignore_ascii_case("checkbox");
            if !is_checkbox && !input_type.eq_ignore_ascii_case("radio") {
                return;
            }
            for attribute in &element.attributes {
                if let AttributeKind::Directive(directive) = &attribute.kind
                    && directive.kind == DirectiveKind::Bind
                    && directive.name == "value"
                {
                    diagnostics.push(if is_checkbox {
                        checkbox_diagnostic(attribute.span)
                    } else {
                        radio_diagnostic(attribute.span)
                    });
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

    use super::NoBindValueOnCheckableInputs;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<input type=\"text\" bind:value={name} />", None, None, path()),
            ("<input bind:value={name} />", None, None, path()),
            ("<input type=\"checkbox\" bind:checked={agreed} />", None, None, path()),
            ("<input type=\"radio\" bind:group={choice} value=\"a\" />", None, None, path()),
            // A plain `value` attribute is how you label a radio.
            ("<input type=\"radio\" value=\"a\" />", None, None, path()),
            // A dynamic type is not resolved (documented deviation).
            ("<input type={kind} bind:value={name} />", None, None, path()),
        ];
        let fail = vec![
            ("<input type=\"checkbox\" bind:value={agreed} />", None, None, path()),
            ("<input type=\"radio\" bind:value={choice} />", None, None, path()),
            // Matching is case-insensitive, like the DOM.
            ("<input TYPE=\"Checkbox\" bind:value={agreed} />", None, None, path()),
            // Shorthand binding.
            ("<input type=\"checkbox\" bind:value />", None, None, path()),
        ];

        Tester::new(
            NoBindValueOnCheckableInputs::NAME,
            NoBindValueOnCheckableInputs::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
