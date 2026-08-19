use lazy_regex::regex::Regex;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn sort_attributes_diagnostic(current: &str, previous: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Attribute `{current}` should go before `{previous}`."))
        .with_help("Reorder the attributes to match the configured order.")
        .with_label(span)
}

/// eslint-plugin-svelte's default group order, as `(patterns, alphabetical)`.
const DEFAULT_ORDER: &[(&[&str], bool)] = &[
    (&["this"], false),
    (&["bind:this"], false),
    (&["id"], false),
    (&["name"], false),
    (&["slot"], false),
    // `--style-props`
    (&["/^--/u"], true),
    // `style` attribute and `style:` directives
    (&["style", "/^style:/u"], false),
    (&["class"], false),
    (&["/^class:/u"], true),
    // Everything else that is not a directive or a `--style-prop`.
    (&["!/:/u", "!/^(?:this|id|name|style|class)$/u", "!/^--/u"], true),
    // `bind:` (other than `bind:this`) and `on:`
    (&["/^bind:/u", "!bind:this", "/^on:/u"], false),
    (&["/^use:/u"], true),
    (&["/^transition:/u"], true),
    (&["/^in:/u"], true),
    (&["/^out:/u"], true),
    (&["/^animate:/u"], true),
    (&["/^let:/u"], true),
];

/// One pattern of a group: an exact name or a `/regex/`, optionally negated
/// with a leading `!`.
#[derive(Debug, Clone)]
struct Matcher {
    negated: bool,
    kind: MatcherKind,
}

#[derive(Debug, Clone)]
enum MatcherKind {
    Exact(String),
    Pattern(Regex),
}

impl Matcher {
    fn parse(pattern: &str) -> Option<Self> {
        let (negated, rest) = match pattern.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        let kind = match rest.strip_prefix('/').and_then(|rest| {
            // Trailing `u` flag, as written in upstream's defaults.
            rest.strip_suffix("/u").or_else(|| rest.strip_suffix('/'))
        }) {
            Some(source) => MatcherKind::Pattern(Regex::new(source).ok()?),
            None => MatcherKind::Exact(rest.to_string()),
        };
        Some(Self { negated, kind })
    }

    fn matches(&self, key: &str) -> bool {
        match &self.kind {
            MatcherKind::Exact(name) => name == key,
            MatcherKind::Pattern(pattern) => pattern.is_match(key),
        }
    }
}

/// One group in the order: a sequence of include/exclude patterns, and
/// whether the group is sorted alphabetically within itself.
#[derive(Debug, Clone)]
struct Group {
    matchers: Vec<Matcher>,
    alphabetical: bool,
}

impl Group {
    fn new(patterns: &[impl AsRef<str>], alphabetical: bool) -> Self {
        Self {
            matchers: patterns.iter().filter_map(|p| Matcher::parse(p.as_ref())).collect(),
            alphabetical,
        }
    }

    /// Upstream's `compileMatcher`: the patterns are applied in order, each
    /// positive one including and each negative one excluding, starting from
    /// "excluded" unless the first pattern is itself a negation.
    fn matches(&self, key: &str) -> bool {
        let Some(first) = self.matchers.first() else { return false };
        let mut result = first.negated;
        for matcher in &self.matchers {
            if result != matcher.negated {
                continue;
            }
            if matcher.matches(key) {
                result = !matcher.negated;
            }
        }
        result
    }
}

/// One entry of the user-facing `order` option.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum OrderEntry {
    /// `"id"` or `"/^bind:/u"`.
    One(String),
    /// `["style", "/^style:/u"]`.
    Many(Vec<String>),
    /// `{ "match": …, "sort": "alphabetical" | "ignore" }`.
    Matched {
        #[serde(rename = "match")]
        match_: OrderMatch,
        sort: SortKind,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum OrderMatch {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SortKind {
    Alphabetical,
    Ignore,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct SortAttributesConfig {
    /// The group order. Defaults to eslint-plugin-svelte's own ordering.
    order: Option<Vec<OrderEntry>>,
}

// Boxed: the compiled order would blow `RuleEnum`'s 16-byte budget unboxed.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct SortAttributes(Box<SortAttributesConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a consistent order for an element's attributes and
    /// directives.
    ///
    /// ### Why is this bad?
    ///
    /// Nothing breaks; a fixed order makes long start tags easier to scan and
    /// keeps diffs small.
    ///
    /// The default order is `this`, `bind:this`, `id`, `name`, `slot`,
    /// `--style-props`, `style` and `style:`, `class`, `class:`, other
    /// attributes, `bind:` and `on:`, `use:`, `transition:`, `in:`, `out:`,
    /// `animate:`, `let:` — alphabetical within most groups.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class="foo" id="bar"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div id="bar" class="foo"></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `order` replaces the default grouping. Each entry is an attribute
    /// name, a `/regex/` (optionally negated with `!`), an array of either,
    /// or `{ "match": …, "sort": "alphabetical" | "ignore" }`.
    ///
    /// ```json
    /// {
    ///   "svelte/sort-attributes": ["error", { "order": ["id", "class", "/^on:/u"] }]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// A spread (`{...props}`) resets the comparison, like upstream: only
    /// attributes on the same side of it are compared with each other.
    SortAttributes,
    svelte,
    style,
    config = SortAttributes,
    version = "1.80.0",
    short_description = "Enforce a consistent order for attributes and directives.",
);

impl Rule for SortAttributes {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let config: SortAttributesConfig = match value.get(0) {
            Some(options) => serde_json::from_value(options.clone())?,
            None => SortAttributesConfig::default(),
        };
        Ok(Self(Box::new(config)))
    }
}

impl SvelteTemplateRule for SortAttributes {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let groups = self.groups();
        let mut reports: Vec<(&str, &str, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Attributes seen since the last spread, with their group index.
            let mut previous: Vec<(&'a str, usize)> = Vec::new();
            for attribute in &element.attributes {
                if matches!(attribute.kind, AttributeKind::Spread { .. }) {
                    // Upstream stops comparing across a spread: reordering
                    // over it would change which value wins.
                    previous.clear();
                    continue;
                }
                let Some(key) = attribute.name() else { continue };
                let Some(index) = groups.iter().position(|group| group.matches(key)) else {
                    // Not covered by the order: upstream ignores it.
                    continue;
                };
                let invalid = previous.iter().find(|&&(previous_key, previous_index)| {
                    match previous_index.cmp(&index) {
                        std::cmp::Ordering::Greater => true,
                        std::cmp::Ordering::Equal => {
                            groups[index].alphabetical && previous_key > key
                        }
                        std::cmp::Ordering::Less => false,
                    }
                });
                if let Some(&(previous_key, _)) = invalid {
                    reports.push((key, previous_key, attribute.span));
                } else {
                    previous.push((key, index));
                }
            }
        });
        for (current, previous, span) in reports {
            ctx.diagnostic(sort_attributes_diagnostic(current, previous, span));
        }
    }
}

impl SortAttributes {
    fn groups(&self) -> Vec<Group> {
        match &self.0.order {
            None => DEFAULT_ORDER
                .iter()
                .map(|(patterns, alphabetical)| Group::new(patterns, *alphabetical))
                .collect(),
            Some(order) => order
                .iter()
                .map(|entry| match entry {
                    OrderEntry::One(pattern) => Group::new(std::slice::from_ref(pattern), false),
                    OrderEntry::Many(patterns) => Group::new(patterns, false),
                    OrderEntry::Matched { match_, sort } => {
                        let alphabetical = *sort == SortKind::Alphabetical;
                        match match_ {
                            OrderMatch::One(pattern) => {
                                Group::new(std::slice::from_ref(pattern), alphabetical)
                            }
                            OrderMatch::Many(patterns) => Group::new(patterns, alphabetical),
                        }
                    }
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SortAttributes;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let pass = vec![
            ("<div id=\"bar\" class=\"foo\"></div>", None, None, path()),
            (
                "<div bind:this={el} id=\"a\" style=\"color: red\" class=\"c\"></div>",
                None,
                None,
                path(),
            ),
            // Alphabetical within the "other attributes" group.
            ("<div aria-label=\"a\" data-x=\"b\" role=\"c\"></div>", None, None, path()),
            // `on:` comes after ordinary attributes.
            ("<button type=\"button\" on:click={f}></button>", None, None, path()),
            // A spread resets the comparison.
            ("<div class=\"c\" {...rest} id=\"a\"></div>", None, None, path()),
            // Custom order.
            (
                "<div class=\"c\" id=\"a\"></div>",
                Some(serde_json::json!([{ "order": ["class", "id"] }])),
                None,
                path(),
            ),
        ];
        let fail = vec![
            ("<div class=\"foo\" id=\"bar\"></div>", None, None, path()),
            ("<div on:click={f} id=\"a\"></div>", None, None, path()),
            // Out of alphabetical order within a group.
            ("<div role=\"c\" data-x=\"b\"></div>", None, None, path()),
            (
                "<div id=\"a\" class=\"c\"></div>",
                Some(serde_json::json!([{ "order": ["class", "id"] }])),
                None,
                path(),
            ),
        ];

        Tester::new(SortAttributes::NAME, SortAttributes::PLUGIN, pass, fail).test_and_snapshot();
    }
}
