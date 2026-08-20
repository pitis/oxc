//! Printing an element's attributes.
//!
//! Attribute *values* are never reflowed: Prettier does not format HTML
//! attribute text, and neither does this. What is decided here is only the
//! shorthand form and where the printer may break between attributes.

use oxc_formatter_core::{
    Buffer,
    builders::{text, token},
    write,
};
use svelte_markup_parser::ast::{Attribute, AttributeKind, AttributeValue, ValuePart};

use super::SvelteFormatter;

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
            write_value(value, source, f);
        }
        AttributeKind::Shorthand { name, .. } => {
            if allow_shorthand {
                write!(f, [token("{"), text(name), token("}")]);
            } else {
                write!(f, [text(name), token("={"), text(name), token("}")]);
            }
        }
        AttributeKind::Spread { expression, .. } => {
            write!(f, [token("{..."), text(expression.trim()), token("}")]);
        }
        AttributeKind::Comment { .. } | AttributeKind::Directive(_) => {
            // Directives and attribute comments keep their exact spelling:
            // the modifier list, the shorthand form and the comment body are
            // all the author's, and none of them is reflowed.
            write!(f, text(source_of(source, attribute)));
        }
    }
}

/// Whether `name={name}` can be written `{name}`.
fn is_shorthandable(name: &str, value: &AttributeValue<'_>) -> bool {
    value.as_single_expression().is_some_and(|tag| tag.expression.trim() == name)
}

/// Write the value after `=`, keeping its quoting as written unless it has
/// none to keep.
fn write_value<'a>(value: &AttributeValue<'a>, source: &'a str, f: &mut SvelteFormatter<'_, 'a>) {
    // A value that is exactly one `{…}` needs no quotes; anything else is
    // quoted, since removing them could change where the value ends.
    if value.as_single_expression().is_some() && value.quote == 0 {
        write!(f, text(slice(source, value.span.start, value.span.end)));
        return;
    }
    write!(f, token("\""));
    for part in &value.parts {
        match part {
            ValuePart::Text(part) => write!(f, text(part.value)),
            ValuePart::Expression(tag) => {
                write!(f, text(slice(source, tag.span.start, tag.span.end)));
            }
        }
    }
    write!(f, token("\""));
}

fn source_of<'a>(source: &'a str, attribute: &Attribute<'_>) -> &'a str {
    slice(source, attribute.span.start, attribute.span.end)
}

fn slice(source: &str, start: u32, end: u32) -> &str {
    &source[start as usize..end as usize]
}
