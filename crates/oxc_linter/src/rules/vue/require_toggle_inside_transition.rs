use std::fmt::Write as _;

use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Attribute, Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        get_attribute, get_directive, has_directive, is_custom_component, start_tag_span,
        walk_elements,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn expected_diagnostic(allowed_directives: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The element inside `<transition>` is expected to have a {allowed_directives} directive."
    ))
    .with_help(
        "Add a toggling directive so the transition has something to animate; a `:key` binding also satisfies this.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct RequireToggleInsideTransitionConfig {
    /// Additional directive names (without the `v-` prefix) that, like
    /// `v-if`/`v-show`, count as toggling the transitioned element. Default:
    /// none.
    additional_directives: Vec<String>,
}

// Boxed (like `vue/valid-v-on`'s `ValidVOn`): `additional_directives:
// Vec<String>` is 24 bytes unboxed, which would make this the largest
// `RuleEnum` variant and grow every rule's in-memory representation from 16
// to 24 bytes. `Box` keeps this rule's own footprint at one pointer (8 bytes).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct RequireToggleInsideTransition(Box<RequireToggleInsideTransitionConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the (first) element child of a `<transition>`/`<Transition>`
    /// in Vue `<template>` blocks to control its own display with `v-if`,
    /// `v-show` (or an `additionalDirectives` name), or a `:key` binding.
    ///
    /// ### Why is this bad?
    ///
    /// `<transition>` animates its child appearing, disappearing, or being
    /// replaced. Without something that toggles the child (or gives it a
    /// changing `:key`, which makes Vue replace rather than patch it), the
    /// child never actually enters or leaves, so the `<transition>` never
    /// has anything to animate.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <transition><div>content</div></transition>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <transition><div v-if="show">content</div></transition>
    /// </template>
    /// ```
    RequireToggleInsideTransition,
    vue,
    correctness,
    config = RequireToggleInsideTransition,
    version = "1.77.0",
    short_description = "Require control the display of the content inside `<transition>`.",
);

impl Rule for RequireToggleInsideTransition {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for RequireToggleInsideTransition {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue's selector matches `VElement[name='transition']`;
            // vue-eslint-parser lowercases `.name` (case-insensitive tag-name
            // parsing, unlike `.rawName`, which this fork's `Element::name`
            // corresponds to), so both `<transition>` and `<Transition>` (and
            // any other casing) match — verified against real eslint-plugin-vue
            // 10.10.0 + vue-eslint-parser 10.4.1, which fires for `<Transition>`
            // and even `<TRANSITION>`, but not for `<TransitionGroup>` or any
            // other custom component name. Case-INsensitive compare here is the
            // deliberate exception to this fork's usual case-sensitive
            // (rawName-style) element-name comparisons.
            if !element.name.eq_ignore_ascii_case("transition") {
                return;
            }
            // Only the first element child is checked (`<transition>` only
            // ever has one meaningful child); matches upstream's
            // `node.parent.children.find(utils.isVElement) !== node` guard.
            let Some(first_child) = element.children.iter().find_map(|child| {
                if let Node::Element(child) = child { Some(child) } else { None }
            }) else {
                return;
            };
            self.verify_inside_element(first_child, element, ctx);
        });
    }
}

impl RequireToggleInsideTransition {
    fn verify_inside_element<'a>(
        &self,
        element: &Element<'a>,
        transition: &Element<'a>,
        ctx: &mut VueTemplateContext<'a>,
    ) {
        if is_custom_component(element) {
            return;
        }

        let v_bind_appear = get_directive(transition, "bind", Some("appear"));
        if get_attribute(transition, "appear").is_some()
            || v_bind_appear.is_some_and(is_valid_bind_appear)
        {
            return;
        }

        if element.name.eq_ignore_ascii_case("slot") {
            return;
        }
        if self.allowed_directives().any(|directive| has_directive(element, directive, None)) {
            return;
        }
        if has_directive(element, "bind", Some("key")) {
            return;
        }

        ctx.diagnostic(expected_diagnostic(
            &self.allowed_directives_string(),
            start_tag_span(element, ctx.source_text()),
        ));
    }

    fn allowed_directives(&self) -> impl Iterator<Item = &str> {
        ["if", "show"].into_iter().chain(self.0.additional_directives.iter().map(String::as_str))
    }

    /// eslint-plugin-vue's `createDirectiveList`: `` `v-if` ``, then
    /// `` `v-if`, `v-show` ``, then `` `v-if`, `v-show` or `v-x` `` for 3+.
    fn allowed_directives_string(&self) -> String {
        let directives: Vec<&str> = self.allowed_directives().collect();
        let mut out = String::new();
        for (index, directive) in directives.iter().enumerate() {
            if index == 0 {
                let _ = write!(out, "`v-{directive}`");
            } else if index < directives.len() - 1 {
                let _ = write!(out, ", `v-{directive}`");
            } else {
                let _ = write!(out, " or `v-{directive}`");
            }
        }
        out
    }
}

/// eslint-plugin-vue's `isValidBindAppear`: a `:appear` bound to a literal
/// `false` disqualifies the exemption; anything else (including a literal
/// other than `false`, or any non-literal expression) is treated as valid.
/// Approximated textually rather than by parsing: this only under-counts a
/// parenthesized/whitespace-padded literal `false` (e.g. `:appear="(false)"`)
/// as "valid" when upstream would not be — the safe direction, since it can
/// only cause this rule to *miss* a violation upstream would flag, never
/// report one upstream wouldn't.
fn is_valid_bind_appear(attribute: &Attribute<'_>) -> bool {
    attribute.value.as_ref().is_none_or(|value| value.text.trim() != "false")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::RequireToggleInsideTransition;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><transition><div v-if="a">x</div></transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><transition><div v-show="a">x</div></transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // PascalCase `<Transition>` is matched too.
            (
                r#"<template><Transition><div v-if="a">x</div></Transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A custom component child is always exempt.
            (
                r"<template><transition><MyComp>x</MyComp></transition></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A bare `appear` attribute on `<transition>` exempts its child
            // entirely.
            (
                r"<template><transition appear><div>x</div></transition></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `:appear="true"` (and any non-`false`-literal expression).
            (
                r#"<template><transition :appear="true"><div>x</div></transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><transition :appear="x"><div>x</div></transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A `:key` binding satisfies the requirement on its own.
            (
                r#"<template><transition><div :key="k">x</div></transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `<slot>` children are always exempt.
            (
                r"<template><transition><slot>x</slot></transition></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `TransitionGroup` is not `<transition>`: not this rule's
            // concern.
            (
                r"<template><TransitionGroup><div>x</div></TransitionGroup></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `additionalDirectives` option.
            (
                r#"<template><transition><div v-my-toggle="a">x</div></transition></template>"#,
                Some(json!([{ "additionalDirectives": ["my-toggle"] }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r"<template><transition><div>x</div></transition></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `:appear="false"` (a literal `false`) does NOT exempt the child.
            (
                r#"<template><transition :appear="false"><div>x</div></transition></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Only the first element child is checked; a second untoggled
            // child is not separately reported, but the first still is.
            (
                r"<template><transition><div>x</div><div>y</div></transition></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            RequireToggleInsideTransition::NAME,
            RequireToggleInsideTransition::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
