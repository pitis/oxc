use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{svelte_scripts, walk_svelte_elements},
};

fn experimental_require_slot_types_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Missing $$Slots declaration.")
        .with_help("Declare `interface $$Slots { … }` in the component's `<script lang=\"ts\">`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ExperimentalRequireSlotTypes;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a TypeScript component that uses `<slot>` to declare a
    /// `$$Slots` interface describing them.
    ///
    /// ### Why is this bad?
    ///
    /// Without `$$Slots`, nothing checks that a consumer fills the slots the
    /// component actually has, or passes the right `let:` props. Declaring
    /// the type makes the component's slot contract part of its API.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script lang="ts"></script>
    ///
    /// <slot />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script lang="ts">
    ///   interface $$Slots {
    ///     default: Record<string, never>;
    ///   }
    /// </script>
    ///
    /// <slot />
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream only runs on Svelte 3/4, or on Svelte 5 outside runes mode
    /// (`$$Slots` is a legacy API superseded by snippet props). oxlint cannot
    /// read the installed Svelte version; the rule is experimental and off by
    /// default. The `$$Slots` declaration is matched lexically in the script
    /// text, so an `interface $$Slots` written inside a comment or a string
    /// counts as a declaration.
    ExperimentalRequireSlotTypes,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Require a `$$Slots` type declaration when using `<slot>`.",
);

impl Rule for ExperimentalRequireSlotTypes {}

impl SvelteTemplateRule for ExperimentalRequireSlotTypes {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let scripts = svelte_scripts(nodes, ctx.source_text());
        // The rule only applies to TypeScript components.
        if !scripts.iter().any(|script| script.typescript) {
            return;
        }
        let mut has_slot = false;
        walk_svelte_elements(nodes, &mut |element| {
            if element.name == "slot" {
                has_slot = true;
            }
        });
        if !has_slot {
            return;
        }
        let declares_slots = scripts.iter().any(|script| declares_type(script.content, "$$Slots"));
        if !declares_slots {
            // Upstream reports at the very start of the file.
            ctx.diagnostic(experimental_require_slot_types_diagnostic(Span::empty(0)));
        }
    }
}

/// Whether the script declares `interface <name>` or `type <name>`.
pub(super) fn declares_type(script: &str, name: &str) -> bool {
    ["interface", "type"].iter().any(|keyword| {
        script.match_indices(keyword).any(|(index, _)| {
            let after = script[index + keyword.len()..].trim_start();
            after.strip_prefix(name).is_some_and(|rest| {
                // The declared name must end here, not continue into a
                // longer identifier.
                !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ExperimentalRequireSlotTypes;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            (
                "<script lang=\"ts\">\n\tinterface $$Slots {\n\t\tdefault: Record<string, never>;\n\t}\n</script>\n<slot />",
                None,
                None,
                path(),
            ),
            (
                "<script lang=\"ts\">\n\ttype $$Slots = { default: Record<string, never> };\n</script>\n<slot />",
                None,
                None,
                path(),
            ),
            // No slot to describe.
            ("<script lang=\"ts\"></script>\n<div />", None, None, path()),
            // Not a TypeScript component.
            ("<script></script>\n<slot />", None, None, path()),
            ("<slot />", None, None, path()),
        ];
        let fail = vec![
            ("<script lang=\"ts\"></script>\n<slot />", None, None, path()),
            (
                "<script lang=\"ts\">\n\tinterface $$SlotsOther {}\n</script>\n<slot name=\"x\" />",
                None,
                None,
                path(),
            ),
        ];

        Tester::new(
            ExperimentalRequireSlotTypes::NAME,
            ExperimentalRequireSlotTypes::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
