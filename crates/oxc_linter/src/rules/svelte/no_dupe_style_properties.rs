use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashMap;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, DirectiveKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_dupe_style_properties_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Duplicate property '{name}'."))
        .with_help("Remove the duplicate declaration; the later value overrides the earlier one.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDupeStyleProperties;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplicate style properties within an element's `style`
    /// attribute, including properties that a `style:` directive already
    /// sets.
    ///
    /// ### Why is this bad?
    ///
    /// When a property is declared twice, one declaration silently
    /// overrides the other — almost always a copy-paste mistake that makes
    /// the intended styling ambiguous.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div style="background: green; background: {color};">...</div>
    /// <div style:background="green" style="background: {color}">...</div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div style="background: green; background-color: {color};">...</div>
    /// <div style:background="green" style="background-color: {color}">...</div>
    /// ```
    NoDupeStyleProperties,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow duplicate style properties.",
);

/// A style declaration whose property name is statically known.
struct StyleDecl<'a> {
    property: &'a str,
    span: Span,
}

/// Collect the declarations of a `style` attribute value whose property
/// names appear in static text.
///
/// `{expr}` parts are masked out byte-for-byte, so a declaration like
/// `background: {color}` still yields its static property name while
/// expression contents can never introduce a phantom `;` or `:`.
/// Declarations are split on `;` outside quotes and parentheses, and the
/// property is the trimmed text before the first `:`.
#[expect(clippy::cast_possible_truncation)] // offsets into a source file, capped at `u32` by `Span`
fn collect_style_decls<'a>(
    value: &AttributeValue<'a>,
    source: &'a str,
    decls: &mut Vec<StyleDecl<'a>>,
) {
    let value_start = value.span.start as usize;
    let value_end = (value.span.end as usize).min(source.len());
    if value_start >= value_end {
        return;
    }
    let raw = &source[value_start..value_end];

    let mut masked = raw.as_bytes().to_vec();
    let mut expression_ranges: Vec<(usize, usize)> = Vec::new();
    for part in &value.parts {
        if let ValuePart::Expression(expression) = part {
            let range_start =
                (expression.span.start as usize).saturating_sub(value_start).min(masked.len());
            let range_end =
                (expression.span.end as usize).saturating_sub(value_start).min(masked.len());
            masked[range_start..range_end].fill(b'0');
            expression_ranges.push((range_start, range_end));
        }
    }

    // Split into `;`-separated declaration segments, ignoring `;` inside
    // quotes and parentheses (`content: "a;b"`, `background: url(a;b)`).
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut segment_start = 0;
    let mut paren_depth = 0u32;
    let mut quote: Option<u8> = None;
    for (index, &byte) in masked.iter().enumerate() {
        match quote {
            Some(open) => {
                if byte == open {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b';' if paren_depth == 0 => {
                    segments.push((segment_start, index));
                    segment_start = index + 1;
                }
                _ => {}
            },
        }
    }
    segments.push((segment_start, masked.len()));

    for (start, end) in segments {
        let Some(colon) = masked[start..end].iter().position(|&byte| byte == b':') else {
            continue;
        };
        let mut property_start = start;
        let property_limit = start + colon;
        while property_start < property_limit && masked[property_start].is_ascii_whitespace() {
            property_start += 1;
        }
        let mut property_end = property_limit;
        while property_end > property_start && masked[property_end - 1].is_ascii_whitespace() {
            property_end -= 1;
        }
        if property_start == property_end {
            continue;
        }
        // A property name overlapping an `{expr}` part is not statically
        // known — skip the declaration.
        if expression_ranges.iter().any(|&(range_start, range_end)| {
            range_start < property_end && range_end > property_start
        }) {
            continue;
        }
        decls.push(StyleDecl {
            property: &raw[property_start..property_end],
            span: Span::new(
                (value_start + property_start) as u32,
                (value_start + property_end) as u32,
            ),
        });
    }
}

impl Rule for NoDupeStyleProperties {}

// Deviations from upstream `svelte/no-dupe-style-properties`:
// - Upstream parses mustache expressions inside the style value (e.g. both
//   branches of `{cond ? 'background: a' : 'background: b'}`) with its own
//   style-template parser and compares those declarations too. This port
//   has no JS expression parser here, so expression parts are treated as
//   opaque: only declarations whose property name is static text are
//   compared.
// - CSS comments inside the style value are not stripped.
impl SvelteTemplateRule for NoDupeStyleProperties {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics: Vec<(&str, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            let mut decls: Vec<StyleDecl<'a>> = Vec::new();
            for attribute in &element.attributes {
                match &attribute.kind {
                    // `style:prop` sets `prop` just like a declaration.
                    AttributeKind::Directive(directive)
                        if directive.kind == DirectiveKind::Style =>
                    {
                        decls.push(StyleDecl {
                            property: directive.name,
                            span: directive.name_span,
                        });
                    }
                    AttributeKind::Plain { name: "style", value: Some(value), .. } => {
                        collect_style_decls(value, source, &mut decls);
                    }
                    _ => {}
                }
            }
            // Upstream reports both the earlier and the later occurrence,
            // each at most once (a third duplicate reports only itself).
            let mut seen: FxHashMap<&str, (Span, bool)> = FxHashMap::default();
            for decl in decls {
                if let Some(&(previous_span, previous_reported)) = seen.get(decl.property) {
                    if !previous_reported {
                        diagnostics.push((decl.property, previous_span));
                    }
                    diagnostics.push((decl.property, decl.span));
                    seen.insert(decl.property, (decl.span, true));
                } else {
                    seen.insert(decl.property, (decl.span, false));
                }
            }
        });
        for (name, span) in diagnostics {
            ctx.diagnostic(no_dupe_style_properties_diagnostic(name, span));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoDupeStyleProperties;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<div style=\"\">...</div>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<div style>...</div>", None, None, Some(PathBuf::from("test.svelte"))),
            (
                "<div style=\"background: green; background-color: {red};\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:background=\"green\" style=\"background-color: {red}\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Only `style` attributes participate.
            (
                "<div style:background=\"green\" not-style=\"background: {red}\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Upstream compares the declarations inside the ternary branches
            // against each other and finds no conflict; this port treats the
            // whole expression as opaque, with the same verdict.
            (
                "<div\n\tstyle=\"\n    background: blue;\n    {red ? `background-color: ${red}` : 'background-color: green'}\n  \"\n>\n\t...\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `;` inside quotes does not split declarations.
            (
                "<div style='content: \"a;color:red\"; color: blue'>...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // `;` inside parentheses does not split declarations.
            (
                "<div style=\"background: url(a;b); color: red\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Upstream compares property names exactly (case-sensitive).
            (
                "<div style=\"background: red; BACKGROUND: blue\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            (
                "<div style=\"background: green; background: {red};\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:background=\"green\" style=\"background: {red}\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Duplicate `style:` directives.
            (
                "<div style:width=\"10px\" style:width=\"20px\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A third duplicate reports only itself.
            (
                "<div style=\"color: red; color: blue; color: green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Opaque expression segments do not hide surrounding duplicates.
            (
                "<div style=\"background: green; {expr}; background: red\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style=\"background: url(a;b); background: red\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside blocks.
            (
                "{#if a}<div style=\"color: red; color: blue\">x</div>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoDupeStyleProperties::NAME, NoDupeStyleProperties::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
