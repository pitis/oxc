//! Printing an element: its tags, and how its content sits between them.

use oxc_formatter_core::{
    Buffer,
    builders::{
        dedent_to_root, expand_parent, group, hard_line_break, if_group_breaks, indent,
        indent_if_group_breaks, soft_line_break, soft_line_break_or_space, space, text,
    },
    write,
};

use super::{
    VueFormatter,
    children::{force_break_content, write_children},
    embed::write_formatted,
    format_with,
    tag::{
        needs_to_borrow_last_child_closing_tag_end_marker,
        needs_to_borrow_parent_closing_tag_start_marker,
        needs_to_borrow_parent_opening_tag_end_marker, needs_to_borrow_prev_closing_tag_end_marker,
        write_closing_tag, write_closing_tag_suffix, write_opening_tag, write_opening_tag_prefix,
    },
    tree::{Kind, NodeId, Tree},
};

pub fn write_element<'a>(tree: &Tree<'_, 'a>, id: NodeId, f: &mut VueFormatter<'_, 'a>) {
    let node = tree.node(id);

    // A top-level block in another language — `<i18n lang="json">` and the
    // like. The tags are this printer's; the body belongs to whoever owns
    // that language.
    if !node.is_self_closing
        && tree.is_vue_non_html_block(id)
        && !tree.is_script_like(id)
        && let Some(language) = custom_block_language(tree, id)
    {
        write_custom_block(tree, id, language, f);
        return;
    }

    // Content this printer must not reshape: a `<pre>` with markup inside it,
    // or a block whose language nothing here knows.
    if tree.should_preserve_content(id) {
        write_opening_tag_prefix(tree, id, f);
        write!(
            f,
            group(&format_with(|f: &mut VueFormatter<'_, 'a>| write_opening_tag(tree, id, f)))
        );
        write!(f, text(node_content(tree, id)));
        write_closing_tag(tree, id, f);
        write_closing_tag_suffix(tree, id, f);
        return;
    }

    let attribute_group = f.state().group_id("vue-element-attributes");

    // `<div>{{ x }}</div>` keeps the interpolation between its tags rather
    // than moving it onto a line of its own: there is no whitespace at either
    // end, so a break there would add one the page renders. The exception is
    // an opening tag that had to break anyway, which takes the content with
    // it.
    let hugs_content = node.children.len() == 1 && {
        let first = tree.node(node.children[0]);
        first.kind == Kind::Interpolation
            && first.is_leading_space_sensitive
            && !first.has_leading_spaces
            && first.is_trailing_space_sensitive
            && !first.has_trailing_spaces
    };

    // An opening tag with no attributes holds nothing that could break, so
    // every layout keyed on "did the tag break?" is settled in advance. Saying
    // so directly, rather than asking a group that cannot break, keeps the
    // measurement from treating the tag as though it had broken — which is
    // what a conditional on an unresolved group would do here.
    let hug = match (hugs_content, node.attributes().is_empty()) {
        (false, _) => Hug::No,
        (true, false) => Hug::WhenTagBreaks,
        (true, true) => Hug::Always,
    };

    write!(
        f,
        group(&format_with(|f: &mut VueFormatter<'_, 'a>| {
            write!(
                f,
                group(&format_with(|f: &mut VueFormatter<'_, 'a>| write_opening_tag(tree, id, f)))
                    .with_group_id(Some(attribute_group))
            );
            write_body(tree, id, hug, attribute_group, f);
            write_closing_tag(tree, id, f);
        }))
    );
}

/// Whether an element's content stays between its tags, and on what.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Hug {
    /// The content is laid out below the tags as usual.
    No,
    /// The content hugs unless the opening tag broke, in which case it goes
    /// below with the attributes.
    WhenTagBreaks,
    /// The content hugs, full stop: the opening tag has no attributes, so it
    /// has nothing to break on.
    Always,
}

fn write_body<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    hug: Hug,
    attribute_group: oxc_formatter_core::GroupId,
    f: &mut VueFormatter<'_, 'a>,
) {
    let node = tree.node(id);

    if node.children.is_empty() {
        // Nothing between the tags, except possibly whitespace that renders.
        if node.has_dangling_spaces && node.is_dangling_space_sensitive {
            write!(f, soft_line_break_or_space());
        }
        return;
    }

    if force_break_content(tree, id) {
        write!(f, expand_parent());
    }

    let content = format_with(|f: &mut VueFormatter<'_, 'a>| {
        write_line_before_children(tree, id, hug, attribute_group, f);
        write_children(tree, id, f);
    });

    if hug == Hug::WhenTagBreaks {
        write!(f, indent_if_group_breaks(&content, attribute_group));
    } else if hug == Hug::Always {
        write!(f, &content);
    } else if is_unindented_block(tree, id, f) {
        // A component's `<script>` and `<style>` sit at the left margin by
        // default: their bodies are whole files in another language, and
        // indenting them would indent every line of every one.
        write!(f, &content);
    } else {
        write!(f, indent(&content));
    }

    write_line_after_children(tree, id, hug, attribute_group, f);
}

/// Whether the block's body keeps the file's own indentation rather than
/// being nested inside its tags.
fn is_unindented_block(tree: &Tree<'_, '_>, id: NodeId, f: &VueFormatter<'_, '_>) -> bool {
    (tree.is_script_like(id) || tree.is_vue_custom_block(id))
        && tree.node(id).parent == Some(super::tree::ROOT)
        && !f.options().indent_script_and_style
}

fn write_line_before_children<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    hug: Hug,
    attribute_group: oxc_formatter_core::GroupId,
    f: &mut VueFormatter<'_, 'a>,
) {
    match hug {
        Hug::WhenTagBreaks => {
            write!(f, if_group_breaks(&soft_line_break()).with_group_id(Some(attribute_group)));
            return;
        }
        Hug::Always => return,
        Hug::No => {}
    }
    let node = tree.node(id);
    let first = tree.node(node.children[0]);
    if first.has_leading_spaces && first.is_leading_space_sensitive {
        write!(f, soft_line_break_or_space());
        return;
    }
    if first.kind == Kind::Text && node.is_whitespace_sensitive && node.is_indentation_sensitive {
        // The first line of a `<pre>` starts at the left margin, whatever the
        // markup around it is indented to.
        write!(f, dedent_to_root(&soft_line_break()));
        return;
    }
    write!(f, soft_line_break());
}

fn write_line_after_children<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    hug: Hug,
    attribute_group: oxc_formatter_core::GroupId,
    f: &mut VueFormatter<'_, 'a>,
) {
    let node = tree.node(id);
    let last_id = *node.children.last().expect("called only with children");
    let last = tree.node(last_id);

    // A neighbour has taken the closing delimiter, so there is no break to
    // make here — only the space it would have rendered as.
    let lends_closing_marker = match tree.next(id) {
        Some(next) => needs_to_borrow_prev_closing_tag_end_marker(tree, next),
        None => node
            .parent
            .is_some_and(|parent| needs_to_borrow_last_child_closing_tag_end_marker(tree, parent)),
    };
    if lends_closing_marker {
        if last.has_trailing_spaces && last.is_trailing_space_sensitive {
            write!(f, space());
        }
        return;
    }

    if tree.is_pre_like(id) && needs_to_borrow_parent_closing_tag_start_marker(tree, last_id) {
        return;
    }
    match hug {
        Hug::WhenTagBreaks => {
            write!(f, if_group_breaks(&soft_line_break()).with_group_id(Some(attribute_group)));
            return;
        }
        Hug::Always => return,
        Hug::No => {}
    }
    if last.has_trailing_spaces && last.is_trailing_space_sensitive {
        write!(f, soft_line_break_or_space());
        return;
    }
    // The content already ends on a line of its own, indented to where the
    // closing tag goes: adding a break would leave a blank line.
    let indentation =
        usize::from(f.options().indent_width.value()) * tree.depth(id).saturating_sub(1);
    if (last.kind == Kind::Comment
        || (last.kind == Kind::Text
            && node.is_whitespace_sensitive
            && node.is_indentation_sensitive))
        && ends_with_line_and_indent(last.value, indentation)
    {
        return;
    }
    write!(f, soft_line_break());
}

/// Whether the text ends with a line break followed by exactly `indent`
/// columns of horizontal whitespace.
fn ends_with_line_and_indent(value: &str, indent: usize) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < indent + 1 {
        return false;
    }
    let split = bytes.len() - indent;
    bytes[split - 1] == b'\n' && bytes[split..].iter().all(|byte| matches!(byte, b'\t' | b' '))
}

/// A block whose body is a whole document in another language.
fn write_custom_block<'a>(
    tree: &Tree<'_, 'a>,
    id: NodeId,
    language: &'static str,
    f: &mut VueFormatter<'_, 'a>,
) {
    let content = node_content(tree, id);
    write_opening_tag_prefix(tree, id, f);
    write!(f, group(&format_with(|f: &mut VueFormatter<'_, 'a>| write_opening_tag(tree, id, f))));
    if !content.trim().is_empty() {
        write!(f, hard_line_break());
        if !write_formatted(language, content.trim(), f) {
            write!(f, text(content.trim()));
        }
        write!(f, hard_line_break());
    }
    write_closing_tag(tree, id, f);
    write_closing_tag_suffix(tree, id, f);
}

/// The language a custom block's `lang` names, of the ones something here can
/// format.
fn custom_block_language(tree: &Tree<'_, '_>, id: NodeId) -> Option<&'static str> {
    let node = tree.node(id);
    if node.attribute_value("src").is_some() {
        return None;
    }
    match node.attribute_value("lang")? {
        "json" => Some("json"),
        "json5" => Some("json5"),
        "jsonc" => Some("jsonc"),
        "yaml" | "yml" => Some("yaml"),
        "css" => Some("css"),
        "scss" => Some("scss"),
        "less" => Some("less"),
        "js" | "javascript" => Some("js"),
        "ts" | "typescript" => Some("ts"),
        _ => None,
    }
}

/// Everything between an element's tags, exactly as written — including any
/// delimiter a child borrowed, which is printed here rather than by the child.
pub fn node_content<'a>(tree: &Tree<'_, 'a>, id: NodeId) -> &'a str {
    let Some(end_span) = tree.end_span(id) else { return "" };
    let mut start = tree.start_span(id).end as usize;
    if tree
        .first_child(id)
        .is_some_and(|child| needs_to_borrow_parent_opening_tag_end_marker(tree, child))
    {
        start -= super::tag::opening_tag_end_marker(tree, id).len();
    }

    let mut end = end_span.start as usize;
    match tree.last_child(id) {
        Some(last) if needs_to_borrow_parent_closing_tag_start_marker(tree, last) => {
            end += super::tag::closing_tag_start_marker(tree, id).len();
        }
        _ if needs_to_borrow_last_child_closing_tag_end_marker(tree, id) => {
            let last = tree.last_child(id).expect("the borrow implies a last child");
            end -= super::tag::closing_tag_end_marker(tree, last).len();
        }
        _ => {}
    }

    // An element written self-closing has one span for both its tags, so
    // "between them" is empty and the two offsets cross. JavaScript's `slice`
    // yields `""` for that; a Rust range would panic.
    if end <= start {
        return "";
    }
    &tree.source()[start..end]
}
