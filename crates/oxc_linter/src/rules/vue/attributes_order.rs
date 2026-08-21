use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vue_sfc_parser::ast::{Attribute, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::walk_elements,
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn expected_order_diagnostic(span: Span, current: &str, previous: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Attribute \"{current}\" should go before \"{previous}\"."))
        .with_help("Reorder the attributes to match the configured grouping.")
        .with_label(span)
}

/// Upstream's `ATTRS`. `OtherAttr` is an alias the `order` option accepts for
/// the three concrete attribute kinds; it is never the type of an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttributeGroup {
    Definition,
    ListRendering,
    Conditionals,
    RenderModifiers,
    Global,
    Unique,
    Slot,
    TwoWayBinding,
    OtherDirectives,
    OtherAttr,
    AttrStatic,
    AttrDynamic,
    AttrShorthandBool,
    Events,
    Content,
}

/// One entry of the `order` option: a single group or a set of groups that
/// share a position.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OrderEntry {
    One(AttributeGroup),
    Several(Vec<AttributeGroup>),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct AttributesOrderConfig {
    /// The groups, in the order they should appear. Omitting a group omits it
    /// from the check entirely.
    pub order: Option<Vec<OrderEntry>>,
    /// Within a group, require attributes to be sorted by name.
    pub alphabetical: bool,
    /// Within a group, require attributes to be sorted by source length.
    /// Takes precedence over `alphabetical` when the lengths differ.
    pub sort_line_length: bool,
}

// Boxed: `order` alone is larger than `RuleEnum`'s 16-byte budget.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AttributesOrder(Box<AttributesOrderConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a consistent order for an element's attributes: structural
    /// directives first (`is`, `v-for`, `v-if`), then identity (`id`, `ref`,
    /// `key`), then bindings, then events, then content directives.
    ///
    /// ### Why is this bad?
    ///
    /// Purely a consistency rule, but a load-bearing one on large templates:
    /// when every tag lists its attributes in the same order, the thing you
    /// are looking for — which element is this, is it conditional, what does
    /// it emit — is always in the same place.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div @click="go" v-if="show" id="a" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-if="show" id="a" @click="go" />
    /// </template>
    /// ```
    ///
    /// ### Options
    ///
    /// #### order
    ///
    /// The groups in the order they should appear. An array element groups
    /// several kinds at one position. Any group left out is not checked.
    /// Defaults to:
    ///
    /// ```json
    /// {
    ///   "vue/attributes-order": ["error", {
    ///     "order": [
    ///       "DEFINITION", "LIST_RENDERING", "CONDITIONALS", "RENDER_MODIFIERS",
    ///       "GLOBAL", ["UNIQUE", "SLOT"], "TWO_WAY_BINDING", "OTHER_DIRECTIVES",
    ///       "OTHER_ATTR", "EVENTS", "CONTENT"
    ///     ]
    ///   }]
    /// }
    /// ```
    ///
    /// `OTHER_ATTR` is an alias for `ATTR_DYNAMIC`, `ATTR_STATIC` and
    /// `ATTR_SHORTHAND_BOOL` together.
    ///
    /// #### alphabetical
    ///
    /// `{ type: boolean, default: false }` — also sort by name within a group.
    ///
    /// #### sortLineLength
    ///
    /// `{ type: boolean, default: false }` — also sort by source length within
    /// a group, taking precedence over `alphabetical` when they differ.
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Upstream is auto-fixable; this is not, because the `<template>` pass
    /// cannot emit fixes yet.
    ///
    /// Upstream throws a configuration error when `order` names `OTHER_ATTR`
    /// alongside one of the three kinds it expands to. Here the alias is simply
    /// expanded and the later position wins, because a rule cannot fail
    /// configuration validation from inside its own run.
    AttributesOrder,
    vue,
    style,
    config = AttributesOrder,
    version = "1.80.0",
    short_description = "Enforce order of attributes.",
);

impl Rule for AttributesOrder {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

/// Upstream's `otherAttrs`, which `OTHER_ATTR` expands to.
const OTHER_ATTRS: [AttributeGroup; 3] =
    [AttributeGroup::AttrDynamic, AttributeGroup::AttrStatic, AttributeGroup::AttrShorthandBool];

/// Upstream's default `attributeOrder`.
fn default_order() -> Vec<Vec<AttributeGroup>> {
    vec![
        vec![AttributeGroup::Definition],
        vec![AttributeGroup::ListRendering],
        vec![AttributeGroup::Conditionals],
        vec![AttributeGroup::RenderModifiers],
        vec![AttributeGroup::Global],
        vec![AttributeGroup::Unique, AttributeGroup::Slot],
        vec![AttributeGroup::TwoWayBinding],
        vec![AttributeGroup::OtherDirectives],
        OTHER_ATTRS.to_vec(),
        vec![AttributeGroup::Events],
        vec![AttributeGroup::Content],
    ]
}

fn is_v_bind(attribute: &Attribute<'_>) -> bool {
    attribute.directive.as_ref().is_some_and(|directive| directive.name == "bind")
}

/// `v-bind="object"` — a whole-object spread rather than one named prop.
fn is_v_bind_object(attribute: &Attribute<'_>) -> bool {
    attribute
        .directive
        .as_ref()
        .is_some_and(|directive| directive.name == "bind" && directive.argument.is_none())
}

/// Upstream's `isVAttributeOrVBindOrVModel`: the three forms that set a prop,
/// and so interact with `v-bind="object"`'s ordering.
fn is_prop_like(attribute: &Attribute<'_>) -> bool {
    match &attribute.directive {
        None => true,
        Some(directive) => directive.name == "bind" || directive.name == "model",
    }
}

/// Upstream's `getAttributeType`.
fn attribute_group(attribute: &Attribute<'_>) -> AttributeGroup {
    let prop_name = match &attribute.directive {
        Some(directive) if directive.name != "bind" => {
            return match directive.name {
                "for" => AttributeGroup::ListRendering,
                "if" | "else-if" | "else" | "show" | "cloak" => AttributeGroup::Conditionals,
                "pre" | "once" => AttributeGroup::RenderModifiers,
                "model" => AttributeGroup::TwoWayBinding,
                "on" => AttributeGroup::Events,
                "html" | "text" => AttributeGroup::Content,
                "slot" => AttributeGroup::Slot,
                "is" => AttributeGroup::Definition,
                _ => AttributeGroup::OtherDirectives,
            };
        }
        // `:[dynamic]` names no single prop, so upstream treats it as unnamed.
        Some(directive) => directive
            .argument
            .as_ref()
            .filter(|argument| !argument.dynamic)
            .map_or("", |argument| argument.text),
        None => attribute.name,
    };

    match prop_name {
        "is" => AttributeGroup::Definition,
        "id" => AttributeGroup::Global,
        "ref" | "key" => AttributeGroup::Unique,
        "slot" | "slot-scope" => AttributeGroup::Slot,
        _ => {
            if is_v_bind(attribute) {
                AttributeGroup::AttrDynamic
            } else if attribute.value.is_none() {
                AttributeGroup::AttrShorthandBool
            } else {
                AttributeGroup::AttrStatic
            }
        }
    }
}

/// Upstream's `getAttributeName`, used only by `alphabetical`.
fn attribute_name(attribute: &Attribute<'_>, source_text: &str) -> String {
    let Some(directive) = &attribute.directive else { return attribute.name.to_string() };
    if directive.name == "bind" {
        return directive
            .argument
            .as_ref()
            .map_or_else(String::new, |argument| source_text[argument.span].to_string());
    }
    let mut text = format!("v-{}", directive.name);
    if let Some(argument) = &directive.argument {
        text.push(':');
        text.push_str(&source_text[argument.span]);
    }
    for modifier in &directive.modifiers {
        text.push('.');
        text.push_str(modifier);
    }
    text
}

impl AttributesOrder {
    /// The configured order, flattened to `group -> position`. A group that is
    /// absent has no position and is skipped entirely.
    fn positions(&self) -> Vec<(AttributeGroup, usize)> {
        let order = self.0.order.as_ref().map_or_else(default_order, |entries| {
            entries
                .iter()
                .map(|entry| {
                    let groups = match entry {
                        OrderEntry::One(group) => std::slice::from_ref(group).to_vec(),
                        OrderEntry::Several(groups) => groups.clone(),
                    };
                    // Expand the `OTHER_ATTR` alias in place.
                    groups
                        .into_iter()
                        .flat_map(|group| {
                            if group == AttributeGroup::OtherAttr {
                                OTHER_ATTRS.to_vec()
                            } else {
                                vec![group]
                            }
                        })
                        .collect()
                })
                .collect()
        });
        order
            .into_iter()
            .enumerate()
            .flat_map(|(index, groups)| groups.into_iter().map(move |group| (group, index)))
            .collect()
    }
}

impl VueTemplateRule for AttributesOrder {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let positions = self.positions();
        let position_of = |group: AttributeGroup| {
            positions.iter().find(|(candidate, _)| *candidate == group).map(|(_, index)| *index)
        };

        let mut reports = Vec::new();
        walk_elements(nodes, &mut |element| {
            if element.attributes.len() <= 1 {
                return;
            }

            // Upstream drops a `v-bind="object"` that sits next to a prop-like
            // attribute: moving it there changes which side wins, so its
            // position is not the rule's to enforce.
            let attributes: Vec<&Attribute<'_>> = element
                .attributes
                .iter()
                .enumerate()
                .filter(|(index, attribute)| {
                    if !is_v_bind_object(attribute) {
                        return true;
                    }
                    let neighbour_is_prop_like = index
                        .checked_sub(1)
                        .and_then(|previous| element.attributes.get(previous))
                        .is_some_and(is_prop_like)
                        || element.attributes.get(index + 1).is_some_and(is_prop_like);
                    !neighbour_is_prop_like
                })
                .map(|(_, attribute)| attribute)
                .collect();

            // Upstream's `getPositionFromAttrIndex`: a surviving
            // `v-bind="object"` borrows the position of the next prop-like
            // attribute, so reordering cannot change behaviour.
            let position_at = |mut index: usize| -> Option<usize> {
                if is_v_bind_object(attributes[index])
                    && let Some(offset) = attributes[index + 1..]
                        .iter()
                        .position(|next| is_prop_like(next) && !is_v_bind_object(next))
                {
                    index += 1 + offset;
                }
                position_of(attribute_group(attributes[index]))
            };

            let ordered: Vec<(&Attribute<'_>, usize)> = (0..attributes.len())
                .filter_map(|index| {
                    position_at(index).map(|position| (attributes[index], position))
                })
                .collect();
            if ordered.len() <= 1 {
                return;
            }

            let (mut previous, mut previous_position) = ordered[0];
            for &(attribute, position) in &ordered[1..] {
                let mut valid = previous_position <= position;
                if valid && previous_position == position {
                    let mut sorted_by_length = false;
                    if self.0.sort_line_length {
                        let previous_length = previous.span.size();
                        let current_length = attribute.span.size();
                        if previous_length != current_length {
                            valid = previous_length < current_length;
                            sorted_by_length = true;
                        }
                    }
                    if self.0.alphabetical && !sorted_by_length {
                        valid = is_alphabetical(previous, attribute, source_text);
                    }
                }

                if valid {
                    previous = attribute;
                    previous_position = position;
                } else {
                    reports.push(expected_order_diagnostic(
                        attribute.span,
                        &source_text[attribute.name_span],
                        &source_text[previous.name_span],
                    ));
                }
            }
        });

        for diagnostic in reports {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// Upstream's `isAlphabetical`, including its tie-break: for the same name a
/// plain attribute sorts before its `v-bind` spelling.
fn is_alphabetical(previous: &Attribute<'_>, current: &Attribute<'_>, source_text: &str) -> bool {
    let previous_name = attribute_name(previous, source_text);
    let current_name = attribute_name(current, source_text);
    if previous_name == current_name {
        return !is_v_bind(previous) || is_v_bind(current);
    }
    previous_name < current_name
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::AttributesOrder;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            ("<template><div v-if=\"x\" id=\"a\" @click=\"go\" /></template>", None, None, vue()),
            // One attribute is never out of order.
            ("<template><div @click=\"go\" /></template>", None, None, vue()),
            // The canonical full ordering.
            (
                "<template><div is=\"x\" v-for=\"i in xs\" v-if=\"x\" v-once id=\"a\" ref=\"r\" v-model=\"m\" v-custom :dyn=\"d\" static=\"s\" @click=\"go\" v-text=\"t\" /></template>",
                None,
                None,
                vue(),
            ),
            // `v-bind="object"` next to a prop-like attribute is not ordered.
            ("<template><div id=\"x\" :foo=\"a\" v-bind=\"obj\" /></template>", None, None, vue()),
            // alphabetical within a group.
            (
                "<template><div a=\"1\" b=\"2\" c=\"3\" /></template>",
                Some(json!([{ "alphabetical": true }])),
                None,
                vue(),
            ),
            // A group left out of `order` is not checked.
            (
                "<template><div @click=\"go\" v-if=\"x\" /></template>",
                Some(json!([{ "order": ["EVENTS"] }])),
                None,
                vue(),
            ),
        ];

        let fail = vec![
            ("<template><div @click=\"go\" v-if=\"x\" /></template>", None, None, vue()),
            ("<template><div id=\"a\" is=\"x\" /></template>", None, None, vue()),
            ("<template><div v-text=\"t\" @click=\"go\" /></template>", None, None, vue()),
            // alphabetical violated.
            (
                "<template><div b=\"2\" a=\"1\" /></template>",
                Some(json!([{ "alphabetical": true }])),
                None,
                vue(),
            ),
            // sortLineLength violated.
            (
                "<template><div longer=\"22\" a=\"1\" /></template>",
                Some(json!([{ "sortLineLength": true }])),
                None,
                vue(),
            ),
            // A custom order.
            (
                "<template><div v-if=\"x\" @click=\"go\" /></template>",
                Some(json!([{ "order": ["EVENTS", "CONDITIONALS"] }])),
                None,
                vue(),
            ),
        ];

        Tester::new(AttributesOrder::NAME, AttributesOrder::PLUGIN, pass, fail).test_and_snapshot();
    }
}
