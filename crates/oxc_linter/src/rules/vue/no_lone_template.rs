use std::borrow::Cow;

use cow_utils::CowUtils;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use vue_sfc_parser::ast::{Attribute, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{element_name_eq_lower, start_tag_span, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_directive_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`<template>` require directive.")
        .with_help(
            "A bare `<template>` renders nothing extra over its children; add a directive \
             (`v-if`, `v-for`, `v-slot`/`#name`, …) or replace it with its children directly.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoLoneTemplate {
    /// When `true`, a `<template>` that only carries `id`/`ref` (and no
    /// structural directive) is also allowed. Default `false`.
    ignore_accessible: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `<template>` elements that carry no structural directive
    /// (`v-if`/`v-else-if`/`v-else`/`v-for`/`v-slot`/`#name`/`slot-scope`/
    /// `scope`/`slot`/`:slot`).
    ///
    /// ### Why is this bad?
    ///
    /// `<template>` is a grouping construct with no runtime effect of its
    /// own; without a directive that gives it a reason to exist, it does
    /// nothing but add a level of nesting for no benefit — its children
    /// could be written directly in its place.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>
    ///     <template>
    ///       <div>content</div>
    ///     </template>
    ///   </div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div>
    ///     <div>content</div>
    ///     <template v-if="cond">content</template>
    ///   </div>
    /// </template>
    /// ```
    NoLoneTemplate,
    vue,
    style,
    config = NoLoneTemplate,
    version = "1.77.0",
    short_description = "Disallow unnecessary `<template>`.",
);

impl Rule for NoLoneTemplate {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for NoLoneTemplate {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            if !element_name_eq_lower(element, "template") {
                return;
            }
            if element.attributes.iter().any(is_structural_template_attribute) {
                return;
            }
            if self.ignore_accessible
                && element.attributes.iter().any(|attribute| {
                    matches!(attribute_key_name(attribute).as_deref(), Some("id" | "ref"))
                })
            {
                return;
            }
            ctx.diagnostic(require_directive_diagnostic(start_tag_span(
                element,
                ctx.source_text(),
            )));
        });
    }
}

/// eslint-plugin-vue `no-lone-template`/`no-useless-template-attributes`'
/// shared `SPECIAL_TEMPLATE_DIRECTIVES` + `getKeyName` check: a
/// `<template>` counts as "structural" when it carries `v-if`/`v-else-if`/
/// `v-else`/`v-for`/`v-slot` (any of these as a bare directive name, `v-slot`
/// including its `#` shorthand), the bare (no `v-` prefix) deprecated Vue 2
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

    use serde_json::json;

    use super::NoLoneTemplate;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div><template v-if="c"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><template v-for="x in xs"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div><template v-slot:foo></template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div><template #foo></template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><template slot-scope="s"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><template scope="s"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><template slot="named"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><template :slot="dyn"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreAccessible`: id/ref alone is allowed with the option on.
            (
                r#"<template><div><template id="a" ref="b"></template></div></template>"#,
                Some(json!([{ "ignoreAccessible": true }])),
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
                r"<template><div><Template>content</Template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div><template></template></div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // id/ref alone is not structural, and the option defaults off.
            (
                r#"<template><div><template id="a" ref="b"></template></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `ignoreAccessible` doesn't allow arbitrary other attributes.
            (
                r#"<template><div><template class="a"></template></div></template>"#,
                Some(json!([{ "ignoreAccessible": true }])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Even at the top level of the template block (nested under the
            // SFC's own `<template>` tag, which upstream treats as a real
            // parent element too) — verified against a real
            // eslint-plugin-vue run.
            (
                r"<template><template></template></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(NoLoneTemplate::NAME, NoLoneTemplate::PLUGIN, pass, fail).test_and_snapshot();
    }
}
