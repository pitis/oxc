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
