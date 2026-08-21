//! Printing an element's attributes.
//!
//! Attribute *values* are never reflowed: Prettier does not format HTML
//! attribute text, and neither does this. What is decided here is only the
//! shorthand form and where the printer may break between attributes.

use std::borrow::Cow;

use oxc_formatter_core::{
    Buffer, FormatElement,
    builders::{expand_parent, text, token},
    write,
};
use svelte_markup_parser::ast::{
    Attribute, AttributeKind, AttributeValue, DirectiveKind, ValuePart,
};

use super::{
    SvelteFormatter,
    expression::{ExpressionPosition, write_expression, write_expression_tag},
};

/// What the element around an attribute tells its printing.
#[derive(Debug, Clone, Copy)]
pub struct AttributeContext {
    /// Whether `foo={foo}` may be written `{foo}`.
    pub allow_shorthand: bool,
    /// Whether the element is a plain HTML tag, which is the only place the
    /// `class` attribute's whitespace is tidied.
    pub regular_element: bool,
    /// Whether a `{…}` in a value is literal text rather than an expression.
    ///
    /// Svelte does not interpolate in a `<script>` or `<style>` tag's
    /// attributes, so `generics="Item extends { label?: string }"` is a type
    /// and not an object, and laying it out is laying out prose. Only the
    /// quoting is still normalized, which is what Prettier does here too.
    pub literal_values: bool,
}

/// Write one attribute, as written in the source apart from the shorthand
/// decision.
pub fn write_attribute<'a>(
    attribute: &Attribute<'a>,
    source: &'a str,
    context: AttributeContext,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    let allow_shorthand = context.allow_shorthand;
    match &attribute.kind {
        AttributeKind::Plain { name, value, .. } => {
            let Some(value) = value else {
                // A bare boolean attribute.
                write!(f, text(name));
                return;
            };
            if context.literal_values {
                write!(f, [text(name), token("=")]);
                write_literal_value(value, source, f);
                return;
            }
            if allow_shorthand && is_shorthandable(name, value) {
                write!(f, [token("{"), text(name), token("}")]);
                return;
            }
            write!(f, [text(name), token("=")]);
            write_value(value, ClassAttribute::of(name, context), f);
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
            let position = match directive.kind {
                DirectiveKind::Bind => ExpressionPosition::BindDirective,
                _ => ExpressionPosition::Braces,
            };
            write_value_at(value, ClassAttribute::NONE, position, f);
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

/// Whether a value's text is a list of CSS classes, and in which of the two
/// senses that matters.
#[derive(Debug, Clone, Copy)]
struct ClassAttribute {
    /// A `class` attribute anywhere: its text is a Tailwind class list.
    sortable: bool,
    /// A `class` attribute on a plain HTML element, whose whitespace Prettier
    /// tidies — the one attribute value it reflows at all.
    tidy_whitespace: bool,
}

impl ClassAttribute {
    const NONE: Self = Self { sortable: false, tidy_whitespace: false };

    fn of(name: &str, context: AttributeContext) -> Self {
        let sortable = name == "class";
        Self { sortable, tidy_whitespace: sortable && context.regular_element }
    }
}

/// Write a value whose `{…}` are text: every byte of it, quoted.
///
/// The quote is still chosen rather than kept, which is what Prettier does
/// for these too — `lang='ts'` comes back `lang="ts"`.
fn write_literal_value<'a>(
    value: &AttributeValue<'a>,
    source: &'a str,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    let text_value = slice(source, value.span.start, value.span.end);
    let quote = if text_value.contains('"') { token("'") } else { token("\"") };
    write!(f, [quote, text(text_value), quote]);
    // A value the author spread over lines takes the attribute list with it:
    // the newline is inside a text token the printer cannot see.
    if text_value.contains('\n') {
        write!(f, expand_parent());
    }
}

/// Write the value after `=`, keeping its quoting as written unless it has
/// none to keep.
fn write_value<'a>(
    value: &AttributeValue<'a>,
    class_list: ClassAttribute,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    write_value_at(value, class_list, ExpressionPosition::Braces, f);
}

/// As [`write_value`], for a value whose lone `{…}` sits in a position with a
/// layout of its own.
fn write_value_at<'a>(
    value: &AttributeValue<'a>,
    class_list: ClassAttribute,
    position: ExpressionPosition,
    f: &mut SvelteFormatter<'_, 'a>,
) {
    // A value that is exactly one `{…}` needs no quotes; anything else is
    // quoted, since removing them could change where the value ends.
    if let Some(tag) = value.as_single_expression() {
        write_expression_tag(tag, position, f);
        return;
    }
    // `"` unless the value contains one, in which case `'`. Prettier writes
    // `"` either way, which turns `prop='"'` into `prop=""" ` — markup that
    // no longer parses. Divergence recorded in `keeps_a_value_that_is_quoted`.
    let quote = if contains_double_quote(value) { token("'") } else { token("\"") };
    write!(f, quote);
    let last = value.parts.len().saturating_sub(1);
    for (index, part) in value.parts.iter().enumerate() {
        match part {
            ValuePart::Text(part) => {
                if !class_list.sortable || !write_tailwind_classes(part.value, f) {
                    let value = if class_list.tidy_whitespace {
                        // The tidied text is new, so it has to live in the
                        // arena the IR does rather than in this frame.
                        match tidied_classes(part.value, index == last) {
                            Cow::Owned(tidied) => f.allocator().alloc_str(&tidied),
                            Cow::Borrowed(value) => value,
                        }
                    } else {
                        part.value
                    };
                    write!(f, text(value));
                }
                // A value the author spread over lines takes the attribute
                // list with it: the tag can no longer sit on one line, and
                // the printer has to be told, since the newline is inside a
                // text token it cannot see.
                if part.value.contains('\n') {
                    write!(f, expand_parent());
                }
            }
            ValuePart::Expression(tag) => {
                write_expression_tag(tag, ExpressionPosition::QuotedAttribute, f);
            }
        }
    }
    write!(f, quote);
}

/// A `class` attribute's text with its whitespace tidied, which is the one
/// attribute value Prettier reflows at all.
///
/// A run of spaces or tabs following something on the same line collapses to
/// a single space, or to nothing when a line break follows it; a run at the
/// very end of the value goes entirely when this is the last part of the
/// value, and becomes one space when an expression follows it. A run on a
/// line of its own — indentation the author wrote — is left alone, which is
/// what makes a multi-line class list keep its shape.
fn tidied_classes(value: &str, is_last_part: bool) -> Cow<'_, str> {
    let bytes = value.as_bytes();
    let mut out: Option<String> = None;
    let mut index = 0;
    let mut copied_from = 0;
    while index < bytes.len() {
        if !matches!(bytes[index], b' ' | b'\t') {
            index += 1;
            continue;
        }
        // Only a run that follows something on its own line is collapsible.
        if index == 0 || matches!(bytes[index - 1], b' ' | b'\t' | b'\n') {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
            index += 1;
        }
        let replacement = match bytes.get(index) {
            None => {
                if is_last_part {
                    ""
                } else {
                    " "
                }
            }
            Some(b'\n') => "",
            Some(_) => " ",
        };
        if value[start..index] == *replacement {
            continue;
        }
        let out = out.get_or_insert_with(|| String::with_capacity(value.len()));
        out.push_str(&value[copied_from..start]);
        out.push_str(replacement);
        copied_from = index;
    }
    match out {
        Some(mut out) => {
            out.push_str(&value[copied_from..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(value),
    }
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
    let index = f.session().add_tailwind_class(classes.to_string());
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
