//! Printing an element's attributes.
//!
//! Attribute *values* are never reflowed: Prettier does not format HTML
//! attribute text, and neither does this. What is decided here is only the
//! shorthand form and where the printer may break between attributes.

use oxc_formatter_core::{
    Buffer, FormatElement, TailwindCollector,
    builders::{text, token},
    write,
};
use svelte_markup_parser::ast::{
    Attribute, AttributeKind, AttributeValue, DirectiveKind, ValuePart,
};

use super::{
    SvelteFormatter,
    expression::{ExpressionPosition, write_expression, write_expression_tag},
};

/// Write one attribute, as written in the source apart from the shorthand
/// decision.
pub fn write_attribute<'a>(
    attribute: &Attribute<'a>,
    source: &'a str,
    allow_shorthand: bool,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    match &attribute.kind {
        AttributeKind::Plain { name, value, .. } => {
            let Some(value) = value else {
                // A bare boolean attribute.
                write!(f, text(name));
                return;
            };
            if allow_shorthand && is_shorthandable(name, value) {
                write!(f, [token("{"), text(name), token("}")]);
                return;
            }
            write!(f, [text(name), token("=")]);
            write_value(value, *name == "class", f);
        }
        AttributeKind::Shorthand { name, .. } => {
            // `{@attach expr}` is an attachment, not a shorthand: the parser
            // reports every non-spread `{…}` attribute the same way, and the
            // `@` sigil is what tells them apart.
            if let Some((keyword, expression)) = attachment(name) {
                write!(f, [token("{@"), text(keyword), token(" ")]);
                write_expression(expression, ExpressionPosition::Braces, f);
                write!(f, token("}"));
                return;
            }
            if allow_shorthand {
                write!(f, [token("{"), text(name), token("}")]);
            } else {
                write!(f, [text(name), token("={"), text(name), token("}")]);
            }
        }
        AttributeKind::Spread { expression, .. } => {
            write!(f, token("{..."));
            write_expression(expression, ExpressionPosition::Braces, f);
            write!(f, token("}"));
        }
        AttributeKind::Directive(directive) => {
            write!(f, text(directive.raw_name));
            let Some(value) = &directive.value else { return };
            // `bind:`, `class:`, `style:` and `let:` drop a value that just
            // names the same thing the directive does.
            if allow_shorthand
                && matches!(
                    directive.kind,
                    DirectiveKind::Bind
                        | DirectiveKind::Class
                        | DirectiveKind::Style
                        | DirectiveKind::Let
                )
                && value
                    .as_single_expression()
                    .is_some_and(|tag| tag.expression.trim() == directive.name)
            {
                return;
            }
            write!(f, token("="));
            // `class:foo={…}` names one class; it is not a class *list*, and
            // `prettier-plugin-tailwindcss` does not sort it either.
            write_value(value, false, f);
        }
        AttributeKind::Comment { .. } => {
            // A comment between attributes is the author's prose; it keeps
            // its exact spelling.
            write!(f, text(source_of(source, attribute)));
        }
    }
}

/// Split a `{@attach expr}` attribute into its keyword and its expression.
fn attachment(text: &str) -> Option<(&'static str, &str)> {
    let rest = text.strip_prefix("@attach")?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let expression = rest.trim_start();
    (!expression.is_empty()).then_some(("attach", expression))
}

/// Whether `name={name}` can be written `{name}`.
fn is_shorthandable(name: &str, value: &AttributeValue<'_>) -> bool {
    value.as_single_expression().is_some_and(|tag| tag.expression.trim() == name)
}

/// Write the value after `=`, keeping its quoting as written unless it has
/// none to keep.
///
/// `class_list` marks the value as one whose text is a list of Tailwind
/// classes to be sorted.
fn write_value<'a>(value: &AttributeValue<'a>, class_list: bool, f: &mut SvelteFormatter<'_, 'a>) {
    // A value that is exactly one `{…}` needs no quotes; anything else is
    // quoted, since removing them could change where the value ends.
    if let Some(tag) = value.as_single_expression() {
        write_expression_tag(tag, ExpressionPosition::Braces, f);
        return;
    }
    // `"` unless the value contains one, in which case `'`. Prettier writes
    // `"` either way, which turns `prop='"'` into `prop=""" ` — markup that
    // no longer parses. Divergence recorded in `keeps_a_value_that_is_quoted`.
    let quote = if contains_double_quote(value) { token("'") } else { token("\"") };
    write!(f, quote);
    for part in &value.parts {
        match part {
            ValuePart::Text(part) => {
                if !class_list || !write_tailwind_classes(part.value, f) {
                    write!(f, text(part.value));
                }
            }
            ValuePart::Expression(tag) => {
                write_expression_tag(tag, ExpressionPosition::QuotedAttribute, f);
            }
        }
    }
    write!(f, quote);
}

/// Register a `class` attribute's text as one sortable list, and write the
/// placeholder the host's sorter fills in. Returns whether it did.
///
/// Only a value that is *all* text: `prettier-plugin-tailwindcss` sorts each
/// text run of a mixed `class="a {x} b"` with its boundary words held in
/// place, and the sorter this talks to takes a whole list with no way to say
/// which end of it is half a class name.
fn write_tailwind_classes<'a>(classes: &'a str, f: &mut SvelteFormatter<'_, 'a>) -> bool {
    if !f.options().sort_tailwind_classes || classes.trim().is_empty() {
        return false;
    }
    let index = f.context_mut().add_class(classes.to_string());
    f.write_element(FormatElement::TailwindClass(index));
    true
}

/// Whether the value's literal text carries a `"`, which the surrounding
/// quotes then cannot be.
///
/// Only for a value that is all text. An expression inside a quoted value is
/// formatted with single quotes preferred — that is what keeps it from ending
/// a double-quoted value early — so a single-quoted wrapper would be the one
/// broken instead. A value carrying both a literal `"` and an expression has
/// no spelling this printer can produce, and keeps Prettier's.
fn contains_double_quote(value: &AttributeValue<'_>) -> bool {
    value.parts.iter().all(|part| matches!(part, ValuePart::Text(_)))
        && value.parts.iter().any(|part| match part {
            ValuePart::Text(part) => part.value.contains('"'),
            ValuePart::Expression(_) => false,
        })
}

fn source_of<'a>(source: &'a str, attribute: &Attribute<'_>) -> &'a str {
    slice(source, attribute.span.start, attribute.span.end)
}

fn slice(source: &str, start: u32, end: u32) -> &str {
    &source[start as usize..end as usize]
}
