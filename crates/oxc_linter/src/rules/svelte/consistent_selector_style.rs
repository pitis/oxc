use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Declaration, Expression, Statement};
use oxc_css_parser::ast::{SimpleSelector, TypeSelector};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{Parser, ParserReturn};
use oxc_span::{SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{
    AttributeKind, BlockKind, DirectiveKind, Element, ExpressionTag, Node, ValuePart,
};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{
        css_ident_name, expression_affixes, for_each_selector, parse_svelte_expression,
        svelte_scripts, svelte_style_blocks,
    },
};

fn consistent_selector_style_diagnostic(
    actual: Style,
    expected: Style,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Selector should select by {} instead of {}",
        expected.noun(),
        actual.noun()
    ))
    .with_help(format!("Rewrite the selector to match by {}.", expected.noun()))
    .with_label(span)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Style {
    Class,
    Id,
    Type,
}

impl Style {
    /// How the rule names this selector kind in its messages.
    fn noun(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Id => "ID",
            Self::Type => "element type",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct ConsistentSelectorStyleConfig {
    /// Also check selectors wrapped in `:global(…)`.
    check_global: bool,
    /// Selector kinds in order of preference. The first kind that can express
    /// the same selection wins; a selector written in a less preferred kind
    /// than one that would work is reported.
    style: Vec<Style>,
}

impl Default for ConsistentSelectorStyleConfig {
    fn default() -> Self {
        Self { check_global: false, style: vec![Style::Type, Style::Id, Style::Class] }
    }
}

// Boxed: the `style` list would blow `RuleEnum`'s 16-byte budget unboxed.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct ConsistentSelectorStyle(Box<ConsistentSelectorStyleConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a consistent way of writing CSS selectors: when a selector
    /// could equally well be written as an element type, an ID, or a class,
    /// this reports the ones that do not use the most preferred kind that
    /// would still select exactly the same elements.
    ///
    /// ### Why is this bad?
    ///
    /// Mixing selector styles for no reason makes a component's stylesheet
    /// harder to read, and a class or ID that adds nothing over the element
    /// type is markup that has to be kept in sync for no benefit.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <div class="wrapper"><p>Hello</p></div>
    ///
    /// <style>
    ///   /* only one element could ever match, so `div` says it */
    ///   .wrapper { color: red }
    /// </style>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <div class="wrapper"><p>Hello</p></div>
    ///
    /// <style>
    ///   div { color: red }
    /// </style>
    /// ```
    ///
    /// ### Options
    ///
    /// `style` lists the selector kinds in order of preference and defaults
    /// to `["type", "id", "class"]`. `checkGlobal` (default `false`) extends
    /// the check to selectors inside `:global(…)`.
    ///
    /// ```json
    /// {
    ///   "svelte/consistent-selector-style": [
    ///     "error",
    ///     { "style": ["type", "id", "class"], "checkGlobal": false }
    ///   ]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream resolves an identifier in `class={…}` / `id={…}` through
    /// ESLint's scope analysis. The markup pass has no scope information, so
    /// oxlint instead looks the name up among the top-level `const`/`let`/
    /// `var` declarations of the component's `<script>` blocks, and treats a
    /// name declared more than once as unresolvable. A binding introduced
    /// anywhere else — an `{#each}` context, a function parameter, an import
    /// — does not resolve, exactly as it does not resolve upstream.
    ConsistentSelectorStyle,
    svelte,
    style,
    config = ConsistentSelectorStyle,
    version = "1.80.0",
    short_description = "Enforce a consistent style for CSS selectors.",
);

impl Rule for ConsistentSelectorStyle {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for ConsistentSelectorStyle {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source = ctx.source_text();
        let blocks = svelte_style_blocks(nodes, source);
        if blocks.is_empty() {
            return;
        }

        // The markup expressions and the `<script>` bodies share one arena,
        // so an identifier can be resolved to an initialiser that outlives
        // the attribute it was read from.
        let allocator = Allocator::new();
        let programs: Vec<ParserReturn<'_>> = svelte_scripts(nodes, source)
            .iter()
            .map(|script| {
                let source_type =
                    if script.typescript { SourceType::ts() } else { SourceType::mjs() };
                Parser::new(&allocator, script.content, source_type).parse()
            })
            .collect();
        let initializers = top_level_initializers(&programs);
        let resolve = |name: &str| initializers.get(name).copied().flatten();

        let mut markup = Markup::default();
        markup.collect(nodes, Occurrence::One, &allocator, &resolve);

        let mut diagnostics = Vec::new();
        for block in &blocks {
            let Ok((stylesheet, _)) = block.parse(&allocator) else {
                // `svelte/valid-style-parse` reports a block that does not
                // parse; there is nothing to check here.
                continue;
            };
            for_each_selector(&stylesheet, &mut |selector, in_global| {
                if in_global && !self.0.check_global {
                    return;
                }
                let (actual, name, span) = match selector {
                    SimpleSelector::Class(class) => {
                        (Style::Class, css_ident_name(&class.name), class.span)
                    }
                    SimpleSelector::Id(id) => (Style::Id, css_ident_name(&id.name), id.span),
                    SimpleSelector::Type(TypeSelector::TagName(tag)) => {
                        (Style::Type, css_ident_name(&tag.name.name), tag.span)
                    }
                    // `*`, attribute, pseudo and nesting selectors have no
                    // equivalent in another style.
                    _ => return,
                };
                // An interpolated name (`.#{$x}` in SCSS) is not known here.
                let Some(name) = name else { return };
                if let Some(expected) = markup.expected_style(actual, name, &self.0.style) {
                    diagnostics.push(consistent_selector_style_diagnostic(
                        actual,
                        expected,
                        block.absolute(span),
                    ));
                }
            });
        }
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// How many times an element can appear in the rendered output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Occurrence {
    ZeroOrOne,
    One,
    ZeroToInf,
}

impl Occurrence {
    /// The multiplicity of an element nested `self` deep inside something
    /// that itself renders `other` times.
    fn multiply(self, other: Self) -> Self {
        match (self, other) {
            (Self::One, result) | (result, Self::One) => result,
            _ if self == other => self,
            _ => Self::ZeroToInf,
        }
    }
}

/// One HTML element of the markup, as a selector target.
#[derive(Debug)]
struct MarkupElement<'a> {
    name: &'a str,
    occurrence: Occurrence,
}

/// The elements a class or ID selector could match, indexed by name.
#[derive(Debug, Default)]
struct Selections<'a> {
    /// Statically written names → indices into [`Markup::elements`].
    exact: FxHashMap<&'a str, Vec<usize>>,
    /// `(prefix, suffix, element)` for a name built by an expression. This is
    /// a flat list rather than a map because upstream keys its map by a fresh
    /// array on every insert, so entries with equal affixes never merge.
    affixes: Vec<(Option<&'a str>, Option<&'a str>, usize)>,
    /// An expression with neither a known prefix nor a known suffix: it could
    /// produce any name, so no selector of this kind can be judged.
    universal: bool,
}

impl<'a> Selections<'a> {
    /// Record the affixes of an expression-valued `class` / `id` attribute.
    fn add_expression<'src, R>(
        &mut self,
        tag: &ExpressionTag<'src>,
        element: usize,
        allocator: &'a Allocator,
        resolve: &R,
    ) where
        'src: 'a,
        R: Fn(&str) -> Option<&'a Expression<'a>>,
    {
        let Some(parsed) = parse_svelte_expression(allocator, tag.expression) else {
            // An expression that does not parse could produce anything.
            self.universal = true;
            return;
        };
        let (prefix, suffix) = expression_affixes(allocator.alloc(parsed), resolve);
        if prefix.is_none() && suffix.is_none() {
            self.universal = true;
        } else {
            self.affixes.push((prefix, suffix, element));
        }
    }

    /// Every element a name of this kind could refer to, paired with whether
    /// the match was exact (as opposed to inferred from affixes).
    fn match_selection(&self, name: &str) -> Vec<(usize, bool)> {
        let mut selection: Vec<(usize, bool)> =
            self.exact.get(name).into_iter().flatten().map(|&element| (element, true)).collect();
        for &(prefix, suffix, element) in &self.affixes {
            if prefix.is_none_or(|prefix| name.starts_with(prefix))
                && suffix.is_none_or(|suffix| name.ends_with(suffix))
            {
                selection.push((element, false));
            }
        }
        selection
    }
}

/// Everything the markup says about what a stylesheet selector can match.
#[derive(Debug, Default)]
struct Markup<'a> {
    elements: Vec<MarkupElement<'a>>,
    /// Element names → indices into `elements`.
    types: FxHashMap<&'a str, Vec<usize>>,
    class: Selections<'a>,
    id: Selections<'a>,
    /// Classes toggled by a `class:` directive, which are never reported —
    /// the directive names the class, so it has to stay a class.
    directive_classes: FxHashSet<&'a str>,
}

impl<'a> Markup<'a> {
    fn collect<'src, R>(
        &mut self,
        nodes: &[Node<'src>],
        occurrence: Occurrence,
        allocator: &'a Allocator,
        resolve: &R,
    ) where
        'src: 'a,
        R: Fn(&str) -> Option<&'a Expression<'a>>,
    {
        for node in nodes {
            match node {
                Node::Element(element) => {
                    self.add_element(element, occurrence, allocator, resolve);
                    // A component renders its children any number of times.
                    let inside = if element.is_component_like() {
                        Occurrence::ZeroToInf
                    } else {
                        Occurrence::One
                    };
                    self.collect(
                        &element.children,
                        occurrence.multiply(inside),
                        allocator,
                        resolve,
                    );
                }
                Node::Block(block) => match &block.kind {
                    BlockKind::If(if_block) => {
                        let inside = occurrence.multiply(Occurrence::ZeroOrOne);
                        for branch in &if_block.branches {
                            self.collect(&branch.children, inside, allocator, resolve);
                        }
                    }
                    BlockKind::Each(each) => {
                        let inside = occurrence.multiply(Occurrence::ZeroToInf);
                        self.collect(&each.children, inside, allocator, resolve);
                        if let Some(fallback) = &each.fallback {
                            // Upstream reaches the `{:else}` body through the
                            // each block, so it inherits the each's count.
                            self.collect(
                                fallback,
                                inside.multiply(Occurrence::ZeroOrOne),
                                allocator,
                                resolve,
                            );
                        }
                    }
                    BlockKind::Await(await_block) => {
                        let inside = occurrence.multiply(Occurrence::ZeroOrOne);
                        self.collect(&await_block.pending, inside, allocator, resolve);
                        for children in [&await_block.then_children, &await_block.catch_children]
                            .into_iter()
                            .flatten()
                        {
                            self.collect(children, inside, allocator, resolve);
                        }
                    }
                    BlockKind::Key(key) => {
                        self.collect(&key.children, occurrence, allocator, resolve);
                    }
                    BlockKind::Snippet(snippet) => {
                        // A snippet renders wherever it is used, any number
                        // of times.
                        self.collect(
                            &snippet.children,
                            occurrence.multiply(Occurrence::ZeroToInf),
                            allocator,
                            resolve,
                        );
                    }
                    BlockKind::Unknown(unknown) => {
                        self.collect(&unknown.children, occurrence, allocator, resolve);
                    }
                },
                _ => {}
            }
        }
    }

    fn add_element<'src, R>(
        &mut self,
        element: &Element<'src>,
        occurrence: Occurrence,
        allocator: &'a Allocator,
        resolve: &R,
    ) where
        'src: 'a,
        R: Fn(&str) -> Option<&'a Expression<'a>>,
    {
        // Only plain HTML elements are selector targets. Components and
        // `<svelte:*>` are a different node kind upstream, and `<script>` /
        // `<style>` are separate node types there, so none of them reach the
        // rule's element visitor.
        if element.is_component_like()
            || element.svelte_name().is_some()
            || element.name.eq_ignore_ascii_case("script")
            || element.name.eq_ignore_ascii_case("style")
        {
            return;
        }

        let index = self.elements.len();
        self.elements.push(MarkupElement { name: element.name, occurrence });
        self.types.entry(element.name).or_default().push(index);

        for attribute in &element.attributes {
            match &attribute.kind {
                AttributeKind::Directive(directive) if directive.kind == DirectiveKind::Class => {
                    self.directive_classes.insert(directive.name);
                    self.class.exact.entry(directive.name).or_default().push(index);
                }
                AttributeKind::Plain { name: "class", value: Some(value), .. } => {
                    for part in &value.parts {
                        match part {
                            ValuePart::Text(text) => {
                                for class in text.value.split_whitespace() {
                                    self.class.exact.entry(class).or_default().push(index);
                                }
                            }
                            ValuePart::Expression(tag) => {
                                self.class.add_expression(tag, index, allocator, resolve);
                            }
                        }
                    }
                }
                AttributeKind::Plain { name: "id", value: Some(value), .. } => {
                    for part in &value.parts {
                        match part {
                            // An ID is one name, not a whitespace-separated
                            // list, so the literal text is the key.
                            ValuePart::Text(text) => {
                                self.id.exact.entry(text.value).or_default().push(index);
                            }
                            ValuePart::Expression(tag) => {
                                self.id.add_expression(tag, index, allocator, resolve);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The selector kind `name` should have been written as, or `None` when
    /// the one it was written as is already the most preferred that works.
    fn expected_style(&self, actual: Style, name: &str, style: &[Style]) -> Option<Style> {
        let selection = match actual {
            Style::Class => {
                if self.class.universal || self.directive_classes.contains(name) {
                    return None;
                }
                self.class.match_selection(name)
            }
            Style::Id => {
                if self.id.universal {
                    return None;
                }
                self.id.match_selection(name)
            }
            Style::Type => {
                self.types.get(name).into_iter().flatten().map(|&element| (element, true)).collect()
            }
        };

        for &preferred in style {
            if preferred == actual {
                return None;
            }
            let usable = match preferred {
                // A class can always stand in for anything else.
                Style::Class => true,
                Style::Id => self.can_use_id_selector(&selection),
                Style::Type => self.can_use_type_selector(&selection),
            };
            if usable {
                return Some(preferred);
            }
        }
        None
    }

    /// An ID has to be unique, so it can only stand in for a selection of at
    /// most one element that renders at most once.
    fn can_use_id_selector(&self, selection: &[(usize, bool)]) -> bool {
        match selection {
            [] => true,
            [(element, _)] => self.elements[*element].occurrence != Occurrence::ZeroToInf,
            _ => false,
        }
    }

    /// A type selector can stand in only when the selection is exactly every
    /// element of one type.
    fn can_use_type_selector(&self, selection: &[(usize, bool)]) -> bool {
        let names: FxHashSet<&str> =
            selection.iter().map(|&(element, _)| self.elements[element].name).collect();
        if names.len() > 1 {
            return false;
        }
        let Some(name) = names.into_iter().next() else {
            // Nothing is selected, so any kind would do equally.
            return true;
        };
        // An affix match inside a loop stands for an unknown number of
        // elements, which no type selector can reproduce.
        if selection.iter().any(|&(element, exact)| {
            !exact && self.elements[element].occurrence == Occurrence::ZeroToInf
        }) {
            return false;
        }
        let Some(of_type) = self.types.get(name) else { return false };
        of_type.len() == selection.len()
            && of_type.iter().all(|element| selection.iter().any(|&(other, _)| other == *element))
    }
}

/// The top-level `const` / `let` / `var` initialisers of the component's
/// scripts, keyed by name. A name declared more than once maps to `None`:
/// upstream's scope lookup also gives up when a variable has more than one
/// declaration site.
fn top_level_initializers<'a>(
    programs: &'a [ParserReturn<'a>],
) -> FxHashMap<&'a str, Option<&'a Expression<'a>>> {
    let mut initializers: FxHashMap<&'a str, Option<&'a Expression<'a>>> = FxHashMap::default();
    for program in programs {
        for statement in &program.program.body {
            let declaration = match statement {
                Statement::VariableDeclaration(declaration) => declaration,
                // `export let x` — how a Svelte 4 component declares a prop.
                Statement::ExportDeclaration(export) => {
                    let Declaration::VariableDeclaration(declaration) = &export.declaration else {
                        continue;
                    };
                    declaration
                }
                _ => continue,
            };
            for declarator in &declaration.declarations {
                let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
                    continue;
                };
                let entry = initializers.entry(identifier.name.as_str());
                entry
                    .and_modify(|existing| *existing = None)
                    .or_insert_with(|| declarator.init.as_ref());
            }
        }
    }
    initializers
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ConsistentSelectorStyle;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let class_first = || Some(serde_json::json!([{ "style": ["class", "id", "type"] }]));
        let check_global = || Some(serde_json::json!([{ "checkGlobal": true }]));
        let pass = vec![
            // The default preference is `type` first, and `div` is it.
            ("<div>x</div>\n<style>\n\tdiv {}\n</style>", None, None, path()),
            // Inside `{#each}` the element repeats, so no ID could do it.
            (
                "{#each xs as x}<div class=\"a\"></div>{/each}\n<div></div>\n<style>\n\t.a {}\n</style>",
                None,
                None,
                path(),
            ),
            // `:global(…)` is left alone by default.
            ("<div class=\"a\"></div>\n<style>\n\t:global(.a) {}\n</style>", None, None, path()),
            // A `class:` directive names the class, so it must stay one.
            ("<div class:a></div>\n<style>\n\t.a {}\n</style>", None, None, path()),
            // A fully dynamic class could be anything.
            ("<div class={cls}></div>\n<style>\n\t.a {}\n</style>", None, None, path()),
            // A class assembled in a loop stands for an unknown number of
            // elements, so neither a type nor an ID selector could do it.
            (
                "{#each xs as x}<div class={`pre-${x}`}></div>{/each}\n<style>\n\t.pre-a {}\n</style>",
                None,
                None,
                path(),
            ),
            // With `class` preferred first, a class selector is correct.
            ("<div class=\"a\"></div>\n<style>\n\t.a {}\n</style>", class_first(), None, path()),
            // No `<style>` block at all.
            ("<div class=\"a\"></div>", None, None, path()),
            // A selector matching nothing in the markup is left alone when
            // the preferred kind is the one already used.
            ("<div></div>\n<style>\n\tspan {}\n</style>", None, None, path()),
        ];
        let fail = vec![
            // One div carries the class and it is the only div: `div` works.
            ("<div class=\"a\"></div>\n<style>\n\t.a {}\n</style>", None, None, path()),
            // Same for an ID.
            ("<div id=\"a\"></div>\n<style>\n\t#a {}\n</style>", None, None, path()),
            // Two divs but only one carries the class, so a type selector
            // would over-select — an ID would not, and is preferred.
            (
                "<div class=\"a\"></div>\n<div></div>\n<style>\n\t.a {}\n</style>",
                None,
                None,
                path(),
            ),
            // Two divs both carrying the class: still exactly the divs.
            (
                "<div class=\"a\"></div>\n<div class=\"a\"></div>\n<style>\n\t.a {}\n</style>",
                None,
                None,
                path(),
            ),
            // `class` preferred: a type selector should have been a class.
            ("<div></div>\n<style>\n\tdiv {}\n</style>", class_first(), None, path()),
            // `checkGlobal` extends the check into `:global(…)`.
            (
                "<div class=\"a\"></div>\n<style>\n\t:global(.a) {}\n</style>",
                check_global(),
                None,
                path(),
            ),
            // A class built with a known prefix does not select `.other`,
            // so the selection is empty and any kind would do.
            (
                "<div class=\"pre-a\"></div>\n<span class={`pre-${x}`}></span>\n<style>\n\t.other {}\n</style>",
                None,
                None,
                path(),
            ),
            // Inside `@media` too.
            (
                "<div class=\"a\"></div>\n<style>\n\t@media print { .a {} }\n</style>",
                None,
                None,
                path(),
            ),
        ];

        Tester::new(ConsistentSelectorStyle::NAME, ConsistentSelectorStyle::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
