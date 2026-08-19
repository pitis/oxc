use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, walk_svelte_elements},
};

fn prefer_class_directive_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected class using the ternary operator.")
        .with_help("Use a `class:name={condition}` directive instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Prefer {
    /// Report every convertible ternary.
    Always,
    /// Report only a ternary that has an empty branch.
    #[default]
    Empty,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PreferClassDirective(Prefer);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers a `class:` directive over a ternary inside a `class`
    /// attribute.
    ///
    /// ### Why is this bad?
    ///
    /// `class:selected={isSelected}` says what it means and lets Svelte
    /// toggle the single class, instead of recomputing the whole attribute.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class="{isSelected ? 'selected' : ''}"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class:selected={isSelected}></div>
    /// ```
    ///
    /// ### Options
    ///
    /// `prefer` is `"empty"` by default, reporting only a ternary with an
    /// empty branch; `"always"` reports every convertible ternary, including
    /// `cond ? 'a' : 'b'`.
    ///
    /// ```json
    /// { "svelte/prefer-class-directive": ["error", { "prefer": "always" }] }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream folds constant expressions when reading a branch's class
    /// name. oxlint reads only string literals and template literals with no
    /// substitutions, so a branch written as, say, `'a' + 'b'` is treated as
    /// unknown and the ternary is left alone.
    PreferClassDirective,
    svelte,
    style,
    config = PreferClassDirective,
    version = "1.80.0",
    short_description = "Prefer a `class:` directive over a ternary in `class`.",
);

impl Rule for PreferClassDirective {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let prefer = match value
            .get(0)
            .and_then(|options| options.get("prefer"))
            .and_then(serde_json::Value::as_str)
        {
            Some("always") => Prefer::Always,
            _ => Prefer::Empty,
        };
        Ok(Self(prefer))
    }
}

impl SvelteTemplateRule for PreferClassDirective {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let allocator = Allocator::new();
        let mut spans = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // Upstream only rewrites plain HTML elements; `class` on a
            // component is an ordinary prop.
            if element.is_component_like() || element.svelte_name().is_some() {
                return;
            }
            for attribute in &element.attributes {
                let AttributeKind::Plain { name: "class", value: Some(value), .. } =
                    &attribute.kind
                else {
                    continue;
                };
                for (index, part) in value.parts.iter().enumerate() {
                    let ValuePart::Expression(expression) = part else { continue };
                    if self.is_convertible(&value.parts, index, expression.expression, &allocator) {
                        spans.push(expression.span);
                    }
                }
            }
        });
        for span in spans {
            ctx.diagnostic(prefer_class_directive_diagnostic(span));
        }
    }
}

impl PreferClassDirective {
    fn is_convertible(
        &self,
        parts: &[ValuePart<'_>],
        index: usize,
        text: &str,
        allocator: &Allocator,
    ) -> bool {
        let Some(expression) = parse_svelte_expression(allocator, text) else { return false };
        let Some(class_names) = conditional_class_names(&expression) else { return false };
        // More than two outcomes is too tangled to rewrite.
        if class_names.len() > 2 {
            return false;
        }
        // The default only reports a ternary that can drop a class entirely.
        if self.0 == Prefer::Empty && class_names.iter().all(|name| !name.trim().is_empty()) {
            return false;
        }

        // The rewritten directive appends its class to the attribute, so it
        // must not be glued to a neighbouring class name.
        let previous_is_word = !ends_with_non_word(parts, index);
        let next_is_word = !starts_with_non_word(parts, index);
        class_names.iter().all(|name| {
            if name.is_empty() {
                // Removing the part entirely would join the neighbours.
                return !(previous_is_word && next_is_word);
            }
            if !name.trim().chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                return false;
            }
            let starts_tight = name.starts_with(|c: char| !c.is_whitespace());
            let ends_tight = name.ends_with(|c: char| !c.is_whitespace());
            !(starts_tight && previous_is_word || ends_tight && next_is_word)
        })
    }
}

/// Every constant string a conditional expression can evaluate to, or `None`
/// when a branch is not a constant string.
fn conditional_class_names(expression: &Expression<'_>) -> Option<Vec<String>> {
    let Expression::ConditionalExpression(conditional) = expression.get_inner_expression() else {
        return None;
    };
    let mut names = Vec::new();
    for branch in [&conditional.consequent, &conditional.alternate] {
        match branch.get_inner_expression() {
            Expression::ConditionalExpression(_) => names.extend(conditional_class_names(branch)?),
            other => names.push(constant_string(other)?),
        }
    }
    Some(names)
}

/// A string literal, or a template literal with no substitutions.
fn constant_string(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::StringLiteral(literal) => Some(literal.value.to_string()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            template.quasis.first().map(|quasi| quasi.value.raw.to_string())
        }
        _ => None,
    }
}

/// Whether the value continues, after `index`, with something that is not a
/// word character — so a class appended after it would not be glued on.
fn starts_with_non_word(parts: &[ValuePart<'_>], index: usize) -> bool {
    for part in parts.iter().skip(index + 1) {
        match part_strings(part) {
            None => return false,
            Some(strings) => {
                for string in strings {
                    if !string.is_empty() {
                        return string.starts_with(char::is_whitespace);
                    }
                }
            }
        }
    }
    // Nothing follows: the end of the attribute is a boundary.
    true
}

/// The mirror of [`starts_with_non_word`], scanning backwards.
fn ends_with_non_word(parts: &[ValuePart<'_>], index: usize) -> bool {
    for part in parts.iter().take(index).rev() {
        match part_strings(part) {
            None => return false,
            Some(strings) => {
                for string in strings {
                    if !string.is_empty() {
                        return string.ends_with(char::is_whitespace);
                    }
                }
            }
        }
    }
    true
}

/// The constant strings a value part can contribute, or `None` when unknown.
fn part_strings(part: &ValuePart<'_>) -> Option<Vec<String>> {
    match part {
        ValuePart::Text(text) => Some(vec![text.value.to_string()]),
        // An expression neighbour is only known when it is itself a
        // constant-string ternary; anything else blocks the rewrite.
        ValuePart::Expression(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::PreferClassDirective;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let always = || Some(serde_json::json!([{ "prefer": "always" }]));
        let pass = vec![
            ("<div class:selected={isSelected}></div>", None, None, path()),
            ("<div class=\"static\"></div>", None, None, path()),
            // Both branches non-empty: only reported with `prefer: always`.
            ("<div class=\"{a ? 'x' : 'y'}\"></div>", None, None, path()),
            // Not a ternary.
            ("<div class=\"{cls}\"></div>", None, None, path()),
            // A branch that is not a plain class name.
            ("<div class=\"{a ? 'x y' : ''}\"></div>", None, None, path()),
            // Glued to a neighbouring class name.
            ("<div class=\"prefix{a ? 'x' : ''}\"></div>", None, None, path()),
            // `class` on a component is an ordinary prop.
            ("<Widget class=\"{a ? 'x' : ''}\" />", None, None, path()),
            // More than two outcomes.
            ("<div class=\"{a ? 'x' : b ? 'y' : c ? 'z' : ''}\"></div>", None, None, path()),
        ];
        let fail = vec![
            ("<div class=\"{isSelected ? 'selected' : ''}\"></div>", None, None, path()),
            ("<div class=\"{a ? '' : 'x'}\"></div>", None, None, path()),
            // Separated from its neighbours by spaces.
            ("<div class=\"a {isSelected ? 'selected' : ''} b\"></div>", None, None, path()),
            ("<div class=\"{a ? 'x' : 'y'}\"></div>", always(), None, path()),
        ];

        Tester::new(PreferClassDirective::NAME, PreferClassDirective::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
