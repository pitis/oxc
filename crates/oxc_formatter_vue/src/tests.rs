use oxc_allocator::Allocator;

use crate::{format, options::VueFormatOptions};

/// Format without a session, so `<script>` and `<style>` bodies stay as
/// written — these cases are about the SFC skeleton, not the embedded
/// languages, which the oxfmt integration covers.
fn print(source: &str) -> String {
    let allocator = Allocator::new();
    format(&allocator, source, VueFormatOptions::default())
        .expect("well-formed")
        .print()
        .expect("printable")
        .into_code()
}

#[test]
fn blocks_are_separated_by_one_blank_line() {
    let out = print("<template><div /></template>\n<script>a</script>\n");
    assert_eq!(out, "<template><div /></template>\n\n<script>a</script>\n");
}

#[test]
fn extra_blank_lines_between_blocks_collapse_to_one() {
    let out = print("<template><div /></template>\n\n\n\n<script>a</script>\n");
    assert_eq!(out, "<template><div /></template>\n\n<script>a</script>\n");
}

#[test]
fn block_order_is_preserved() {
    let out = print("<script>a</script>\n<template><div /></template>\n");
    assert_eq!(out, "<script>a</script>\n\n<template><div /></template>\n");
}

#[test]
fn attribute_values_are_normalized_to_double_quotes() {
    let out = print("<script setup lang='ts'>a</script>\n");
    assert_eq!(out, "<script setup lang=\"ts\">a</script>\n");
}

#[test]
fn an_empty_block_keeps_its_tags_together() {
    assert_eq!(print("<template></template>\n"), "<template></template>\n");
}

#[test]
fn a_whitespace_only_block_keeps_one_line_break() {
    assert_eq!(print("<template>\n\n</template>\n"), "<template>\n</template>\n");
}

#[test]
fn a_custom_block_is_kept_verbatim() {
    let source = "<i18n>\n  { \"en\": {} }\n</i18n>\n";
    assert_eq!(print(source), source);
}

#[test]
fn a_component_with_no_closing_tag_is_refused() {
    let allocator = Allocator::new();
    let result = format(&allocator, "<template><div />", VueFormatOptions::default());
    assert!(result.is_err(), "an unclosed block must not be reformatted from a guess");
}

#[test]
fn a_file_ends_with_exactly_one_newline() {
    assert_eq!(print("<script>a</script>"), "<script>a</script>\n");
    assert_eq!(print("<script>a</script>\n\n\n"), "<script>a</script>\n");
}
