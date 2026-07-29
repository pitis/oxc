use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::{Attribute, Node};

use crate::{
    rule::Rule,
    utils::{is_custom_component, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn invalid_attribute_name_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Attribute name {name} is not valid."))
        .with_help("Use a name that is a valid XML `Name` (letters, digits, `-`, `.`, `_`, `:`, not starting with a digit or `-`/`.`).")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidAttributeName;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires that attribute names — and `v-bind`'s static argument, e.g.
    /// the `foo` in `:foo` — are valid XML names, on elements that are known
    /// native HTML/SVG/MathML elements (not custom components).
    ///
    /// ### Why is this bad?
    ///
    /// An attribute name that is not a valid XML name (e.g. it contains `$`,
    /// starts with a digit, or is otherwise malformed) cannot be represented
    /// in the DOM and is either dropped or throws at runtime.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div a$b="1" />
    /// </template>
    /// ```
    ///
    /// ```vue
    /// <template>
    ///   <div :a$b="1" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div data-foo="1" :class="c" @click="go" v-if="x" />
    /// </template>
    /// ```
    ValidAttributeName,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Require valid attribute names.",
);

impl Rule for ValidAttributeName {}

impl VueTemplateRule for ValidAttributeName {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| {
            // eslint-plugin-vue's `valid-attribute-name` skips custom
            // components entirely (native elements only): a component's
            // props/attrs are not DOM attribute names.
            if is_custom_component(element) {
                return;
            }
            for attribute in &element.attributes {
                check_attribute(attribute, ctx);
            }
        });
    }
}

/// eslint-plugin-vue's `valid-attribute-name`: a plain attribute's raw name
/// must be a valid XML name; a `v-bind`/`:`/`.prop` directive with a static
/// argument validates the argument instead (the bound property/attribute
/// name). Every other directive form (`v-on`, `v-if`, `v-model`, dynamic
/// `:[arg]`, argument-less `v-bind="obj"`, custom directives, …) is not
/// checked by this rule at all.
fn check_attribute<'a>(attribute: &Attribute<'a>, ctx: &mut VueTemplateContext<'a>) {
    match &attribute.directive {
        None => {
            if !is_xml_name(attribute.name) {
                ctx.diagnostic(invalid_attribute_name_diagnostic(attribute.name, attribute.span));
            }
        }
        Some(directive) if directive.name == "bind" => {
            if let Some(argument) = &directive.argument
                && !argument.dynamic
                && !is_xml_name(argument.text)
            {
                ctx.diagnostic(invalid_attribute_name_diagnostic(argument.text, attribute.span));
            }
        }
        Some(_) => {}
    }
}

/// XML 1.0's `Name` production (the same grammar `xml-name-validator`'s
/// `name()` — what eslint-plugin-vue calls — implements): a `NameStartChar`
/// followed by zero or more `NameChar`s.
/// <https://www.w3.org/TR/xml/#NT-Name>
fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if is_name_start_char(c) => {}
        _ => return false,
    }
    chars.all(is_name_char)
}

fn is_name_start_char(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}'
        | '\u{D8}'..='\u{F6}'
        | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}'
        | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}'
        | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}'
    )
}

fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || matches!(c,
            '-' | '.' | '0'..='9'
            | '\u{B7}'
            | '\u{300}'..='\u{36F}'
            | '\u{203F}'..='\u{2040}'
        )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidAttributeName;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div data-foo="1" :class="c" @click="go" v-if="x" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // v-on's argument is not checked by this rule.
            (
                r#"<template><div @a$b="doIt" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom components are skipped entirely, even for plain attrs.
            (
                r#"<template><MyComp a$b="1" :c$d="2" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Dynamic `v-bind` arguments are not checked.
            (
                r#"<template><div :[a$b]="1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Argument-less `v-bind="obj"` is not checked.
            (
                r#"<template><div v-bind="obj" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><div a$b="1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `:` shorthand for `v-bind`'s static argument.
            (
                r#"<template><div :a$b="1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `.prop` shorthand is a `bind` directive too.
            (
                r#"<template><div .a$b="1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Longhand `v-bind:` form.
            (
                r#"<template><div v-bind:a$b="1" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidAttributeName::NAME, ValidAttributeName::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
