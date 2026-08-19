use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashSet;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, DirectiveKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_shorthand_style_property_overrides_diagnostic(
    shorthand: &str,
    original: &str,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Unexpected shorthand '{shorthand}' after '{original}'."))
        .with_help(
            "The shorthand resets every longhand it covers, silently discarding the earlier declaration; declare the shorthand first or remove one of the two.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoShorthandStylePropertyOverrides;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows a shorthand style property (like `background`) declared
    /// after one of its longhand properties (like `background-color`)
    /// within one element's `style` attribute and `style:` directives.
    ///
    /// ### Why is this bad?
    ///
    /// A shorthand sets *all* of its longhands, resetting the ones it does
    /// not mention to their initial values. Declared after a related
    /// longhand, it silently wipes that earlier declaration out — almost
    /// always an ordering mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div style="background-repeat: repeat; background: green">...</div>
    /// <div style:background-repeat="repeat" style:background="green">...</div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div style="background: green; background-repeat: repeat">...</div>
    /// <div style:background-repeat="repeat" style:background-color="green">...</div>
    /// ```
    NoShorthandStylePropertyOverrides,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow shorthand style properties that override related longhands.",
);

/// CSS shorthand properties and the longhands they reset, ported from
/// eslint-plugin-svelte's `src/utils/css-utils/resource.ts`
/// (`SHORTHAND_PROPERTIES`), re-sorted by key for binary search.
const SHORTHAND_PROPERTIES: [(&str, &[&str]); 35] = [
    (
        "animation",
        &[
            "animation-name",
            "animation-duration",
            "animation-timing-function",
            "animation-delay",
            "animation-iteration-count",
            "animation-direction",
            "animation-fill-mode",
            "animation-play-state",
        ],
    ),
    (
        "background",
        &[
            "background-image",
            "background-size",
            "background-position",
            "background-repeat",
            "background-origin",
            "background-clip",
            "background-attachment",
            "background-color",
        ],
    ),
    (
        "border",
        &[
            "border-top-width",
            "border-bottom-width",
            "border-left-width",
            "border-right-width",
            "border-top-style",
            "border-bottom-style",
            "border-left-style",
            "border-right-style",
            "border-top-color",
            "border-bottom-color",
            "border-left-color",
            "border-right-color",
        ],
    ),
    (
        "border-block-end",
        &["border-block-end-width", "border-block-end-style", "border-block-end-color"],
    ),
    (
        "border-block-start",
        &["border-block-start-width", "border-block-start-style", "border-block-start-color"],
    ),
    ("border-bottom", &["border-bottom-width", "border-bottom-style", "border-bottom-color"]),
    (
        "border-color",
        &["border-top-color", "border-bottom-color", "border-left-color", "border-right-color"],
    ),
    (
        "border-image",
        &[
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
    ),
    (
        "border-inline-end",
        &["border-inline-end-width", "border-inline-end-style", "border-inline-end-color"],
    ),
    (
        "border-inline-start",
        &["border-inline-start-width", "border-inline-start-style", "border-inline-start-color"],
    ),
    ("border-left", &["border-left-width", "border-left-style", "border-left-color"]),
    (
        "border-radius",
        &[
            "border-top-right-radius",
            "border-top-left-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ],
    ),
    ("border-right", &["border-right-width", "border-right-style", "border-right-color"]),
    (
        "border-style",
        &["border-top-style", "border-bottom-style", "border-left-style", "border-right-style"],
    ),
    ("border-top", &["border-top-width", "border-top-style", "border-top-color"]),
    (
        "border-width",
        &["border-top-width", "border-bottom-width", "border-left-width", "border-right-width"],
    ),
    ("column-rule", &["column-rule-width", "column-rule-style", "column-rule-color"]),
    ("columns", &["column-width", "column-count"]),
    ("flex", &["flex-grow", "flex-shrink", "flex-basis"]),
    ("flex-flow", &["flex-direction", "flex-wrap"]),
    (
        "font",
        &[
            "font-style",
            "font-variant",
            "font-weight",
            "font-stretch",
            "font-size",
            "font-family",
            "line-height",
        ],
    ),
    (
        "grid",
        &[
            "grid-template-rows",
            "grid-template-columns",
            "grid-template-areas",
            "grid-auto-rows",
            "grid-auto-columns",
            "grid-auto-flow",
            "grid-column-gap",
            "grid-row-gap",
        ],
    ),
    ("grid-area", &["grid-row-start", "grid-column-start", "grid-row-end", "grid-column-end"]),
    ("grid-column", &["grid-column-start", "grid-column-end"]),
    ("grid-gap", &["grid-row-gap", "grid-column-gap"]),
    ("grid-row", &["grid-row-start", "grid-row-end"]),
    ("grid-template", &["grid-template-columns", "grid-template-rows", "grid-template-areas"]),
    ("list-style", &["list-style-type", "list-style-position", "list-style-image"]),
    ("margin", &["margin-top", "margin-bottom", "margin-left", "margin-right"]),
    (
        "mask",
        &[
            "mask-image",
            "mask-mode",
            "mask-position",
            "mask-size",
            "mask-repeat",
            "mask-origin",
            "mask-clip",
            "mask-composite",
        ],
    ),
    ("outline", &["outline-color", "outline-style", "outline-width"]),
    ("padding", &["padding-top", "padding-bottom", "padding-left", "padding-right"]),
    (
        "text-decoration",
        &["text-decoration-color", "text-decoration-style", "text-decoration-line"],
    ),
    ("text-emphasis", &["text-emphasis-style", "text-emphasis-color"]),
    (
        "transition",
        &[
            "transition-delay",
            "transition-duration",
            "transition-property",
            "transition-timing-function",
        ],
    ),
];

/// The longhands the given (vendor-prefix-stripped) property resets, when it
/// is a known shorthand.
fn shorthand_longhands(property: &str) -> Option<&'static [&'static str]> {
    SHORTHAND_PROPERTIES
        .binary_search_by_key(&property, |(shorthand, _)| shorthand)
        .ok()
        .map(|index| SHORTHAND_PROPERTIES[index].1)
}

/// The `-vendor-` prefix of a property name, mirroring upstream's
/// `/^-\w+-/` (`getVendorPrefix`); empty when there is none.
fn vendor_prefix(property: &str) -> &str {
    let Some(rest) = property.strip_prefix('-') else {
        return "";
    };
    let word_len =
        rest.bytes().take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_').count();
    if word_len > 0 && rest.as_bytes().get(word_len) == Some(&b'-') {
        &property[..word_len + 2]
    } else {
        ""
    }
}

/// A style declaration whose property name is statically known.
struct StyleDecl<'a> {
    property: &'a str,
    span: Span,
}

/// Collect the declarations of a `style` attribute value whose property
/// names appear in static text. Same approach as
/// `svelte/no-dupe-style-properties` (kept local per this fork's convention
/// of duplicating small helpers per rule file):
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

impl Rule for NoShorthandStylePropertyOverrides {}

// Ports eslint-plugin-svelte's `no-shorthand-style-property-overrides`.
//
// Deviations (shared with this fork's `svelte/no-dupe-style-properties`):
// - Upstream parses mustache expressions inside the style value (e.g. both
//   branches of `{cond ? 'background: a' : 'background: b'}`) with its own
//   style-template parser and checks those declarations too. This port has
//   no JS expression parser here, so expression parts are treated as opaque:
//   only declarations whose property name is static text participate.
// - CSS comments inside the style value are not stripped.
impl SvelteTemplateRule for NoShorthandStylePropertyOverrides {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let mut diagnostics: Vec<(&str, String, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Declarations in source order across `style:` directives and
            // `style` attributes, exactly as upstream iterates them.
            let mut decls: Vec<StyleDecl<'a>> = Vec::new();
            for attribute in &element.attributes {
                match &attribute.kind {
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

            let mut before: FxHashSet<&str> = FxHashSet::default();
            for decl in decls {
                let prefix = vendor_prefix(decl.property);
                let normalized = &decl.property[prefix.len()..];
                if let Some(longhands) = shorthand_longhands(normalized) {
                    for longhand in longhands {
                        let with_prefix = if prefix.is_empty() {
                            (*longhand).to_string()
                        } else {
                            format!("{prefix}{longhand}")
                        };
                        if before.contains(with_prefix.as_str()) {
                            diagnostics.push((decl.property, with_prefix, decl.span));
                        }
                    }
                }
                before.insert(decl.property);
            }
        });
        for (shorthand, original, span) in diagnostics {
            ctx.diagnostic(no_shorthand_style_property_overrides_diagnostic(
                shorthand, &original, span,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoShorthandStylePropertyOverrides;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<div style=\"\">...</div>", None, None, Some(PathBuf::from("test.svelte"))),
            ("<div style>...</div>", None, None, Some(PathBuf::from("test.svelte"))),
            // A shorthand alone, or after unrelated properties, is fine.
            (
                "<div style:background={red}>...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style=\"background: green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Longhand after longhand.
            (
                "<div style:background-repeat=\"repeat\" style:background-color=\"green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div\n\tstyle=\"\n    background-repeat: repeat;\n    background-color: green;\n  \"\n>\n\t...\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:background-repeat=\"repeat\" style=\"background-color: green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Longhand AFTER its shorthand is this rule's mirror image and fine.
            (
                "<div style=\"background: green; background-color: red\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Only `style` attributes participate.
            (
                "<div style:background-repeat=\"repeat\" not-style=\"background: green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:background-repeat=\"repeat\" not-style=\"background: {red}\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Same-named longhands on different elements don't interact.
            (
                "<div style:background-repeat=\"repeat\">...</div><div style=\"background: green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Vendor-prefixed shorthand does not override the unprefixed longhand.
            (
                "<div style=\"mask-image: url(a.png); -webkit-mask: none\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Upstream's ternary01-valid: the branch declarations are
            // longhands, no conflict; this port treats the expression as
            // opaque, with the same verdict.
            (
                "<div\n\tstyle=\"\n    background-repeat: repeat;\n    {red ? `background-color: ${red}` : 'background-color: green'}\n  \"\n>\n\t...\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // DEVIATION from upstream (ternary01-invalid): upstream parses
            // the string literals inside the mustache and reports the
            // `background` shorthands in both branches; this port treats
            // expression parts as opaque and cannot see them.
            (
                "<div\n\tstyle=\"\n    background-repeat: repeat;\n    {red ? `background: ${red}` : 'background: green'}\n  \"\n>\n\t...\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            // style: directive pairs.
            (
                "<div style:background-repeat=\"repeat\" style:background=\"green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:background-repeat=\"repeat\" style:background={red}>...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Within one style attribute.
            (
                "<div\n\tstyle=\"\n    background-repeat: repeat;\n    background: green;\n  \"\n>\n\t...\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div\n\tstyle=\"\n    background-repeat: repeat;\n    background: {red};\n  \"\n>\n\t...\n</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Directive then style attribute.
            (
                "<div style:background-repeat=\"repeat\" style=\"background: green\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:background-repeat=\"repeat\" style=\"background: {red}\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Other shorthand families.
            (
                "<div style=\"border-top-color: red; border-top: 1px solid\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            (
                "<div style:font-size=\"12px\" style=\"font: caption\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Vendor prefixes carry over to the longhand comparison.
            (
                "<div style=\"-webkit-mask-image: url(a.png); -webkit-mask: none\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // A shorthand overriding two longhands reports once per longhand.
            (
                "<div style=\"margin-top: 1px; margin-left: 2px; margin: 0\">...</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Nested inside blocks.
            (
                "{#if a}<div style=\"margin-top: 1px; margin: 0\">x</div>{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(
            NoShorthandStylePropertyOverrides::NAME,
            NoShorthandStylePropertyOverrides::PLUGIN,
            pass,
            fail,
        )
        .test_and_snapshot();
    }
}
