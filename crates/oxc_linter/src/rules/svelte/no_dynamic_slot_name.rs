use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn dynamic_name_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`<slot>` name cannot be dynamic.")
        .with_help("Use a static string for the slot name, e.g. `<slot name=\"header\" />`.")
        .with_label(span)
}

fn require_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`<slot>` name requires a value.")
        .with_help("Give the `name` attribute a static value, e.g. `<slot name=\"header\" />`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDynamicSlotName;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the `name` attribute of a `<slot>` element to be static
    /// text, disallowing dynamic slot names like `<slot name={expr} />`.
    ///
    /// ### Why is this bad?
    ///
    /// Slot names identify slots at compile time, so they cannot be
    /// computed at runtime. The Svelte compiler itself rejects a dynamic
    /// slot name with a compile error.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <slot name={dynamicName} />
    /// <slot name="prefix-{suffix}" />
    /// <slot name />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <slot name="header" />
    /// <slot />
    /// ```
    NoDynamicSlotName,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow dynamic slot names.",
);

impl Rule for NoDynamicSlotName {}

impl SvelteTemplateRule for NoDynamicSlotName {
    // Ports eslint-plugin-svelte's `no-dynamic-slot-name`. Upstream also
    // offers a constant-folding autofix; the Svelte markup pass does not
    // support fixes, so only the reports are ported.
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            if element.name != "slot" {
                return;
            }
            for attribute in &element.attributes {
                let AttributeKind::Plain { name, value, .. } = &attribute.kind else {
                    continue;
                };
                if *name != "name" {
                    continue;
                }
                match value {
                    // `<slot name />` and `<slot name="" />` both have no
                    // value parts; upstream reports `requireValue` for both.
                    None => diagnostics.push(require_value_diagnostic(attribute.span)),
                    Some(value) if value.parts.is_empty() => {
                        diagnostics.push(require_value_diagnostic(attribute.span));
                    }
                    Some(value) => {
                        for part in &value.parts {
                            if let ValuePart::Expression(expression) = part {
                                diagnostics.push(dynamic_name_diagnostic(expression.span));
                            }
                        }
                    }
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

    use super::NoDynamicSlotName;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<slot name=\"name\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // The default slot has no name attribute at all.
            ("<slot />", None, None, Some(PathBuf::from("test.svelte"))),
            // `name` attributes on other elements are not slot names.
            ("<input name={dynamic} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<Foo name={dynamic} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Static name nested inside a block.
            (
                "{#if a}<slot name=\"header\" />{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            // Bare `name` attribute has no value.
            ("<slot name />", None, None, Some(PathBuf::from("test.svelte"))),
            // An empty quoted value has no value parts either.
            ("<slot name=\"\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // Even a literal-only expression is dynamic syntax.
            ("<slot name={'name'} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<slot name={SLOT_NAME} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Quoted expression form.
            ("<slot name=\"{SLOT_NAME}\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // Mixed text/expression value: each expression part is reported.
            ("<slot name=\"a{b}c{d}\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested inside blocks.
            (
                "{#if a}<slot name={a} />{:else}<slot name={b} />{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#each items as item}<slot name={item} />{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Multiple dynamic slots each get their own report.
            (
                "<slot name={SLOT_NAME} />\n<slot name={SLOT_NAME} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoDynamicSlotName::NAME, NoDynamicSlotName::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
