use crate::ast::{DirectiveShorthand, Node};
use crate::{parse_sfc, parse_template};

fn only_element<'a>(nodes: &'a [Node<'a>]) -> &'a crate::ast::Element<'a> {
    let elements: Vec<_> = nodes
        .iter()
        .filter_map(|node| if let Node::Element(element) = node { Some(element) } else { None })
        .collect();
    assert_eq!(elements.len(), 1, "expected exactly one element");
    elements[0]
}

#[test]
fn parses_element_tree_with_text_and_interpolation() {
    let nodes = parse_template("<div class=\"a\">hi {{ name }}!</div>");
    let div = only_element(&nodes);
    assert_eq!(div.name, "div");
    assert_eq!(div.attributes.len(), 1);
    assert_eq!(div.attributes[0].name, "class");
    assert_eq!(div.attributes[0].value.as_ref().unwrap().text, "a");
    assert!(div.attributes[0].directive.is_none());
    assert_eq!(div.children.len(), 3);
    let Node::Interpolation(interpolation) = &div.children[1] else {
        panic!("expected interpolation");
    };
    assert_eq!(interpolation.expression, " name ");
}

#[test]
fn decomposes_directives() {
    let nodes = parse_template(
        r#"<input v-if="ok" :value="v" @keyup.enter.stop="go" v-on:click="c" #default v-bind:[key].sync="d" v-my-thing:arg.m="x" />"#,
    );
    let input = only_element(&nodes);
    let directive =
        |index: usize| input.attributes[index].directive.as_ref().expect("directive expected");

    let v_if = directive(0);
    assert_eq!((v_if.name, v_if.shorthand), ("if", None));
    assert!(v_if.argument.is_none());

    let bind = directive(1);
    assert_eq!((bind.name, bind.shorthand), ("bind", Some(DirectiveShorthand::Bind)));
    assert_eq!(bind.argument.as_ref().unwrap().text, "value");

    let on_keyup = directive(2);
    assert_eq!((on_keyup.name, on_keyup.shorthand), ("on", Some(DirectiveShorthand::On)));
    assert_eq!(on_keyup.argument.as_ref().unwrap().text, "keyup");
    assert_eq!(on_keyup.modifiers, ["enter", "stop"]);

    let on_click = directive(3);
    assert_eq!((on_click.name, on_click.shorthand), ("on", None));
    assert_eq!(on_click.argument.as_ref().unwrap().text, "click");

    let slot = directive(4);
    assert_eq!((slot.name, slot.shorthand), ("slot", Some(DirectiveShorthand::Slot)));
    assert_eq!(slot.argument.as_ref().unwrap().text, "default");

    let dynamic = directive(5);
    assert_eq!(dynamic.name, "bind");
    let argument = dynamic.argument.as_ref().unwrap();
    assert_eq!((argument.text, argument.dynamic), ("[key]", true));
    assert_eq!(dynamic.modifiers, ["sync"]);

    let custom = directive(6);
    assert_eq!(custom.name, "my-thing");
    assert_eq!(custom.argument.as_ref().unwrap().text, "arg");
    assert_eq!(custom.modifiers, ["m"]);
}

#[test]
fn void_and_self_closing_elements_have_no_children() {
    let nodes = parse_template("<br><Item :x=\"1\" /><img src=\"a\">");
    let names: Vec<_> = nodes
        .iter()
        .filter_map(|node| if let Node::Element(element) = node { Some(element) } else { None })
        .collect();
    assert_eq!(names.len(), 3);
    assert!(names[0].is_void);
    assert!(names[1].self_closing);
    assert!(names[1].is_component_like());
    assert!(names[2].is_void && !names[2].is_component_like());
}

#[test]
fn raw_text_elements_keep_bodies_verbatim() {
    let source = "<pre>  <div> not parsed {{ x }}  </pre>";
    let nodes = parse_template(source);
    let pre = only_element(&nodes);
    let raw = pre.raw_text.expect("raw text");
    assert_eq!(&source[raw.start as usize..raw.end as usize], "  <div> not parsed {{ x }}  ");
    assert!(pre.children.is_empty());
}

#[test]
fn recovers_from_unclosed_elements() {
    let nodes = parse_template("<ul><li>a<li>b</ul>");
    let ul = only_element(&nodes);
    assert_eq!(ul.name, "ul");
    let list_items: Vec<_> = ul
        .children
        .iter()
        .filter_map(|node| if let Node::Element(element) = node { Some(element) } else { None })
        .collect();
    assert_eq!(list_items.len(), 2);
    assert!(list_items[0].unclosed);
    // The second `<li>` swallows `b` and recovers at `</ul>`.
    assert!(list_items[1].unclosed);
}

#[test]
fn comments_and_doctype() {
    let nodes = parse_template("<!-- note --><!DOCTYPE html><span>x</span>");
    assert!(matches!(&nodes[0], Node::Comment(comment) if comment.content == " note "));
    assert!(matches!(&nodes[1], Node::Raw(_)));
}

#[test]
fn sfc_blocks_split_and_script_is_raw() {
    let source = "<script setup lang=\"ts\">\n/** mentions </template> and <script> */\nconst a = 1;\n</script>\n\n<template>\n  <div v-if=\"a\">{{ a }}</div>\n  <template #fallback>f</template>\n</template>\n\n<style scoped>\n.a { color: red; }\n</style>\n";
    let sfc = parse_sfc(source);
    assert_eq!(sfc.blocks.len(), 3);

    let script = &sfc.blocks[0];
    assert_eq!(script.name, "script");
    assert!(script.has_attribute("setup"));
    assert_eq!(script.lang(), Some("ts"));
    assert!(script.content.contains("</template>"), "script content must be raw");

    let template = &sfc.blocks[1];
    assert_eq!(template.name, "template");
    // The nested `<template #fallback>` must not have closed the block.
    assert!(template.content.contains("#fallback"));
    assert!(template.content.trim_end().ends_with("</template>"));

    let style = &sfc.blocks[2];
    assert_eq!(style.name, "style");
    assert!(style.has_attribute("scoped"));
    assert!(style.content.contains("color: red"));
}

#[test]
fn template_reparse_of_sfc_block_content() {
    let source = "<template>\n  <button @click=\"n++\">{{ n }}</button>\n</template>\n";
    let sfc = parse_sfc(source);
    let template = &sfc.blocks[0];
    let nodes = parse_template(template.content);
    let button = only_element(&nodes);
    assert_eq!(button.name, "button");
    let directive = button.attributes[0].directive.as_ref().unwrap();
    assert_eq!(directive.name, "on");
    assert_eq!(directive.argument.as_ref().unwrap().text, "click");
}

/// Sibling nodes must tile `[start, end)` with no gaps or overlaps, and
/// every element's children must tile its own interior — mirrors the
/// corpus-wide invariant checked in `examples/parse_corpus.rs`.
fn assert_contiguous(nodes: &[Node], start: u32, end: u32) {
    let mut cursor = start;
    for node in nodes {
        let span = node.span();
        assert_eq!(span.start, cursor, "gap or overlap before {span:?}");
        cursor = span.end;
        if let Node::Element(element) = node
            && let (Some(first), Some(last)) = (element.children.first(), element.children.last())
        {
            assert!(
                first.span().start >= span.start && last.span().end <= span.end,
                "child span escapes element {span:?}"
            );
            assert_contiguous(&element.children, first.span().start, last.span().end);
        }
    }
    assert_eq!(cursor, end, "tail gap: ended at {cursor}, expected {end}");
}

// Regression tests for PR1-C1 (CRITICAL): a stray closing tag that doesn't
// belong to any open element used to cascade `unclosed` all the way up
// through every real ancestor, truncating their content and re-parsing the
// remainder as fabricated top-level siblings. Recovery must instead check
// the *whole* open-ancestor stack: only a tag that actually matches
// something open may close anything; an unmatched tag is dropped in place
// (covered by a `Raw` node so span-tiling still holds) and parsing
// continues where it was.

#[test]
fn stray_closing_tag_does_not_truncate_sfc_block() {
    // `</br>` matches no open ancestor (`br` is void and was already closed
    // by its own tag) — it must not close `<template>`. Before the fix this
    // truncated the template block after `<br>` and fabricated a phantom
    // top-level `p` block from the remaining `<p>x</p>`.
    let source = "<template><br></br><p>x</p></template>";
    let sfc = parse_sfc(source);
    assert_eq!(sfc.blocks.len(), 1, "stray </br> must not fabricate extra top-level blocks");
    let template = &sfc.blocks[0];
    assert_eq!(template.name, "template");
    assert!(
        template.content.contains("<p>x</p>"),
        "template content must not be truncated at the stray tag: {:?}",
        template.content
    );

    let nodes = parse_template(template.content);
    assert_contiguous(&nodes, 0, u32::try_from(template.content.len()).unwrap());
}

#[test]
fn stray_closing_tag_is_dropped_without_closing_the_open_element() {
    // `</span>` was never opened anywhere in the stack (the only open
    // element is `div`, and `div` doesn't match `span` either) — exactly
    // the "matches no open ancestor" case. A browser drops such a tag
    // silently rather than closing `div`, so `<p>` still opens as a child
    // of the still-open `div` (there is no real `</div>` anywhere in this
    // source, so `div` is legitimately `unclosed` at EOF — same as it
    // would be with the stray tag removed entirely).
    let source = "<div></span><p>x</p>";
    let nodes = parse_template(source);
    let div = only_element(&nodes);
    assert_eq!(div.name, "div");
    assert!(div.unclosed, "no real </div> exists in the source");
    assert!(
        matches!(div.children.first(), Some(Node::Raw(_))),
        "the stray </span> must be covered by a Raw node, not dropped from the tree"
    );
    let p = div
        .children
        .iter()
        .find_map(|node| if let Node::Element(element) = node { Some(element) } else { None })
        .expect("<p> must be preserved, not lost or fabricated as a top-level sibling");
    assert_eq!(p.name, "p");
    assert!(!p.unclosed);

    assert_contiguous(&nodes, 0, u32::try_from(source.len()).unwrap());
}

#[test]
fn closing_tag_matching_outer_ancestor_still_cascades_unclosed() {
    // Regression guard: a tag that matches an *outer* ancestor (not the
    // innermost open element) must still close the current element as
    // `unclosed` and propagate — this is the pre-existing, intentional
    // behavior that makes `<div><p></div>` recover sensibly, and the fix
    // for the no-match case must not disturb it.
    let source = "<div><p></div><i>y</i>";
    let nodes = parse_template(source);
    let elements: Vec<_> = nodes
        .iter()
        .filter_map(|node| if let Node::Element(element) = node { Some(element) } else { None })
        .collect();
    assert_eq!(elements.len(), 2, "div and i are top-level siblings");

    let div = elements[0];
    assert_eq!(div.name, "div");
    assert!(!div.unclosed);
    let p = only_element(&div.children);
    assert_eq!(p.name, "p");
    assert!(p.unclosed, "</div> closed div, so p recovers as unclosed");

    let i = elements[1];
    assert_eq!(i.name, "i");
    assert!(!i.unclosed);

    assert_contiguous(&nodes, 0, u32::try_from(source.len()).unwrap());
}

#[test]
fn orphan_closing_tag_at_sfc_top_level_does_not_fabricate_a_block() {
    // A closing tag with no open ancestor at all (the top level) must be
    // dropped in place, exactly like the nested case — it must not be
    // mistaken for (or interfere with) real block boundaries.
    let source = "</template>\n<script>const a = 1;</script>\n";
    let sfc = parse_sfc(source);
    assert_eq!(sfc.blocks.len(), 1, "orphan </template> must not become a phantom block");
    assert_eq!(sfc.blocks[0].name, "script");

    let nodes = parse_template(source);
    assert_contiguous(&nodes, 0, u32::try_from(source.len()).unwrap());
}
