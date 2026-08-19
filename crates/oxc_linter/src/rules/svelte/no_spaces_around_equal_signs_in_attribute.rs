use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_spaces_around_equal_signs_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected spaces found around equal signs.")
        .with_help("Remove the spaces so the attribute reads `name=value`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoSpacesAroundEqualSignsInAttribute;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows whitespace around the equal sign between an attribute name
    /// and its value.
    ///
    /// ### Why is this bad?
    ///
    /// HTML5 discourages spaces around `=`, and attributes written as
    /// `name=value` are easier to read and consistent with how attributes
    /// are conventionally formatted.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class = "foo" />
    /// <div class ="foo" />
    /// <div class= "foo" />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class="foo" />
    /// ```
    NoSpacesAroundEqualSignsInAttribute,
    svelte,
    style,
    version = "1.80.0",
    short_description = "Disallow spaces around equal signs in attributes.",
);

impl Rule for NoSpacesAroundEqualSignsInAttribute {}

// Upstream `svelte/no-spaces-around-equal-signs-in-attribute` takes the
// source between the attribute key and its value and reports when the
// `[\s=]*` run contains whitespace.
//
// This port scans the raw source after each written attribute name: the
// markup parser only pairs a value with a name when `=` immediately follows
// it, so a spaced form like `attr = "v"` parses as a bare attribute followed
// by stray tokens. Scanning the `[\s=]*` run in the source reconstructs
// upstream's check; requiring an `=` in the run reproduces upstream's
// bounding by the attribute node's end (a bare boolean attribute never
// reaches the following attribute's text). Deviation: the parser tokenizes
// with ASCII whitespace, so the scan does too (upstream matches Unicode
// whitespace). No autofix is provided.
impl SvelteTemplateRule for NoSpacesAroundEqualSignsInAttribute {
    #[expect(clippy::cast_possible_truncation)] // offsets into a source file, capped at `u32` by `Span`
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let bytes = ctx.source_text().as_bytes();
        let mut spans = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                let key_end = match &attribute.kind {
                    AttributeKind::Plain { name_span, .. } => name_span.end,
                    AttributeKind::Directive(directive) => {
                        // `name_span` covers only the part after the prefix;
                        // the written key ends after the full raw name
                        // (prefix, name, and modifiers).
                        attribute.span.start + directive.raw_name.len() as u32
                    }
                    // Shorthand and spread attributes have no `=`.
                    AttributeKind::Shorthand { .. } | AttributeKind::Spread { .. } => continue,
                };
                let mut position = key_end as usize;
                let mut has_whitespace = false;
                let mut has_equal_sign = false;
                while let Some(&byte) = bytes.get(position) {
                    if byte.is_ascii_whitespace() {
                        has_whitespace = true;
                    } else if byte == b'=' {
                        has_equal_sign = true;
                    } else {
                        break;
                    }
                    position += 1;
                }
                if has_whitespace && has_equal_sign {
                    spans.push(Span::new(key_end, position as u32));
                }
            }
        });
        for span in spans {
            ctx.diagnostic(no_spaces_around_equal_signs_diagnostic(span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoSpacesAroundEqualSignsInAttribute;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                "<p class=\"test\" style=\"\" bind:test={value} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<p class=\"test2\" style style:width=\"10px\" />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            ("<p class=p this={expression} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<div {shorthand} {...spread} />", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<p on:click|preventDefault={handler} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "{#if a}<p on:click={handler}></p>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            ("<p class = \"h\"></p>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<p class =\"e\"></p>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<p class=   \"l\"></p>", None, None, Some(PathBuf::from("test.svelte"))),
            // Line breaks count as whitespace too.
            ("<p class=\n       \"l\"></p>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<p class\n       = \"o\"></p>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<p class= \"=\"></p>", None, None, Some(PathBuf::from("test.svelte"))),
            // Unquoted values.
            ("<p class= a></p>", None, None, Some(PathBuf::from("test.svelte"))),
            // Directives, including a second spaced one on the same tag.
            (
                "<p bind:test= {value} bind:test2  = {value} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            ("<p style:width =\"10px\" />", None, None, Some(PathBuf::from("test.svelte"))),
            // Directive with modifiers.
            ("<p on:click|once = {handler} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<p this = {expression} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested inside blocks.
            (
                "{#each items as item}<div a =\t\"b\"></div>{/each}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(
            NoSpacesAroundEqualSignsInAttribute::NAME,
            NoSpacesAroundEqualSignsInAttribute::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
