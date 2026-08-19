use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::Node;

use crate::{
    rule::Rule,
    rules::svelte::experimental_require_slot_types::declares_type,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{get_plain_attribute, svelte_scripts, svelte_start_tag_span, walk_svelte_elements},
};

fn experimental_require_strict_events_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Missing strictEvents attribute or $$Events declaration.")
        .with_help(
            "Add `strictEvents` to the `<script>` tag, or declare `interface $$Events { … }`.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ExperimentalRequireStrictEvents;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a TypeScript component to declare the events it dispatches,
    /// either with the `strictEvents` attribute on its `<script>` tag or with
    /// a `$$Events` interface.
    ///
    /// ### Why is this bad?
    ///
    /// By default a Svelte component's event map is open: a consumer can
    /// listen for an event the component never dispatches — usually a typo —
    /// and TypeScript will not complain. Declaring the events closes the map.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script lang="ts"></script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script lang="ts" strictEvents></script>
    /// ```
    ///
    /// ```svelte
    /// <script lang="ts">
    ///   interface $$Events {
    ///     click: MouseEvent;
    ///   }
    /// </script>
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream only runs on Svelte 3/4 (`$$Events` is a legacy API). oxlint
    /// cannot read the installed Svelte version; the rule is experimental and
    /// off by default. The `$$Events` declaration is matched lexically in the
    /// script text.
    ExperimentalRequireStrictEvents,
    svelte,
    restriction,
    version = "1.80.0",
    short_description = "Require `strictEvents` or a `$$Events` declaration.",
);

impl Rule for ExperimentalRequireStrictEvents {}

impl SvelteTemplateRule for ExperimentalRequireStrictEvents {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let scripts = svelte_scripts(nodes, ctx.source_text());
        if !scripts.iter().any(|script| script.typescript) {
            return;
        }
        if scripts.iter().any(|script| declares_type(script.content, "$$Events")) {
            return;
        }

        let mut reports = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            if !element.name.eq_ignore_ascii_case("script") {
                return;
            }
            if get_plain_attribute(element, "strictEvents").is_none() {
                reports.push(svelte_start_tag_span(element));
            }
        });
        for span in reports {
            ctx.diagnostic(experimental_require_strict_events_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ExperimentalRequireStrictEvents;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<script lang=\"ts\" strictEvents></script>", None, None, path()),
            (
                "<script lang=\"ts\">\n\tinterface $$Events {\n\t\tclick: MouseEvent;\n\t}\n</script>",
                None,
                None,
                path(),
            ),
            (
                "<script lang=\"ts\">\n\ttype $$Events = { click: MouseEvent };\n</script>",
                None,
                None,
                path(),
            ),
            // Not a TypeScript component.
            ("<script></script>", None, None, path()),
            ("<div />", None, None, path()),
        ];
        let fail = vec![
            ("<script lang=\"ts\"></script>", None, None, path()),
            ("<script lang=\"ts\">\n\tinterface $$EventsOther {}\n</script>", None, None, path()),
        ];

        Tester::new(
            ExperimentalRequireStrictEvents::NAME,
            ExperimentalRequireStrictEvents::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
