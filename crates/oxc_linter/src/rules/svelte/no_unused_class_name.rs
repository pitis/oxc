use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, Node, ValuePart};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{deserialize_to_regexp_group_vec, svelte_start_tag_span, walk_svelte_elements},
};

fn no_unused_class_name_diagnostic(class_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Unused class \"{class_name}\"."))
        .with_help("Add a rule for it to the component's `<style>`, or remove the class.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct NoUnusedClassNameConfig {
    /// Class names that are allowed even with no matching style rule, as
    /// names or `/regex/` patterns.
    #[serde(deserialize_with = "deserialize_to_regexp_group_vec")]
    #[schemars(with = "Vec<String>")]
    allowed_class_names: Vec<lazy_regex::Regex>,
}

// Boxed: the pattern list would blow `RuleEnum`'s 16-byte budget unboxed.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct NoUnusedClassName(Box<NoUnusedClassNameConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports a class used in the markup that no rule in the component's
    /// `<style>` block styles.
    ///
    /// ### Why is this bad?
    ///
    /// A class with no matching rule is usually a typo or a leftover from a
    /// style that was deleted — and because Svelte scopes styles per
    /// component, it will not be picked up from anywhere else either.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class="card"></div>
    ///
    /// <style>
    ///   .box { color: red }
    /// </style>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class="card"></div>
    ///
    /// <style>
    ///   .card { color: red }
    /// </style>
    /// ```
    ///
    /// ### Options
    ///
    /// `allowedClassNames`: class names (or `/regex/` patterns) that are
    /// never reported — for classes a global stylesheet or a library owns.
    ///
    /// ```json
    /// {
    ///   "svelte/no-unused-class-name": ["error", { "allowedClassNames": ["/^js-/"] }]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Class names are read out of the `<style>` block with a selector
    /// scanner rather than a PostCSS parse: every `.name` token that appears
    /// in selector position counts as styled. The practical difference is
    /// that a `.name` written inside an unusual at-rule prelude also counts,
    /// which can only ever suppress a report.
    NoUnusedClassName,
    svelte,
    correctness,
    config = NoUnusedClassName,
    version = "1.80.0",
    short_description = "Disallow classes with no matching style rule.",
);

impl Rule for NoUnusedClassName {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for NoUnusedClassName {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();

        // Classes any `<style>` block styles.
        let mut styled: FxHashSet<&str> = FxHashSet::default();
        walk_svelte_elements(nodes, &mut |element| {
            if !element.name.eq_ignore_ascii_case("style") {
                return;
            }
            let Some(raw) = element.raw_text else { return };
            collect_selector_classes(&source[raw.start as usize..raw.end as usize], &mut styled);
        });

        // Classes the markup uses, in source order.
        let mut used: Vec<(&str, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            let span = svelte_start_tag_span(element);
            for attribute in &element.attributes {
                match &attribute.kind {
                    AttributeKind::Plain { name: "class", value: Some(value), .. } => {
                        // Only the literal parts: an interpolated class name
                        // is not statically known.
                        for part in &value.parts {
                            if let ValuePart::Text(text) = part {
                                used.extend(
                                    text.value.split_whitespace().map(|class| (class, span)),
                                );
                            }
                        }
                    }
                    AttributeKind::Directive(directive)
                        if directive.kind == DirectiveKind::Class =>
                    {
                        used.push((directive.name, span));
                    }
                    _ => {}
                }
            }
        });

        let mut reports = Vec::new();
        for (class, span) in used {
            if styled.contains(class)
                || self.0.allowed_class_names.iter().any(|pattern| pattern.is_match(class))
            {
                continue;
            }
            reports.push((class, span));
        }
        for (class, span) in reports {
            ctx.diagnostic(no_unused_class_name_diagnostic(class, span));
        }
    }
}

/// Collect every `.name` written in selector position of a stylesheet.
///
/// Scans the text outside strings and comments, tracking brace depth so only
/// selectors — the text before a `{` — are considered, and skipping
/// `[attr=".x"]` contents.
fn collect_selector_classes<'a>(style: &'a str, out: &mut FxHashSet<&'a str>) {
    let bytes = style.as_bytes();
    let mut index = 0;
    // Text since the last `{`, `}` or `;`, i.e. the pending selector.
    let mut selector_start = 0;
    let mut quote: Option<u8> = None;
    let mut bracket_depth = 0u32;

    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let mut end = index + 2;
            while end + 1 < bytes.len() && !(bytes[end] == b'*' && bytes[end + 1] == b'/') {
                end += 1;
            }
            index = (end + 2).min(bytes.len());
            continue;
        }
        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => {
                extract_classes(&style[selector_start..index], out);
                selector_start = index + 1;
            }
            b'}' | b';' => selector_start = index + 1,
            _ => {}
        }
        index += 1;
    }
    let _ = bracket_depth;
}

/// Pull `.name` class tokens out of one selector.
fn extract_classes<'a>(selector: &'a str, out: &mut FxHashSet<&'a str>) {
    let bytes = selector.as_bytes();
    let mut index = 0;
    let mut bracket_depth = 0u32;
    while index < bytes.len() {
        match bytes[index] {
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            // A `.` inside `[attr=".x"]` is part of a value, not a class.
            b'.' if bracket_depth == 0 => {
                let start = index + 1;
                let mut end = start;
                while end < bytes.len() && is_class_char(bytes[end]) {
                    end += 1;
                }
                if end > start {
                    out.insert(&selector[start..end]);
                }
                index = end;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
}

fn is_class_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte >= 0x80 || byte == b'\\'
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUnusedClassName;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            (
                "<div class=\"card\"></div>\n<style>\n\t.card { color: red }\n</style>",
                None,
                None,
                path(),
            ),
            // `class:` directive.
            (
                "<div class:active></div>\n<style>\n\t.active { color: red }\n</style>",
                None,
                None,
                path(),
            ),
            // Compound and descendant selectors.
            (
                "<div class=\"a b\"></div>\n<style>\n\tdiv.a > .b { color: red }\n</style>",
                None,
                None,
                path(),
            ),
            // Interpolated class names are not statically known.
            ("<div class={cls}></div>\n<style>\n\t.x {}\n</style>", None, None, path()),
            // No class at all.
            ("<div></div>", None, None, path()),
            // Allow-list.
            (
                "<div class=\"js-hook\"></div>\n<style>\n\t.x {}\n</style>",
                Some(serde_json::json!([{ "allowedClassNames": ["/^js-/"] }])),
                None,
                path(),
            ),
            // A `.` inside an attribute selector is not a class.
            (
                "<div class=\"a\"></div>\n<style>\n\t.a { color: red }\n\t[data-x=\".b\"] {}\n</style>",
                None,
                None,
                path(),
            ),
        ];
        let fail = vec![
            (
                "<div class=\"card\"></div>\n<style>\n\t.box { color: red }\n</style>",
                None,
                None,
                path(),
            ),
            ("<div class=\"card\"></div>", None, None, path()),
            ("<div class:active></div>\n<style>\n\t.inactive {}\n</style>", None, None, path()),
            // Only the unstyled one of the two is reported.
            (
                "<div class=\"a b\"></div>\n<style>\n\t.a { color: red }\n</style>",
                None,
                None,
                path(),
            ),
        ];

        Tester::new(NoUnusedClassName::NAME, NoUnusedClassName::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
