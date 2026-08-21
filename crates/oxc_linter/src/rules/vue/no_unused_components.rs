use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::Node;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        get_attribute, get_directive, is_html_svg_or_math_element_name,
        vue_casing::{camel_case, is_camel_case, is_pascal_case, pascal_case},
        walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unused_component_diagnostic(span: Span, name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The \"{name}\" component has been registered but not used."))
        .with_help("Use it in the template, or drop the registration.")
        .with_label(span)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoUnusedComponents {
    /// When the template contains a dynamic `<component :is="expr">`, any
    /// registered component might be the one it resolves to, so nothing is
    /// reported. On by default.
    pub ignore_when_binding_present: bool,
}

impl Default for NoUnusedComponents {
    fn default() -> Self {
        Self { ignore_when_binding_present: true }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports a component registered in the `components` option that the
    /// template never renders.
    ///
    /// ### Why is this bad?
    ///
    /// The registration keeps the import alive, so the component is bundled
    /// into every build of this one, and the reader has to check the whole
    /// template to find out that it is not used. It is usually the residue of
    /// a deleted piece of markup.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template><div /></template>
    /// <script>
    /// import Modal from './Modal.vue'
    /// export default { components: { Modal } }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template><Modal /></template>
    /// <script>
    /// import Modal from './Modal.vue'
    /// export default { components: { Modal } }
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// #### ignoreWhenBindingPresent
    ///
    /// `{ type: boolean, default: true }` — suppress the whole rule for a file
    /// whose template has a dynamic `<component :is="expr">`, since any
    /// registration could be what it resolves to.
    ///
    /// ```json
    /// { "vue/no-unused-components": ["error", { "ignoreWhenBindingPresent": false }] }
    /// ```
    NoUnusedComponents,
    vue,
    correctness,
    config = NoUnusedComponents,
    version = "1.80.0",
    short_description = "Disallow registering components that are not used inside templates.",
);

impl Rule for NoUnusedComponents {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoUnusedComponents {
    fn needs_script_components(&self) -> bool {
        true
    }

    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let registered: Vec<(String, Span)> = ctx.script_components().to_vec();
        if registered.is_empty() {
            return;
        }

        let mut used: FxHashSet<String> = FxHashSet::default();
        let mut dynamic_binding = false;
        walk_elements(nodes, &mut |element| {
            // A well-known HTML/SVG/MathML name is never a registered
            // component, so it cannot mark one as used.
            if !is_html_svg_or_math_element_name(element.name) {
                used.insert(element.name.to_string());
            }

            // `<component :is="…">` / `v-bind:is`.
            if let Some(attribute) = get_directive(element, "bind", Some("is"))
                && let Some(value) = &attribute.value
            {
                match string_literal_value(value.text) {
                    Some(name) => {
                        used.insert(name);
                    }
                    // A non-literal binding could resolve to anything.
                    None => dynamic_binding = true,
                }
            }

            // The plain `is="…"` attribute, including Vue 3.1's `vue:` prefix
            // for using a component on a native element.
            if let Some(attribute) = get_attribute(element, "is")
                && let Some(value) = &attribute.value
            {
                used.insert(value.text.strip_prefix("vue:").unwrap_or(value.text).to_string());
            }
        });

        if dynamic_binding && self.ignore_when_binding_present {
            return;
        }

        for (name, span) in registered {
            if is_used(&name, &used) {
                continue;
            }
            ctx.diagnostic(unused_component_diagnostic(span, &name));
        }
    }
}

/// A component registered under a PascalCase or camelCase name may be written
/// in the template in any casing that normalises back to it — `MyThing`,
/// `my-thing`, `myThing`. Registered under any other spelling (snake_case, for
/// instance) the template must match exactly, which is upstream's rule.
fn is_used(name: &str, used: &FxHashSet<String>) -> bool {
    if is_pascal_case(name) || is_camel_case(name) {
        return used.iter().any(|candidate| {
            !candidate.contains('_')
                && (name == pascal_case(candidate) || name == camel_case(candidate))
        });
    }
    used.contains(name)
}

/// The value of a directive expression that is a plain string literal, which
/// names a component statically.
fn string_literal_value(text: &str) -> Option<String> {
    let text = text.trim();
    let mut chars = text.chars();
    let quote = chars.next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let inner = text.strip_prefix(quote)?.strip_suffix(quote)?;
    // A quote inside means this is an expression, not a bare literal.
    (!inner.contains(quote)).then(|| inner.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoUnusedComponents;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            (
                "<template><Modal /></template><script>export default { components: { Modal } }</script>",
                None,
                None,
                vue(),
            ),
            // Kebab-case usage of a PascalCase registration.
            (
                "<template><my-modal /></template><script>export default { components: { MyModal } }</script>",
                None,
                None,
                vue(),
            ),
            // A literal `:is`.
            (
                "<template><component :is=\"'Modal'\" /></template><script>export default { components: { Modal } }</script>",
                None,
                None,
                vue(),
            ),
            // A plain `is` attribute.
            (
                "<template><div is=\"vue:Modal\" /></template><script>export default { components: { Modal } }</script>",
                None,
                None,
                vue(),
            ),
            // A dynamic binding suppresses the rule by default.
            (
                "<template><component :is=\"which\" /></template><script>export default { components: { Modal } }</script>",
                None,
                None,
                vue(),
            ),
            // Nothing registered.
            ("<template><div /></template><script>export default {}</script>", None, None, vue()),
        ];

        let fail = vec![
            (
                "<template><div /></template><script>export default { components: { Modal } }</script>",
                None,
                None,
                vue(),
            ),
            // Only one of the two is used.
            (
                "<template><Modal /></template><script>export default { components: { Modal, Drawer } }</script>",
                None,
                None,
                vue(),
            ),
            // With the option off, a dynamic binding no longer suppresses.
            (
                "<template><component :is=\"which\" /></template><script>export default { components: { Modal } }</script>",
                Some(json!([{ "ignoreWhenBindingPresent": false }])),
                None,
                vue(),
            ),
            // snake_case registrations must match exactly.
            (
                "<template><my-modal /></template><script>export default { components: { my_modal } }</script>",
                None,
                None,
                vue(),
            ),
        ];

        Tester::new(NoUnusedComponents::NAME, NoUnusedComponents::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
