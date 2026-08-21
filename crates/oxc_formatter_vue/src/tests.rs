//! Every expectation here is Prettier 3.9.6's own output for the same input,
//! captured with `prettier.format(source, { parser: "vue" })`. They are not
//! judgements about what the layout should be — they are a record of what it
//! is, so a change that drifts from Prettier fails here rather than in a
//! project's diff.

use oxc_allocator::Allocator;

use crate::{format, options::VueFormatOptions};

/// Format without a session, so `<script>` and `<style>` bodies stay as
/// written. The cases below are about markup; the embedded languages are
/// covered by the oxfmt integration, which installs a dispatcher.
fn print(source: &str) -> String {
    let allocator = Allocator::new();
    // Prettier's default print width is 80 and oxfmt's is 100. The
    // expectations here come from Prettier's defaults, so the width has to as
    // well — comparing a 100-column layout against an 80-column oracle
    // reports differences that are only the setting.
    let options = VueFormatOptions {
        line_width: 80.try_into().expect("80 is a valid width"),
        ..VueFormatOptions::default()
    };
    format(&allocator, source, options)
        .expect("well-formed")
        .print()
        .expect("printable")
        .into_code()
}

#[track_caller]
fn check(source: &str, expected: &str) {
    assert_eq!(print(source), expected);
}

// ---------------------------------------------------------------------------
// The component's blocks
// ---------------------------------------------------------------------------

#[test]
fn blocks_touch_when_the_source_had_no_blank_line() {
    check(
        "<template><div /></template>\n<style></style>\n",
        "<template><div /></template>\n<style></style>\n",
    );
}

#[test]
fn a_blank_line_between_blocks_is_kept() {
    check(
        "<template><div /></template>\n\n<style></style>\n",
        "<template><div /></template>\n\n<style></style>\n",
    );
}

#[test]
fn several_blank_lines_collapse_to_one() {
    check(
        "<template><div /></template>\n\n\n\n<style></style>\n",
        "<template><div /></template>\n\n<style></style>\n",
    );
}

#[test]
fn block_order_is_preserved() {
    check(
        "<style></style>\n<template><div /></template>\n",
        "<style></style>\n<template><div /></template>\n",
    );
}

#[test]
fn an_empty_block_keeps_its_tags_together() {
    check("<template></template>\n", "<template></template>\n");
    check("<template>\n\n</template>\n", "<template></template>\n");
}

#[test]
fn a_custom_block_is_kept_verbatim() {
    let source = "<i18n>\n  { \"en\": {} }\n</i18n>\n";
    check(source, source);
}

/// A component's blocks hold other languages, so `</` inside one is whatever
/// that language says it is. Reading it as markup would leave an element open
/// and get the whole file refused.
#[test]
fn a_block_body_is_not_read_as_markup() {
    let source = "<custom lang=\"unknown\">\nconst foo = \"</\";\n</custom>\n";
    check(source, source);

    // `<template>` is the exception, and stops being one when it declares a
    // language of its own.
    let pug = "<template lang=\"pug\">\n  .test\n    #foo\n</template>\n";
    check(pug, pug);
}

#[test]
fn a_component_with_no_closing_tag_is_refused() {
    let allocator = Allocator::new();
    let result = format(&allocator, "<template><div />", VueFormatOptions::default());
    assert!(result.is_err(), "an unclosed block must not be reformatted from a guess");
}

#[test]
fn a_file_ends_with_exactly_one_newline() {
    check("<style></style>", "<style></style>\n");
    check("<style></style>\n\n\n", "<style></style>\n");
}

// ---------------------------------------------------------------------------
// Markup
// ---------------------------------------------------------------------------

#[test]
fn attribute_values_are_normalized_to_double_quotes() {
    check("<template><div class='a' /></template>\n", "<template><div class=\"a\" /></template>\n");
}

#[test]
fn a_void_element_is_written_self_closing() {
    check(
        "<template>\n  <img src=\"a.png\">\n</template>\n",
        "<template>\n  <img src=\"a.png\" />\n</template>\n",
    );
    check("<template>\n  <MyThing/>\n</template>\n", "<template>\n  <MyThing />\n</template>\n");
}

#[test]
fn an_inline_element_keeps_its_text_between_its_tags() {
    let source = "<template>\n  <span>hello</span>\n</template>\n";
    check(source, source);
}

#[test]
fn a_nested_template_is_markup_not_a_block() {
    let source = "<template>\n  <template #default><span>a</span></template>\n</template>\n";
    check(source, source);
}

#[test]
fn attributes_break_one_per_line_when_the_tag_is_too_long() {
    check(
        "<template>\n  <div class=\"aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeeeeeee ffffffffff\" id=\"x\" />\n</template>\n",
        "<template>\n  <div\n    class=\"aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeeeeeee ffffffffff\"\n    id=\"x\"\n  />\n</template>\n",
    );
}

#[test]
fn text_is_filled_to_the_line_width() {
    check(
        "<template>\n  <p>one two three four five six seven eight nine ten eleven twelve thirteen fourteen</p>\n</template>\n",
        "<template>\n  <p>\n    one two three four five six seven eight nine ten eleven twelve thirteen\n    fourteen\n  </p>\n</template>\n",
    );
}

/// The closing `</a` has nowhere to break before it — there is no whitespace
/// between `</a>` and the `.` — so the `>` moves onto the next line instead.
/// Nothing about what the page renders changes.
#[test]
fn a_tag_delimiter_moves_when_there_is_no_space_to_break_at() {
    check(
        "<template>\n  <p>Write us<a href=\"/x\"><b>here</b></a>.</p>\n</template>\n",
        "<template>\n  <p>\n    Write us<a href=\"/x\"><b>here</b></a\n    >.\n  </p>\n</template>\n",
    );
}

#[test]
fn a_list_puts_every_item_on_its_own_line() {
    check(
        "<template>\n  <ul><li>a</li><li>b</li></ul>\n</template>\n",
        "<template>\n  <ul>\n    <li>a</li>\n    <li>b</li>\n  </ul>\n</template>\n",
    );
}

#[test]
fn a_comment_keeps_its_spelling() {
    let source = "<template>\n  <!-- a note -->\n  <div />\n</template>\n";
    check(source, source);
}

#[test]
fn pre_content_is_kept_exactly() {
    let source = "<template>\n  <pre>\n  a\n   b\n</pre>\n</template>\n";
    check(source, source);
}

/// With no formatter to hand the expression to, an interpolation is kept
/// exactly as written rather than half-tidied — the spacing is the author's
/// until something can actually parse it. `{{x}}` becoming `{{ x }}` is the
/// formatted path, covered end to end in `oxfmt`.
#[test]
fn an_interpolation_is_kept_as_written_with_no_formatter_for_it() {
    let source = "<template>\n  <div>{{x}}</div>\n</template>\n";
    check(source, source);
}

/// A `"` inside a value would close the attribute early, so it is written as
/// the entity — otherwise the output is not markup any more. `class` is
/// written straight through and needs the same protection as a formatted
/// value.
#[test]
fn a_quote_in_an_attribute_value_is_written_as_an_entity() {
    check(
        "<template><div class=\"a&quot;b\" /></template>\n",
        "<template><div class=\"a&quot;b\" /></template>\n",
    );
    check(
        "<template><div class=\"a  b&quot;c\" /></template>\n",
        "<template><div class=\"a b&quot;c\" /></template>\n",
    );
}

/// Inside an `<svg>` the HTML stylesheet does not apply — SVG lays its own
/// elements out as blocks — so `<circle>` gets a line of its own where a
/// `<span>` in the same position would not.
#[test]
fn svg_children_are_laid_out_as_blocks() {
    check(
        "<template>\n  <span class=\"a\"><svg class=\"size-2\" viewBox=\"0 0 8 8\"><circle cx=\"4\" cy=\"4\" r=\"3\" /></svg></span>\n</template>\n",
        "<template>\n  <span class=\"a\"\n    ><svg class=\"size-2\" viewBox=\"0 0 8 8\"><circle cx=\"4\" cy=\"4\" r=\"3\" /></svg\n  ></span>\n</template>\n",
    );
}

/// An empty `<textarea>` has nothing between its tags, not whitespace: the
/// difference decides whether the tag may stay on one line.
#[test]
fn an_empty_textarea_keeps_its_tags_together() {
    let source = "<template>\n  <textarea class=\"input w-full\" rows=\"2\" v-model=\"x\"></textarea>\n</template>\n";
    check(source, source);
}

#[test]
fn a_blank_line_between_list_items_is_kept() {
    let source = "<template>\n  <ul>\n    <li>a</li>\n\n    <li>b</li>\n  </ul>\n</template>\n";
    check(source, source);
}

#[test]
fn a_class_list_has_its_whitespace_collapsed() {
    check(
        "<template>\n  <div class=\"  a   b\n  c \" />\n</template>\n",
        "<template>\n  <div class=\"a b c\" />\n</template>\n",
    );
}

/// Without a dispatcher there is nothing to format an expression with, so the
/// value is kept exactly as written rather than half-processed. The formatted
/// path is covered end to end in `oxfmt`, which installs one.
#[test]
fn an_expression_value_is_kept_as_written_with_no_formatter_for_it() {
    let source = "<template>\n  <div :title=\"a?b:c\" />\n</template>\n";
    check(source, source);
}
