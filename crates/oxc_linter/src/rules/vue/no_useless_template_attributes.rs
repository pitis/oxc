use std::borrow::Cow;

use cow_utils::CowUtils;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::{Attribute, Node};

use crate::{
    rule::Rule,
    utils::{element_name_eq_lower, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_attribute_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected useless attribute on `<template>`.")
        .with_help("`<template>` isn't rendered, so non-structural attributes on it have no effect; remove it.")
        .with_label(span)
}

fn unexpected_directive_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected useless directive on `<template>`.")
        .with_help("`<template>` isn't rendered, so non-structural directives on it have no effect; remove it.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoUselessTemplateAttributes;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// On a `<template>` that already carries a structural directive
    /// (`v-if`/`v-for`/`v-slot`/…), disallows every *other* attribute except
    /// `key` (which `v-for` legitimately needs).
    ///
    /// ### Why is this bad?
    ///
    /// `<template>` is never itself rendered as a DOM element — only its
    /// children are — so any attribute on it besides the ones that control
    /// its own structural behavior (and `key`) is simply discarded and
    /// misleads readers into thinking it does something.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <template v-if="cond" id="foo" class="bar"></template>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <template v-if="cond"></template>
    ///   <template v-for="item in items" :key="item.id"></template>
    /// </template>
    /// ```
    NoUselessTemplateAttributes,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Disallow useless attribute on `<template>`.",
);

impl Rule for NoUselessTemplateAttributes {}

impl VueTemplateRule for NoUselessTemplateAttributes {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if !element_name_eq_lower(element, "template") {
                return;
            }
            // Only applies once the template already has a reason to exist;
            // a template with no structural attribute at all is
            // `no-lone-template`'s concern, not this rule's.
            if !element.attributes.iter().any(is_structural_template_attribute) {
                return;
            }
            for attribute in &element.attributes {
                if is_structural_template_attribute(attribute) {
                    continue;
                }
                if attribute_key_name(attribute).as_deref() == Some("key") {
                    continue;
                }
                let diagnostic = if attribute.directive.is_some() {
                    unexpected_directive_diagnostic(attribute.span)
                } else {
                    unexpected_attribute_diagnostic(attribute.span)
                };
                ctx.diagnostic(diagnostic);
            }
        });
    }
}

/// eslint-plugin-vue's local `SPECIAL_TEMPLATE_DIRECTIVES` +
/// `isFragmentTemplateAttribute` (duplicated in `no-lone-template.js` too —
/// this fork mirrors that duplication in
/// `no_lone_template.rs`/`no_useless_template_attributes.rs` rather than
/// sharing it, matching upstream's own structure): a `<template>` attribute
/// counts as "structural" when it's `v-if`/`v-else-if`/`v-else`/`v-for`/
/// `v-slot` (any of these as a bare directive name, `v-slot` including its
/// `#` shorthand), the bare (no `v-` prefix) deprecated Vue 2
/// `slot-scope`/`scope` attributes — which vue-eslint-parser recognizes as
/// directives despite the missing prefix, verified against a real
/// eslint-plugin-vue run; this fork's parser does not special-case them, so
/// they surface here as plain attributes instead — or a `slot` attribute in
/// either its plain (`slot="x"`, Vue 2 style) or bound (`:slot="x"`) form.
fn is_structural_template_attribute(attribute: &Attribute<'_>) -> bool {
    if let Some(directive) = &attribute.directive {
        if matches!(directive.name, "if" | "else" | "else-if" | "for" | "slot") {
            return true;
        }
    } else if attribute.name.eq_ignore_ascii_case("slot-scope")
        || attribute.name.eq_ignore_ascii_case("scope")
    {
        return true;
    }
    attribute_key_name(attribute).as_deref() == Some("slot")
}

/// eslint-plugin-vue's local `getKeyName`: for a `v-bind`/`:`/`.` directive
/// with a static argument, the (lowercased) argument name; for a plain
/// attribute, the (lowercased) attribute name; `None` for any other
/// directive (including a dynamic-argument bind).
fn attribute_key_name<'a>(attribute: &Attribute<'a>) -> Option<Cow<'a, str>> {
    match &attribute.directive {
        None => Some(attribute.name.cow_to_ascii_lowercase()),
        Some(directive) => {
            if directive.name != "bind" {
                return None;
            }
            let argument = directive.argument.as_ref()?;
            if argument.dynamic {
                return None;
            }
            Some(argument.text.cow_to_ascii_lowercase())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUselessTemplateAttributes;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><template v-if="c"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `key` is exempt even though it's not itself structural.
            (
                r#"<template><template v-if="c" :key="k"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="x in xs" :key="x.id"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No structural attribute at all: not this rule's concern.
            (
                r#"<template><template id="a"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><template></template></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            // Element names are matched case-insensitively: upstream's
            // `VElement[name='…']` selectors see vue-eslint-parser's
            // *lowercased* `name`, so `<Template>`/`<Component>` are the same
            // element to them (verified against real eslint-plugin-vue
            // 10.10.0).
            (
                r#"<template><div><Template v-if="ok" class="c">x</Template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-if="c" id="a"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A plain attribute and a directive attribute, both useless.
            (
                r#"<template><template v-if="c" id="a" class="b"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="x in xs" id="a"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // A non-structural directive on a structural template.
            (
                r#"<template><template v-if="c" @click="onClick"></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(
            NoUselessTemplateAttributes::NAME,
            NoUselessTemplateAttributes::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
