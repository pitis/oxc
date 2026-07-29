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
    let nodes = parse_template("<div class=\"a\">hi {{ name }}!</div>", 0);
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
        0,
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
    let nodes = parse_template("<br><Item :x=\"1\" /><img src=\"a\">", 0);
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
    // `pre` was removed from the raw-text set (see `pre_parses_children_normally`
    // below) — `textarea` is used here instead to exercise the same
    // byte-preservation behavior for genuine raw-text elements.
    let source = "<textarea>  <div> not parsed {{ x }}  </textarea>";
    let nodes = parse_template(source, 0);
    let textarea = only_element(&nodes);
    let raw = textarea.raw_text.expect("raw text");
    assert_eq!(&source[raw.start as usize..raw.end as usize], "  <div> not parsed {{ x }}  ");
    assert!(textarea.children.is_empty());
}

#[test]
fn pre_parses_children_normally() {
    // `<pre>` used to be treated as raw text, hiding every directive and
    // interpolation inside it from the AST. HTML parses full markup inside
    // `<pre>` (only whitespace *rendering* differs) and Vue compiles
    // directives inside it, so it must parse like any other element.
    let source = "<pre><code v-if=\"x\">{{ y }}</code></pre>";
    let nodes = parse_template(source, 0);
    let pre = only_element(&nodes);
    assert!(pre.raw_text.is_none());
    assert_eq!(pre.children.len(), 1);
    let Node::Element(code) = &pre.children[0] else {
        panic!("expected <code> element child");
    };
    assert_eq!(code.name, "code");
    let directive = code.attributes[0].directive.as_ref().expect("v-if directive");
    assert_eq!(directive.name, "if");
    assert_eq!(code.children.len(), 1);
    let Node::Interpolation(interpolation) = &code.children[0] else {
        panic!("expected interpolation child");
    };
    assert_eq!(interpolation.expression, " y ");
    assert!(!interpolation.unterminated);

    assert_contiguous(&nodes, 0, u32::try_from(source.len()).unwrap());
}

#[test]
fn nested_pre_does_not_produce_a_stray_top_level_raw() {
    // Regression test: when `pre` was raw-text, its body scan matched the
    // *first* `</pre>` it found regardless of nesting, so the inner closing
    // tag closed the outer element and the real outer `</pre>` was left
    // over as a stray top-level `Raw` node. With `pre` parsed like any
    // other element, the ancestor-aware closing-tag matching handles
    // nesting correctly and no such artifact appears.
    let source = "<pre><pre>x</pre></pre>";
    let nodes = parse_template(source, 0);
    assert_eq!(nodes.len(), 1, "no stray top-level Raw node");
    let outer = only_element(&nodes);
    assert_eq!(outer.name, "pre");
    assert!(!outer.unclosed);
    let inner = only_element(&outer.children);
    assert_eq!(inner.name, "pre");
    assert!(!inner.unclosed);
    assert_eq!(inner.children.len(), 1);
    assert!(matches!(&inner.children[0], Node::Text(text) if text.value == "x"));

    assert_contiguous(&nodes, 0, u32::try_from(source.len()).unwrap());
}

#[test]
fn unterminated_comment_is_flagged() {
    let source = "<!-- never closed";
    let nodes = parse_template(source, 0);
    assert_eq!(nodes.len(), 1);
    let Node::Comment(comment) = &nodes[0] else {
        panic!("expected comment");
    };
    assert!(comment.unterminated);
    assert_eq!(comment.content, " never closed");
}

#[test]
fn terminated_comment_is_not_flagged() {
    let source = "<!-- closed -->";
    let nodes = parse_template(source, 0);
    let Node::Comment(comment) = &nodes[0] else {
        panic!("expected comment");
    };
    assert!(!comment.unterminated);
    assert_eq!(comment.content, " closed ");
}

#[test]
fn unterminated_interpolation_is_flagged() {
    let source = "{{ never closed";
    let nodes = parse_template(source, 0);
    assert_eq!(nodes.len(), 1);
    let Node::Interpolation(interpolation) = &nodes[0] else {
        panic!("expected interpolation");
    };
    assert!(interpolation.unterminated);
    assert_eq!(interpolation.expression, " never closed");
}

#[test]
fn terminated_interpolation_is_not_flagged() {
    let source = "{{ closed }}";
    let nodes = parse_template(source, 0);
    let Node::Interpolation(interpolation) = &nodes[0] else {
        panic!("expected interpolation");
    };
    assert!(!interpolation.unterminated);
    assert_eq!(interpolation.expression, " closed ");
}

#[test]
fn unterminated_attribute_value_is_flagged() {
    let source = "<div class=\"never closed";
    let nodes = parse_template(source, 0);
    let div = only_element(&nodes);
    let value = div.attributes[0].value.as_ref().expect("attribute value");
    assert!(value.unterminated);
    assert_eq!(value.text, "never closed");
}

#[test]
fn terminated_attribute_value_is_not_flagged() {
    let source = "<div class=\"closed\">";
    let nodes = parse_template(source, 0);
    let div = only_element(&nodes);
    let value = div.attributes[0].value.as_ref().expect("attribute value");
    assert!(!value.unterminated);
    assert_eq!(value.text, "closed");
}

#[test]
fn recovers_from_unclosed_elements() {
    let nodes = parse_template("<ul><li>a<li>b</ul>", 0);
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
    let nodes = parse_template("<!-- note --><!DOCTYPE html><span>x</span>", 0);
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
fn sfc_collects_only_top_level_comments() {
    // Only the comments outside every block: a linter reads these as
    // file-scoped directive comments, while the ones inside `<template>` are
    // reached through that block's own parse.
    let source = "<!-- first -->\n<template>\n  <!-- inside -->\n  <div/>\n</template>\n<!-- second -->\n<script>/* not a comment node */</script>\n<!-- third -->\n";
    let sfc = parse_sfc(source);
    let comments: Vec<&str> =
        sfc.top_level_comments.iter().map(|comment| comment.content.trim()).collect();
    assert_eq!(comments, ["first", "second", "third"]);
    // Spans are file offsets, and in source order.
    assert_eq!(sfc.top_level_comments[0].span.start, 0);
    assert!(
        sfc.top_level_comments.windows(2).all(|pair| pair[0].span.start < pair[1].span.start),
        "top-level comments must be in source order"
    );
}

#[test]
fn template_reparse_of_sfc_block_content() {
    let source = "<template>\n  <button @click=\"n++\">{{ n }}</button>\n</template>\n";
    let sfc = parse_sfc(source);
    let template = &sfc.blocks[0];
    let nodes = parse_template(template.content, 0);
    let button = only_element(&nodes);
    assert_eq!(button.name, "button");
    let directive = button.attributes[0].directive.as_ref().unwrap();
    assert_eq!(directive.name, "on");
    assert_eq!(directive.argument.as_ref().unwrap().text, "click");
}

#[test]
fn template_reparse_with_base_offset_is_file_relative() {
    // A downstream consumer typically extracts `SfcBlock::content` and
    // re-parses it independently (e.g. to build a fresh tree without
    // re-splitting the whole file). Passing `content_span.start` as
    // `base_offset` must make every span in that independent parse line up
    // with offsets into the *original* file, not into the substring.
    let source = "<template>\n  <button @click=\"n++\">{{ n }}</button>\n</template>\n";
    let sfc = parse_sfc(source);
    let template = &sfc.blocks[0];
    let nodes = parse_template(template.content, template.content_span.start);
    let button = only_element(&nodes);
    assert_eq!(button.name, "button");
    let directive = button.attributes[0].directive.as_ref().unwrap();
    let argument = directive.argument.as_ref().unwrap();

    // Slicing the *original* `source` (not `template.content`) at these
    // spans must recover the expected text: proof the spans are
    // file-relative, not content-relative.
    assert_eq!(
        &source[button.span.start as usize..button.span.end as usize],
        "<button @click=\"n++\">{{ n }}</button>"
    );
    assert_eq!(&source[argument.span.start as usize..argument.span.end as usize], "click");
}

#[test]
fn base_offset_zero_vs_n_shifts_every_span_by_exactly_n() {
    // Walk the whole tree for two parses of the same source differing only
    // in `base_offset`, and check every span (including nested attribute,
    // directive-argument, and child spans) differs by exactly `n`.
    const N: u32 = 1000;
    let source =
        "<div v-if=\"a\"><span :key=\"k\">{{ x }}</span><!-- c --><textarea>x</textarea></div>tail";
    let zero = parse_template(source, 0);
    let shifted = parse_template(source, N);
    assert_spans_shifted_by(&zero, &shifted, N);
}

fn assert_spans_shifted_by(zero: &[Node], shifted: &[Node], n: u32) {
    assert_eq!(zero.len(), shifted.len());
    for (a, b) in zero.iter().zip(shifted.iter()) {
        assert_eq!(b.span().start, a.span().start + n);
        assert_eq!(b.span().end, a.span().end + n);
        match (a, b) {
            (Node::Element(a), Node::Element(b)) => {
                assert_eq!(b.name_span.start, a.name_span.start + n);
                assert_eq!(b.name_span.end, a.name_span.end + n);
                assert_eq!(b.open_tag_end, a.open_tag_end + n);
                assert_eq!(a.raw_text.is_some(), b.raw_text.is_some());
                if let (Some(a_raw), Some(b_raw)) = (a.raw_text, b.raw_text) {
                    assert_eq!(b_raw.start, a_raw.start + n);
                    assert_eq!(b_raw.end, a_raw.end + n);
                }
                assert_eq!(a.attributes.len(), b.attributes.len());
                for (a_attr, b_attr) in a.attributes.iter().zip(b.attributes.iter()) {
                    assert_eq!(b_attr.span.start, a_attr.span.start + n);
                    assert_eq!(b_attr.span.end, a_attr.span.end + n);
                    assert_eq!(b_attr.name_span.start, a_attr.name_span.start + n);
                    assert_eq!(b_attr.name_span.end, a_attr.name_span.end + n);
                    if let (Some(a_value), Some(b_value)) = (&a_attr.value, &b_attr.value) {
                        assert_eq!(b_value.span.start, a_value.span.start + n);
                        assert_eq!(b_value.span.end, a_value.span.end + n);
                    }
                    if let (Some(a_dir), Some(b_dir)) = (&a_attr.directive, &b_attr.directive)
                        && let (Some(a_arg), Some(b_arg)) = (&a_dir.argument, &b_dir.argument)
                    {
                        assert_eq!(b_arg.span.start, a_arg.span.start + n);
                        assert_eq!(b_arg.span.end, a_arg.span.end + n);
                    }
                }
                assert_spans_shifted_by(&a.children, &b.children, n);
            }
            (Node::Comment(a), Node::Comment(b)) => {
                assert_eq!(b.content_span.start, a.content_span.start + n);
                assert_eq!(b.content_span.end, a.content_span.end + n);
            }
            (Node::Interpolation(a), Node::Interpolation(b)) => {
                assert_eq!(b.expression_span.start, a.expression_span.start + n);
                assert_eq!(b.expression_span.end, a.expression_span.end + n);
            }
            (Node::Text(_), Node::Text(_)) | (Node::Raw(_), Node::Raw(_)) => {}
            _ => panic!("node kind mismatch between base_offset 0 and {n} parses"),
        }
    }
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

    let nodes = parse_template(template.content, 0);
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
    let nodes = parse_template(source, 0);
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
    let nodes = parse_template(source, 0);
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
fn empty_block_content_span_points_right_after_the_open_tag() {
    // Before the fix, an empty block's `content_span` fell back to
    // `element.span.end` (past the closing tag entirely), so a consumer
    // splicing formatted content at that offset would write after
    // `</template>` instead of between the tags.
    let source = "<template></template>";
    let sfc = parse_sfc(source);
    let template = &sfc.blocks[0];
    assert_eq!(template.content_span.start, 10, "right after <template>'s `>`");
    assert_eq!(template.content_span.end, 10);
    assert_eq!(template.content, "");
}

#[test]
fn unterminated_script_block_is_flagged_unclosed() {
    let source = "<script>const a = 1;";
    let sfc = parse_sfc(source);
    assert_eq!(sfc.blocks.len(), 1);
    assert!(sfc.blocks[0].unclosed, "unterminated <script> must be flagged unclosed");
}

#[test]
fn closed_block_is_not_flagged_unclosed() {
    let source = "<script>const a = 1;</script>";
    let sfc = parse_sfc(source);
    assert!(!sfc.blocks[0].unclosed);
}

#[test]
fn stray_top_level_text_between_blocks_becomes_an_orphan_span() {
    let source = "<template></template>stray text<script></script>";
    let sfc = parse_sfc(source);
    assert_eq!(sfc.blocks.len(), 2);
    assert_eq!(sfc.orphan_spans.len(), 1, "exactly one orphan span for the stray text");
    let span = sfc.orphan_spans[0];
    assert_eq!(&source[span.start as usize..span.end as usize], "stray text");
}

#[test]
fn whitespace_only_text_between_blocks_is_not_an_orphan() {
    let source = "<template></template>\n\n  \n<script></script>";
    let sfc = parse_sfc(source);
    assert_eq!(sfc.blocks.len(), 2);
    assert!(
        sfc.orphan_spans.is_empty(),
        "whitespace-only top-level text must not be reported as orphan content: {:?}",
        sfc.orphan_spans
    );
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

    let nodes = parse_template(source, 0);
    assert_contiguous(&nodes, 0, u32::try_from(source.len()).unwrap());
}
