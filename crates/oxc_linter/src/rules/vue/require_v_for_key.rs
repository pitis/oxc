use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_vue_parser::ast::{Element, Node};

use crate::{
    rule::Rule,
    utils::{
        VUE_RESERVED_DEPRECATED_HTML_ELEMENTS, VUE_RESERVED_HTML_ELEMENTS,
        VUE_RESERVED_KEBAB_CASE_ELEMENTS, VUE_RESERVED_SVG_ELEMENTS,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn require_key_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Elements in iteration expect to have 'v-bind:key' directives.")
        .with_help("Add a `:key` binding so Vue can track each node's identity, e.g. `v-for=\"item in items\" :key=\"item.id\"`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireVForKey;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `v-bind:key` with `v-for` directives in Vue `<template>` blocks.
    ///
    /// ### Why is this bad?
    ///
    /// Without a `key`, Vue patches list elements in place: when the list
    /// reorders, element state (form inputs, component state, transition
    /// state) stays at the old position instead of moving with the data.
    /// A unique `key` lets Vue track each node's identity.
    ///
    /// Custom components are not reported by this rule (that case is covered
    /// by `vue/valid-v-for` in eslint-plugin-vue).
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="todo in todos" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-for="todo in todos" :key="todo.id" />
    /// </template>
    /// ```
    RequireVForKey,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Require `v-bind:key` with `v-for` directives.",
);

impl Rule for RequireVForKey {}

impl VueTemplateRule for RequireVForKey {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk(nodes, ctx);
    }
}

fn walk<'a>(nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
    for node in nodes {
        let Node::Element(element) = node else { continue };
        if has_directive(element, "for", None) {
            check_key(element, ctx);
        }
        walk(&element.children, ctx);
    }
}

/// eslint-plugin-vue `require-v-for-key`'s `checkKey`: a keyed element is
/// fine; `<template>`/`<slot>` push the requirement down to their children;
/// anything else must be keyed unless it is a custom component.
fn check_key<'a>(element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
    if has_directive(element, "bind", Some("key")) {
        return;
    }
    if element.name == "template" || element.name == "slot" {
        for child in &element.children {
            if let Node::Element(child_element) = child {
                check_key(child_element, ctx);
            }
        }
    } else if !is_custom_component(element) {
        ctx.diagnostic(require_key_diagnostic(start_tag_span(element, ctx.source_text())));
    }
}

fn has_directive(element: &Element<'_>, name: &str, argument: Option<&str>) -> bool {
    element.attributes.iter().any(|attribute| {
        attribute.directive.as_ref().is_some_and(|directive| {
            directive.name == name
                && match argument {
                    None => true,
                    Some(expected) => directive
                        .argument
                        .as_ref()
                        .is_some_and(|arg| !arg.dynamic && arg.text == expected),
                }
        })
    })
}

/// eslint-plugin-vue's `isCustomComponent`: an `is` attribute / `v-bind:is` /
/// `v-is` makes any element a component; otherwise an element is custom when
/// its name is not a well-known HTML/SVG/MathML element. SFC template names
/// are case-sensitive (`<DIV>` resolves as a component in an SFC).
fn is_custom_component(element: &Element<'_>) -> bool {
    let has_is = element.attributes.iter().any(|attribute| {
        if let Some(directive) = &attribute.directive {
            directive.name == "is"
                || (directive.name == "bind"
                    && directive
                        .argument
                        .as_ref()
                        .is_some_and(|arg| !arg.dynamic && arg.text == "is"))
        } else {
            attribute.name.eq_ignore_ascii_case("is")
        }
    });
    if has_is {
        return true;
    }

    let name = element.name;
    !(VUE_RESERVED_HTML_ELEMENTS.contains(name)
        || VUE_RESERVED_DEPRECATED_HTML_ELEMENTS.contains(name)
        || VUE_RESERVED_SVG_ELEMENTS.contains(name)
        || VUE_RESERVED_KEBAB_CASE_ELEMENTS.contains(name)
        || MATHML_ELEMENTS.contains(&name))
}

/// MathML element names (eslint-plugin-vue checks these alongside HTML/SVG).
const MATHML_ELEMENTS: &[&str] = &[
    "annotation",
    "annotation-xml",
    "maction",
    "math",
    "menclose",
    "merror",
    "mfenced",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "semantics",
];

/// The span of the element's start tag, `<name ...attributes>`; mirrors
/// eslint-plugin-vue reporting on `element.startTag`. Attribute values may
/// contain `>`, so the scan starts after the last attribute.
fn start_tag_span(element: &Element<'_>, source_text: &str) -> Span {
    let scan_from =
        element.attributes.last().map_or(element.name_span.end, |attribute| attribute.span.end);
    let bytes = source_text.as_bytes();
    let mut index = scan_from as usize;
    while index < bytes.len() && bytes[index] != b'>' {
        index += 1;
    }
    let end = u32::try_from((index + 1).min(bytes.len())).unwrap_or(element.span.end);
    Span::new(element.span.start, end.min(element.span.end))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireVForKey;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><div v-for="item in items" :key="item.id">{{ item }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item in items" v-bind:key="item.id" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `<template v-for>`: the key may sit on the template tag itself…
            (
                r#"<template><template v-for="item in items" :key="item.id"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // …or on every child element.
            (
                r#"<template><template v-for="item in items"><div :key="item.id" /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom components are not this rule's concern.
            (
                r#"<template><MyRow v-for="item in items" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="item in items" :is="item.component" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No v-for, no requirement.
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r#"<template><div v-for="item in items">{{ item }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template v-for="item in items"><div /></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested inside other markup, with a script block in the file.
            (
                "<script setup>\nconst items = [];\n</script>\n<template>\n  <ul>\n    <li v-for=\"item in items\">{{ item }}</li>\n  </ul>\n</template>\n",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(RequireVForKey::NAME, RequireVForKey::PLUGIN, pass, fail).test_and_snapshot();
    }
}
