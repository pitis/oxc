use std::borrow::Cow;

use cow_utils::CowUtils;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::{Attribute, Node};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn duplicate_attribute_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Duplicate attribute '{name}'."))
        .with_help("Remove the duplicated attribute; only the last one takes effect.")
        .with_label(span)
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoDuplicateAttributes {
    /// Whether a plain `class` and a `:class` binding may coexist. Default `true`.
    allow_coexist_class: bool,
    /// Whether a plain `style` and a `:style` binding may coexist. Default `true`.
    allow_coexist_style: bool,
}

impl Default for NoDuplicateAttributes {
    fn default() -> Self {
        Self { allow_coexist_class: true, allow_coexist_style: true }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplication of attributes in Vue `<template>` blocks.
    /// A plain attribute and its `v-bind` form (e.g. `foo` and `:foo`) count
    /// as duplicates; by default `class`/`style` are allowed to coexist with
    /// their bound forms because Vue merges them.
    ///
    /// ### Why is this bad?
    ///
    /// When duplicate attributes exist, only the last one is used and the
    /// rest are silently ignored — which is almost always a mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div foo="abc" :foo="def" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div :foo="def" class="a" :class="b" />
    /// </template>
    /// ```
    NoDuplicateAttributes,
    vue,
    correctness,
    config = NoDuplicateAttributes,
    version = "1.77.0",
    short_description = "Disallow duplication of attributes.",
);

impl Rule for NoDuplicateAttributes {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoDuplicateAttributes {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        self.walk(nodes, ctx);
    }
}

impl NoDuplicateAttributes {
    fn walk<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            self.check_element_attributes(&element.attributes, ctx);
        });
    }

    fn check_element_attributes<'a>(
        &self,
        attributes: &[Attribute<'a>],
        ctx: &mut VueTemplateContext<'a>,
    ) {
        let mut directive_names: FxHashSet<Cow<'a, str>> = FxHashSet::default();
        let mut attribute_names: FxHashSet<Cow<'a, str>> = FxHashSet::default();

        for attribute in attributes {
            let Some((name, is_directive)) = attribute_name(attribute) else { continue };

            let is_duplicate = if (self.allow_coexist_style && name == "style")
                || (self.allow_coexist_class && name == "class")
            {
                // The bound and plain forms may coexist; only a repeat of the
                // same form is a duplicate.
                if is_directive {
                    directive_names.contains(&name)
                } else {
                    attribute_names.contains(&name)
                }
            } else {
                directive_names.contains(&name) || attribute_names.contains(&name)
            };

            if is_duplicate {
                ctx.diagnostic(duplicate_attribute_diagnostic(&name, attribute.span));
            }

            if is_directive {
                directive_names.insert(name);
            } else {
                attribute_names.insert(name);
            }
        }
    }
}

/// eslint-plugin-vue `no-duplicate-attributes`'s `getName`: plain attributes
/// count under their own name, `v-bind` directives under their static
/// argument; every other directive (including dynamic `:[arg]`) is ignored.
/// Names compare case-insensitively, matching vue-eslint-parser's lowercased
/// identifier names.
fn attribute_name<'a>(attribute: &Attribute<'a>) -> Option<(Cow<'a, str>, bool)> {
    match &attribute.directive {
        None => Some((attribute.name.cow_to_ascii_lowercase(), false)),
        Some(directive) if directive.name == "bind" => {
            let argument = directive.argument.as_ref()?;
            if argument.dynamic {
                return None;
            }
            Some((argument.text.cow_to_ascii_lowercase(), true))
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::NoDuplicateAttributes;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div foo="a" bar="b" :baz="c" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Bound and plain class/style coexist by default.
            (
                r#"<template><div class="a" :class="b" style="c" :style="d" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Non-bind directives are not checked.
            (
                r#"<template><button @click="a" @click.stop="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Same name on different elements is fine.
            (
                r#"<template><div foo="a" /><div foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><div foo="a" foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Plain + bound form of the same attribute.
            (
                r#"<template><div foo="a" :foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :foo="a" v-bind:foo="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Two bound classes are duplicates even with coexistence allowed.
            (
                r#"<template><div :class="a" :class="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Coexistence disabled by options.
            (
                r#"<template><div class="a" :class="b" /></template>"#,
                Some(json!([{ "allowCoexistClass": false }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div style="a" :style="b" /></template>"#,
                Some(json!([{ "allowCoexistStyle": false }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoDuplicateAttributes::NAME, NoDuplicateAttributes::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
