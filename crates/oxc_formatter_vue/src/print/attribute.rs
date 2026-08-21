//! Printing one attribute, and the language inside its value.
//!
//! The name is kept exactly as written — a Vue template's attribute names are
//! case-sensitive, and `:class`, `@click`, `#default` and `v-bind` are all
//! spellings the compiler distinguishes. The *value* is where the work is: an
//! attribute name says what language its value is in, and most of them are
//! JavaScript. `:prop` and `{{ … }}` are expressions, `@click` is an
//! expression or a run of statements, `v-slot` is a parameter list, `v-for`
//! is a small grammar of its own.
//!
//! A value whose name says nothing is text, and only its quoting is
//! normalised.

use std::cell::RefCell;

use cow_utils::CowUtils;
use oxc_allocator::ArenaVec;
use oxc_formatter_core::{
    Buffer, BufferExtensions, Format, FormatElement,
    builders::{group, indent, soft_line_break, space, text, token},
    escape_double_quotes, write,
};
use vue_sfc_parser::ast::Attribute;

use crate::context::VueFormatContext;

use super::{
    VueFormatter, format_with,
    tree::{NodeId, Tree},
};

/// Print `name`, `name="value"`, or `name='value'` when the value contains
/// more double quotes than single ones.
pub fn write_attribute<'a>(
    tree: &Tree<'_, 'a>,
    element: NodeId,
    attribute: &Attribute<'a>,
    f: &mut VueFormatter<'_, 'a>,
) {
    write!(f, text(attribute.name));
    // A bare attribute has no value at all, which is not the same as an empty
    // one: `disabled` and `disabled=""` are both legal and both preserved.
    let Some(value) = &attribute.value else { return };
    let raw = unescape_quote_entities(value.text, f);

    // A printer that cannot produce a value — the fragment did not parse —
    // leaves nothing written, and the value falls back to its own text. That
    // is deliberately more forgiving than Prettier, which fails the file.
    if write_printed_value(tree, element, attribute, raw, f) {
        return;
    }
    write_plain_value(raw, f);
}

/// The value as text, in whichever quote needs the less escaping.
fn write_plain_value<'a>(raw: &'a str, f: &mut VueFormatter<'_, 'a>) {
    let quote = preferred_quote(raw);
    let escaped = escape_for_quote(raw, quote, f);
    let quote_token = if quote == b'"' { token("\"") } else { token("'") };
    write!(f, [token("="), quote_token, text(escaped), quote_token]);
}

/// Which language an attribute's value is written in, if any.
#[derive(Clone, Copy, Debug)]
enum ValuePrinter {
    /// `class`: a list of names, whose whitespace is tidied.
    ClassNames,
    /// `style`: a CSS declaration list.
    Style,
    /// A JavaScript expression, under the named dispatch language.
    Expression(&'static str),
    /// `@click`: an expression, or statements when it is not one.
    EventHandler,
    /// `v-for="(item, index) in items"`.
    VFor,
    /// `v-slot` / `#default` / `<script setup="…">`: a parameter list.
    Bindings,
    /// `<script generic="T">`: type parameters.
    Generic,
}

/// Prettier's printer table, in its order.
fn value_printer(
    tree: &Tree<'_, '_>,
    element: NodeId,
    attribute: &Attribute<'_>,
    raw: &str,
) -> Option<ValuePrinter> {
    let name = attribute.name;

    // A value carrying an interpolation is not the language its name says: it
    // is a template the Vue compiler assembles, and reformatting the pieces
    // would break the seams.
    if name == "style" && !raw.contains("{{") {
        return Some(ValuePrinter::Style);
    }
    if name == "class" && !raw.contains("{{") {
        return Some(ValuePrinter::ClassNames);
    }
    if name == "v-for" {
        return Some(ValuePrinter::VFor);
    }
    if name == "generic" && tree.is_vue_sfc_block(element) && tree.node(element).name() == "script"
    {
        return Some(ValuePrinter::Generic);
    }
    if is_slot_attribute(name) || is_sfc_bindings_attribute(tree, element, name) {
        return Some(ValuePrinter::Bindings);
    }
    if name.starts_with('@') || name.starts_with("v-on:") {
        return Some(ValuePrinter::EventHandler);
    }
    // `:prop` gets the Vue expression flavour; a plain `v-if` does not, which
    // is Prettier's distinction between `__vue_expression` and
    // `__ts_expression` and decides whether a string literal hugs the quotes.
    let flavour = tree.script_flavour();
    if name.starts_with(':') || name.starts_with('.') || name.starts_with("v-bind:") {
        return Some(ValuePrinter::Expression(flavour.bound_attribute()));
    }
    if name.starts_with("v-") {
        return Some(ValuePrinter::Expression(flavour.attribute_expression()));
    }
    None
}

/// `v-slot`, `#default`, `slot-scope` — the attributes that declare the
/// bindings a slot hands down.
fn is_slot_attribute(name: &str) -> bool {
    name.starts_with('#') || name == "slot-scope" || name == "v-slot" || name.starts_with("v-slot:")
}

/// `<script setup="…">` and `<style vars="…">`, whose values are also binding
/// lists.
fn is_sfc_bindings_attribute(tree: &Tree<'_, '_>, element: NodeId, name: &str) -> bool {
    if !tree.is_vue_sfc_block(element) {
        return false;
    }
    matches!((tree.node(element).name(), name), ("script", "setup") | ("style", "vars"))
}

/// Print `="value"` through whichever printer the name selects. Returns
/// whether one did; a `false` leaves nothing written.
fn write_printed_value<'a>(
    tree: &Tree<'_, 'a>,
    element: NodeId,
    attribute: &Attribute<'a>,
    raw: &'a str,
    f: &mut VueFormatter<'_, 'a>,
) -> bool {
    let Some(printer) = value_printer(tree, element, attribute, raw) else { return false };
    match printer {
        ValuePrinter::ClassNames => {
            write!(f, token("=\""));
            if !write_tailwind_classes(raw, f) {
                write!(f, text(tidied_class_list(raw, f)));
            }
            write!(f, token("\""));
            true
        }
        // Unlike an expression, a declaration list never hugs: it always
        // gets the indented break of its own that Prettier's `printExpand`
        // supplies.
        ValuePrinter::Style => {
            // A value with no declarations in it has none to print, whatever
            // whitespace it was written with.
            if raw.trim().is_empty() {
                write!(f, token("=\"\""));
                return true;
            }
            let Some(value) = dispatch("css-style-attribute", raw, raw, f) else { return false };
            write_value_doc(Value { hugs: false, ..value }, f);
            true
        }
        ValuePrinter::Expression(language) => {
            let Some(value) = dispatch(language, raw, raw, f) else { return false };
            write_value_doc(value, f);
            true
        }
        // Prettier tries the expression first and only reads the value as
        // statements when it does not parse as one — which is what keeps
        // `@click="doThing($event)"` a call rather than a statement list.
        ValuePrinter::EventHandler => {
            let flavour = tree.script_flavour();
            let value = dispatch(flavour.attribute_expression(), raw, raw, f)
                .or_else(|| dispatch(flavour.event_handler(), raw, raw, f));
            let Some(value) = value else { return false };
            write_value_doc(value, f);
            true
        }
        ValuePrinter::Bindings => {
            let wrapped = f.allocator().alloc_str(&format!("function _({raw}) {{}}"));
            let language = tree.script_flavour().binding_params();
            let Some(value) = dispatch(language, wrapped, raw, f) else { return false };
            write_value_doc(value, f);
            true
        }
        ValuePrinter::Generic => {
            let wrapped = f.allocator().alloc_str(&format!("type T<{raw}> = any"));
            let Some(value) = dispatch("vue-generic", wrapped, raw, f) else { return false };
            write_value_doc(value, f);
            true
        }
        ValuePrinter::VFor => write_v_for(tree, raw, f),
    }
}

/// `v-for="(item, index) in items"`: a binding list, a keyword, and an
/// expression, each laid out on its own.
fn write_v_for<'a>(tree: &Tree<'_, 'a>, raw: &'a str, f: &mut VueFormatter<'_, 'a>) -> bool {
    let flavour = tree.script_flavour();
    let Some(parsed) = parse_v_for(raw) else { return false };
    let wrapped = f.allocator().alloc_str(&format!("function _({}) {{}}", parsed.left));
    let Some(left) = dispatch(flavour.v_for_left(), wrapped, parsed.left.as_str(), f) else {
        return false;
    };
    let Some(right) = dispatch(flavour.attribute_expression(), parsed.right, parsed.right, f)
    else {
        return false;
    };

    let left = Doc::new(left.ir);
    let right = Doc::new(right.ir);
    let operator = if parsed.operator == "in" { token("in") } else { token("of") };
    write!(f, token("=\""));
    write!(
        f,
        group(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            write!(f, group(&group(&left)));
            write!(f, [space(), operator, space()]);
            write!(f, group(&right));
        }))
    );
    write!(f, token("\""));
    true
}

/// Write `="…"` around a formatted value.
///
/// An expression whose own brackets can hold the indentation keeps the quotes
/// against it; anything else gets a break of its own, so a long value moves
/// below the attribute name rather than running past the margin.
fn write_value_doc<'a>(value: Value<'a>, f: &mut VueFormatter<'_, 'a>) {
    let hugs = value.hugs;
    let doc = Doc::new(value.ir);
    write!(f, token("=\""));
    write!(
        f,
        group(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            if hugs {
                write!(f, group(&doc));
            } else {
                write!(
                    f,
                    indent(&format_with(|f: &mut VueFormatter<'_, 'a>| {
                        write!(f, [soft_line_break(), &doc]);
                    }))
                );
                write!(f, soft_line_break());
            }
        }))
    );
    write!(f, token("\""));
}

/// A formatted fragment, with the layout answer its language gave.
struct Value<'a> {
    ir: ArenaVec<'a, FormatElement<'a>>,
    hugs: bool,
}

/// Format `source` as `language`, or `None` when nothing can.
///
/// `snippet` is the value the author wrote, which differs from `source` for
/// the contexts that have to be wrapped before they parse.
fn dispatch<'a>(
    language: &'static str,
    source: &str,
    snippet: &str,
    f: &mut VueFormatter<'_, 'a>,
) -> Option<Value<'a>> {
    let mut fragment = super::embed::dispatch(language, source, snippet, f)?;
    if fragment.ir.is_empty() {
        return None;
    }
    // The value goes inside `="…"`, so a `"` the fragment carries — in a
    // string, a template, a regex, a comment — would end the attribute early
    // and the markup would no longer parse. Only the attribute path escapes;
    // an interpolation has no delimiter to protect.
    let indent_width = f.options().indent_width;
    escape_double_quotes(&mut fragment.ir, f.allocator(), indent_width);
    Some(Value { ir: fragment.ir, hugs: fragment.hugs })
}

/// A child document's IR, written once into whatever position the layout puts
/// it.
///
/// The builders take their content by reference, so the elements are moved out
/// on the first write.
struct Doc<'a>(RefCell<Option<ArenaVec<'a, FormatElement<'a>>>>);

impl<'a> Doc<'a> {
    fn new(elements: ArenaVec<'a, FormatElement<'a>>) -> Self {
        Self(RefCell::new(Some(elements)))
    }
}

impl<'a> Format<'a, VueFormatContext<'a>> for Doc<'a> {
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        if let Some(elements) = self.0.borrow_mut().take() {
            f.write_elements(elements);
        }
    }
}

// ---------------------------------------------------------------------------
// `class`
// ---------------------------------------------------------------------------

/// Register the class list for the host's sorter and write the placeholder it
/// fills in. Returns whether it did.
fn write_tailwind_classes<'a>(classes: &'a str, f: &mut VueFormatter<'_, 'a>) -> bool {
    if !f.options().sort_tailwind_classes || classes.trim().is_empty() {
        return false;
    }
    let index = f.session().add_tailwind_class(classes.to_string());
    f.write_element(FormatElement::TailwindClass(index));
    true
}

/// The class list with its whitespace collapsed to single spaces. This is the
/// one attribute value Prettier reflows without parsing it.
fn tidied_class_list<'a>(value: &'a str, f: &VueFormatter<'_, 'a>) -> &'a str {
    let trimmed = value.trim();
    if !trimmed.bytes().any(|byte| matches!(byte, b'\n' | b'\t' | b'\r' | 0x0c))
        && !trimmed.contains("  ")
        && !trimmed.contains('"')
    {
        return trimmed;
    }
    let tidied = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    // Written straight into `="…"`, so it needs the same protection a
    // formatted value gets.
    f.allocator().alloc_str(&tidied.cow_replace('"', "&quot;"))
}

// ---------------------------------------------------------------------------
// `v-for`
// ---------------------------------------------------------------------------

struct VFor<'a> {
    /// The declared names, comma-separated — what goes on the left of `in`.
    left: String,
    operator: &'a str,
    right: &'a str,
}

/// Split `v-for="(item, index) in items"` into its three parts, or `None`
/// when the value is not that shape — in which case it is left as written
/// rather than being guessed at.
///
/// Ported from Vue's own compiler, by way of Prettier.
fn parse_v_for(value: &str) -> Option<VFor<'_>> {
    let (alias, operator, right) = split_on_keyword(value)?;
    let right = right.trim();
    if right.is_empty() {
        return None;
    }

    // `(item, index)` and `item` both declare the same thing; the parentheses
    // are Vue's syntax, not the binding's.
    let alias = alias.trim();
    let alias = alias.strip_prefix('(').unwrap_or(alias);
    let alias = alias.strip_suffix(')').unwrap_or(alias);

    let (name, iterator1, iterator2) = match split_iterators(alias) {
        Some((name, first, second)) => (name, first.trim(), second.map(str::trim)),
        None => (alias, "", None),
    };

    let parts = [name, iterator1, iterator2.unwrap_or("")];
    // A hole in the list — `(, index)` — means this is not a binding list at
    // all, and the value is left alone.
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() && (index == 0 || parts[index + 1..].iter().any(|rest| !rest.is_empty()))
        {
            return None;
        }
    }

    let left = parts.iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join(",");
    Some(VFor { left, operator, right })
}

/// The first whitespace-delimited `in` or `of`, with what falls either side.
fn split_on_keyword(value: &str) -> Option<(&str, &str, &str)> {
    let mut search = 0;
    while search < value.len() {
        let Some(offset) = value[search..].find(char::is_whitespace) else { return None };
        let whitespace_start = search + offset;
        let after = value[whitespace_start..].trim_start();
        for keyword in ["in", "of"] {
            if let Some(rest) = after.strip_prefix(keyword)
                && rest.starts_with(char::is_whitespace)
            {
                return Some((&value[..whitespace_start], keyword, rest));
            }
        }
        search =
            whitespace_start + value[whitespace_start..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// Split `item, index` or `item, key, index` into its parts, leaving anything
/// inside brackets alone — `{ a, b }, i` declares two things, not three.
fn split_iterators(alias: &str) -> Option<(&str, &str, Option<&str>)> {
    let bytes = alias.as_bytes();
    for (position, byte) in bytes.iter().enumerate() {
        if *byte != b',' {
            continue;
        }
        let rest = &alias[position + 1..];
        let first_end =
            rest.bytes().position(|byte| matches!(byte, b',' | b']' | b'}')).unwrap_or(rest.len());
        if first_end == rest.len() {
            return Some((&alias[..position], rest, None));
        }
        if rest.as_bytes()[first_end] != b',' {
            // A bracket closes before the list does, so this comma is inside
            // a pattern rather than between two declarations.
            continue;
        }
        let second = &rest[first_end + 1..];
        if second.bytes().any(|byte| matches!(byte, b',' | b']' | b'}')) {
            continue;
        }
        return Some((&alias[..position], &rest[..first_end], Some(second)));
    }
    None
}

// ---------------------------------------------------------------------------
// Quoting
// ---------------------------------------------------------------------------

/// The quote the value is written in: double, unless that would mean escaping
/// more characters than single quotes would.
fn preferred_quote(value: &str) -> u8 {
    let doubles = value.bytes().filter(|byte| *byte == b'"').count();
    let singles = value.bytes().filter(|byte| *byte == b'\'').count();
    if doubles > singles { b'\'' } else { b'"' }
}

/// Turn `&apos;` and `&quot;` back into the characters they stand for, so the
/// choice of quote below is made against what the value really contains.
fn unescape_quote_entities<'a>(value: &'a str, f: &VueFormatter<'_, 'a>) -> &'a str {
    if !value.contains("&apos;") && !value.contains("&quot;") {
        return value;
    }
    f.allocator().alloc_str(&value.cow_replace("&apos;", "'").cow_replace("&quot;", "\""))
}

/// Escape whichever quote the value is about to be wrapped in.
fn escape_for_quote<'a>(value: &'a str, quote: u8, f: &VueFormatter<'_, 'a>) -> &'a str {
    let (needle, entity) = if quote == b'"' { ('"', "&quot;") } else { ('\'', "&apos;") };
    if !value.contains(needle) {
        return value;
    }
    f.allocator().alloc_str(&value.cow_replace(needle, entity))
}

#[cfg(test)]
mod tests {
    use super::{parse_v_for, preferred_quote};

    #[test]
    fn double_quotes_win_ties() {
        assert_eq!(preferred_quote("plain"), b'"');
        assert_eq!(preferred_quote("it's"), b'"');
        assert_eq!(preferred_quote("say \"hi\""), b'\'');
        // A tie keeps the preferred quote rather than switching.
        assert_eq!(preferred_quote("\"'"), b'"');
    }

    #[track_caller]
    fn v_for(value: &str) -> (String, &'static str, String) {
        let parsed = parse_v_for(value).expect("a v-for value");
        let operator: &'static str = if parsed.operator == "in" { "in" } else { "of" };
        (parsed.left, operator, parsed.right.to_string())
    }

    #[test]
    fn v_for_shapes() {
        assert_eq!(v_for("item in items"), ("item".into(), "in", "items".into()));
        assert_eq!(v_for("(item, index) in items"), ("item,index".into(), "in", "items".into()));
        assert_eq!(v_for("(v, k, i) of map"), ("v,k,i".into(), "of", "map".into()));
        // A comma inside a destructuring pattern is not a separator.
        assert_eq!(v_for("({ a, b }, i) in xs"), ("{ a, b },i".into(), "in", "xs".into()));
        // The right-hand side may be any expression, `in` included.
        assert_eq!(v_for("x in a in b"), ("x".into(), "in", "a in b".into()));
    }

    #[test]
    fn a_value_that_is_not_a_v_for_is_left_alone() {
        assert!(parse_v_for("items").is_none());
        assert!(parse_v_for("item in ").is_none());
        assert!(parse_v_for("(, index) in items").is_none());
    }
}
