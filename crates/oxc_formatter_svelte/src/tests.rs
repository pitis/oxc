use oxc_allocator::Allocator;

use oxc_formatter_core::IndentStyle;

use crate::{format, options::SvelteFormatOptions};

/// Every expectation below is written with tabs, so the tests read as the
/// files they describe.
fn options() -> SvelteFormatOptions {
    SvelteFormatOptions { indent_style: IndentStyle::Tab, ..SvelteFormatOptions::default() }
}

fn run(source: &str) -> Result<String, String> {
    let allocator = Allocator::new();
    let formatted = format(&allocator, source, options()).map_err(|error| error.to_string())?;
    formatted.print().map(oxc_formatter_core::Printed::into_code).map_err(|error| error.to_string())
}

fn check(source: &str, expected: &str) {
    assert_eq!(run(source).as_deref(), Ok(expected), "for {source:?}");
}

#[test]
fn normalizes_tags_and_attributes() {
    check("<div    class='a'   ></div>\n", "<div class=\"a\"></div>\n");
    // Every self-closing tag is spelled `<x />`, a void element included.
    check("<br>\n", "<br />\n");
    check("<input disabled readonly>\n", "<input disabled readonly />\n");
    // `name={name}` is the shorthand.
    check("<Widget foo={foo} />\n", "<Widget {foo} />\n");
}

#[test]
fn lays_out_blocks_and_inlines_differently() {
    // A block element breaks onto its own line; the whitespace around it is
    // not rendered, so a break costs nothing.
    check("<div>\n<p>one</p>\n<p>two</p>\n</div>\n", "<div>\n\t<p>one</p>\n\t<p>two</p>\n</div>\n");
    // Between inline elements the whitespace *is* rendered, so a break has
    // to carry it — and none is invented where there was none.
    check("<span>a</span> <span>b</span>\n", "<span>a</span> <span>b</span>\n");
    check("<span>a</span><span>b</span>\n", "<span>a</span><span>b</span>\n");
}

#[test]
fn collapses_whitespace_but_keeps_one_blank_line() {
    check("<p>a     b</p>\n", "<p>a b</p>\n");
    check("<div>a</div>\n\n\n\n<div>b</div>\n", "<div>a</div>\n\n<div>b</div>\n");
}

#[test]
fn wraps_a_long_attribute_list() {
    let source = "<div id=\"a\" class=\"b\" role=\"c\" tabindex=\"0\" data-one=\"1\" data-two=\"2\" data-three=\"3\" aria-label=\"x\"></div>\n";
    let expected = "<div\n\tid=\"a\"\n\tclass=\"b\"\n\trole=\"c\"\n\ttabindex=\"0\"\n\tdata-one=\"1\"\n\tdata-two=\"2\"\n\tdata-three=\"3\"\n\taria-label=\"x\"\n></div>\n";
    check(source, expected);
}

#[test]
fn orders_the_top_level_sections() {
    check(
        "<div>markup</div>\n\n<script>\n\tlet a = 1;\n</script>\n",
        "<script>\n\tlet a = 1;\n</script>\n\n<div>markup</div>\n",
    );
    // `<script module>` comes before the instance script however it was
    // written, and a blank line separates them.
    check(
        "<script>\n\tlet a = 1;\n</script>\n<script module>\n\texport const K = 1;\n</script>\n",
        "<script module>\n\texport const K = 1;\n</script>\n\n<script>\n\tlet a = 1;\n</script>\n",
    );
}

/// `<pre>` and `<textarea>` render their own whitespace, and `<script>` and
/// `<style>` are other languages: all four keep their bodies byte for byte.
#[test]
fn keeps_content_that_must_not_be_reflowed() {
    check("<pre>\n  keep   this\n</pre>\n", "<pre>\n  keep   this\n</pre>\n");
    check("<textarea>\n  a   b\n</textarea>\n", "<textarea>\n  a   b\n</textarea>\n");
}

#[test]
fn handles_bom_and_crlf() {
    check("\u{feff}<div>x</div>\n", "\u{feff}<div>x</div>\n");
    check("<div>x</div>\r\n", "<div>x</div>\n");
}

/// Markup the Svelte compiler would reject is refused, not rewritten: the
/// tree for it is the parser's guess, and printing a guess would change what
/// the component means.
#[test]
fn refuses_markup_that_is_not_well_formed() {
    for source in ["<div>a", "<div>a</span></div>", "{#if a}x", "<!-- x", "{ a"] {
        let error = run(source).expect_err(&format!("should refuse {source:?}"));
        assert!(error.contains("not well-formed"), "for {source:?}: {error}");
    }
}

/// Known divergence from `prettier-plugin-svelte`, kept here so a change in
/// it is noticed: Prettier writes ` text` after a block element, keeping the
/// space that begins the text run; this printer drops a space that would
/// land at the start of a line, because it has no end-of-line trimming pass
/// to take it away again. The rendered HTML is the same either way.
#[test]
fn drops_a_space_that_would_start_a_line() {
    check("<div>text <p>block</p> text</div>\n", "<div>\n\ttext <p>block</p>\n\ttext\n</div>\n");
}
