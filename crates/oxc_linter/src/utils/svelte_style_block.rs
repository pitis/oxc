//! Parsing the `<style>` blocks of a `.svelte` file.
//!
//! Uses the same CSS parser the formatter does (`oxc-css-parser`), so the
//! `svelte/*` CSS rules see a real stylesheet AST — selectors included —
//! rather than a hand-rolled scanner.
//!
//! Spans coming out of the CSS parser are relative to the block's own text,
//! so everything here returns them already shifted to file-absolute offsets.

use oxc_allocator::Allocator;
use oxc_css_parser::{
    Parser, Syntax,
    ast::{
        ComplexSelectorChild, InterpolableIdent, PseudoClassSelector, PseudoClassSelectorArgKind,
        SelectorList, SimpleSelector, Statement, Stylesheet,
    },
    error::Error,
};
use oxc_span::Span;
use svelte_markup_parser::ast::{AttributeValue, Element, Node};

use crate::utils::{get_plain_attribute, svelte_start_tag_span, walk_svelte_elements};

/// One `<style>` block of a component, located and classified.
#[derive(Debug, Clone, Copy)]
pub struct SvelteStyleBlock<'a> {
    /// The block body, exactly the element's raw-text span.
    pub content: &'a str,
    /// File offset of `content`'s first byte. Add to any span the CSS parser
    /// produces to get a file-absolute span.
    pub offset: u32,
    /// The `lang` attribute as written, if any.
    pub lang: Option<&'a str>,
    /// The dialect `lang` selects, or `None` when it names a preprocessor
    /// this parser cannot read (`stylus`, `postcss-*`, …).
    pub syntax: Option<Syntax>,
    /// The `<style …>` opening tag, for reporting against the block itself.
    pub tag_span: Span,
}

impl<'a> SvelteStyleBlock<'a> {
    /// Shift a CSS-parser span into file coordinates.
    pub fn absolute(&self, span: oxc_css_parser::Span) -> Span {
        let start = u32::try_from(span.start).unwrap_or(u32::MAX).saturating_add(self.offset);
        let end = u32::try_from(span.end).unwrap_or(u32::MAX).saturating_add(self.offset);
        Span::new(start, end)
    }

    /// Parse the block, returning the stylesheet plus any recoverable errors.
    ///
    /// Returns `Err` with the first fatal parse error. `Ok` may still carry
    /// recoverable errors — the CSS spec recovers from those, but a linter
    /// wants to see them.
    pub fn parse<'alloc>(
        &self,
        allocator: &'alloc Allocator,
    ) -> Result<(Stylesheet<'alloc>, Vec<Error>), Error>
    where
        'a: 'alloc,
    {
        let syntax = self.syntax.unwrap_or_default();
        // `Parser::new` strips a leading BOM from its own input, which would
        // desync every span; a `<style>` interior can never start with one.
        debug_assert!(!self.content.starts_with('\u{feff}'));
        let mut parser = Parser::new(allocator, self.content, syntax);
        let stylesheet = parser.parse::<Stylesheet>()?;
        let recoverable = parser.recoverable_errors().to_vec();
        Ok((stylesheet, recoverable))
    }
}

/// The `<style>` blocks of a parsed `.svelte` file, in source order.
pub fn svelte_style_blocks<'a>(
    nodes: &[Node<'a>],
    source_text: &'a str,
) -> Vec<SvelteStyleBlock<'a>> {
    let mut blocks = Vec::new();
    walk_svelte_elements(nodes, &mut |element| {
        if !element.name.eq_ignore_ascii_case("style") {
            return;
        }
        let Some(raw) = element.raw_text else { return };
        let lang = style_lang(element);
        blocks.push(SvelteStyleBlock {
            content: &source_text[raw.start as usize..raw.end as usize],
            offset: raw.start,
            lang,
            syntax: lang.map_or(Some(Syntax::Css), syntax_for_lang),
            tag_span: svelte_start_tag_span(element),
        });
    });
    blocks
}

/// The block's `lang` attribute when it is a plain literal.
fn style_lang<'a>(element: &Element<'a>) -> Option<&'a str> {
    get_plain_attribute(element, "lang")
        .and_then(|(_, value)| value.and_then(AttributeValue::as_static_text))
}

/// Visit every simple selector in a stylesheet, in source order.
///
/// Descends into at-rule blocks and into the selector-list arguments of
/// functional pseudo-classes, so `:global(.a)`, `:is(.a, .b)`, `:where(.a)`
/// and `:has(> .a)` all yield their inner selectors. `in_global` tracks
/// whether the selector sits inside a `:global(…)` argument or nested
/// inside a bare `:global { … }` block.
pub fn for_each_selector<'a>(
    stylesheet: &Stylesheet<'a>,
    visit: &mut impl FnMut(&SimpleSelector<'a>, bool),
) {
    visit_statements(&stylesheet.statements, false, visit);
}

fn visit_statements<'a>(
    statements: &[Statement<'a>],
    in_global: bool,
    visit: &mut impl FnMut(&SimpleSelector<'a>, bool),
) {
    for statement in statements {
        match statement {
            Statement::QualifiedRule(rule) => {
                visit_selector_list(&rule.selector, in_global, visit);
                // Everything nested inside a bare `:global { … }` is global
                // too. `:global(.a) { … }` is not: only its argument is.
                let nested_global = in_global || is_bare_global(&rule.selector);
                visit_statements(&rule.block.statements, nested_global, visit);
            }
            Statement::AtRule(rule) => {
                if let Some(block) = &rule.block {
                    visit_statements(&block.statements, in_global, visit);
                }
            }
            _ => {}
        }
    }
}

fn visit_selector_list<'a>(
    list: &SelectorList<'a>,
    in_global: bool,
    visit: &mut impl FnMut(&SimpleSelector<'a>, bool),
) {
    for complex in &list.selectors {
        for child in &complex.children {
            let ComplexSelectorChild::CompoundSelector(compound) = child else { continue };
            for simple in &compound.children {
                visit(simple, in_global);
                // `:global(…)` / `:is(…)` / `:where(…)` / `:has(…)` carry
                // nested selector lists.
                if let SimpleSelector::PseudoClass(pseudo) = simple
                    && let Some(arg) = &pseudo.arg
                {
                    let nested_global = in_global || pseudo_name_is(pseudo, "global");
                    match &arg.kind {
                        PseudoClassSelectorArgKind::SelectorList(list) => {
                            visit_selector_list(list, nested_global, visit);
                        }
                        PseudoClassSelectorArgKind::CompoundSelectorList(list) => {
                            for compound in &list.selectors {
                                for simple in &compound.children {
                                    visit(simple, nested_global);
                                }
                            }
                        }
                        PseudoClassSelectorArgKind::RelativeSelectorList(list) => {
                            for relative in &list.selectors {
                                for child in &relative.complex_selector.children {
                                    let ComplexSelectorChild::CompoundSelector(compound) = child
                                    else {
                                        continue;
                                    };
                                    for simple in &compound.children {
                                        visit(simple, nested_global);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Whether a rule's selector is exactly `:global`, the block form that makes
/// everything inside it global — as opposed to the `:global(…)` argument
/// form, which scopes only what it wraps.
fn is_bare_global(list: &SelectorList<'_>) -> bool {
    let [complex] = list.selectors.as_slice() else { return false };
    let [ComplexSelectorChild::CompoundSelector(compound)] = complex.children.as_slice() else {
        return false;
    };
    let [SimpleSelector::PseudoClass(pseudo)] = compound.children.as_slice() else { return false };
    pseudo.arg.is_none() && pseudo_name_is(pseudo, "global")
}

fn pseudo_name_is(pseudo: &PseudoClassSelector<'_>, name: &str) -> bool {
    matches!(&pseudo.name, InterpolableIdent::Literal(ident) if ident.name.eq_ignore_ascii_case(name))
}

/// The literal text of an interpolable identifier, when it is not
/// interpolated.
pub fn css_ident_name<'a>(ident: &InterpolableIdent<'a>) -> Option<&'a str> {
    match ident {
        InterpolableIdent::Literal(literal) => Some(literal.name),
        _ => None,
    }
}

/// The parser dialect a `lang` value selects, or `None` when the language is
/// one this parser does not read.
pub fn syntax_for_lang(lang: &str) -> Option<Syntax> {
    if lang.eq_ignore_ascii_case("css") || lang.eq_ignore_ascii_case("postcss") {
        Some(Syntax::Css)
    } else if lang.eq_ignore_ascii_case("scss") {
        Some(Syntax::Scss)
    } else if lang.eq_ignore_ascii_case("sass") {
        Some(Syntax::Sass)
    } else if lang.eq_ignore_ascii_case("less") {
        Some(Syntax::Less)
    } else {
        None
    }
}
