// A `.a{color:red}` in an expectation is CSS, not a format argument.
#![expect(clippy::literal_string_with_formatting_args)]

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
    // Everything under a `<pre>` shows its own whitespace, not only its text.
    check("<pre>a <b>  c  </b></pre>\n", "<pre>a <b>  c  </b></pre>\n");
    // The tag itself is markup, and is normalized like any other.
    check("<textarea   readonly={readonly}></textarea>\n", "<textarea {readonly}></textarea>\n");
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

/// A quoted value keeps a `"` of its own by changing the quotes around it.
///
/// Known divergence: `prettier-plugin-svelte` always writes `"`, so it turns
/// `prop='"'` into `prop="""` — markup that no longer parses.
#[test]
fn keeps_a_value_that_is_quoted() {
    check("<Child prop='\"' />\n", "<Child prop='\"' />\n");
    check("<span title='\"a\"'>x</span>\n", "<span title='\"a\"'>x</span>\n");
    check("<span title='a'>x</span>\n", "<span title=\"a\">x</span>\n");
}

/// Directives drop a value that only repeats their own name.
#[test]
fn shortens_directives() {
    check("<input bind:value={value} />\n", "<input bind:value />\n");
    check("<div class:active={active}></div>\n", "<div class:active></div>\n");
    check("<div class:active={other}></div>\n", "<div class:active={other}></div>\n");
}

/// `format` builds a session with no services, so there is nothing to hand a
/// `<script>` or `<style>` body to and it stays exactly as written. The
/// embedded path — where oxfmt installs a dispatcher and the body reaches
/// `oxc_formatter` / `oxc_formatter_css` — is measured against
/// `prettier-plugin-svelte` through oxfmt itself.
#[test]
fn keeps_embedded_bodies_when_nothing_can_format_them() {
    check("<script>\n\tlet   a=1\n</script>\n", "<script>\n\tlet   a=1\n</script>\n");
    check("<style>\n\t.a{color:red}\n</style>\n", "<style>\n\t.a{color:red}\n</style>\n");
    // A language nothing here formats is never touched either.
    check(
        "<style lang=\"stylus\">\n\t.a\n\t\tcolor red\n</style>\n",
        "<style lang=\"stylus\">\n\t.a\n\t\tcolor red\n</style>\n",
    );
}

/// A block is control flow: it always breaks, and its branches are indented.
#[test]
fn lays_blocks_out_over_lines() {
    check("{#if a}x{/if}\n", "{#if a}x{/if}\n");
    check("{#if a}\n\tx\n{/if}\n", "{#if a}\n\tx\n{/if}\n");
    check(
        "{#if a}\n\tx\n{:else if b}\n\ty\n{:else}\n\tz\n{/if}\n",
        "{#if a}\n\tx\n{:else if b}\n\ty\n{:else}\n\tz\n{/if}\n",
    );
    // A branch that is nothing but a blank line keeps it: two breaks with
    // nothing between them are one blank line, not one break.
    check("{#snippet t()}\n\t\n{/snippet}\n", "{#snippet t()}\n\n{/snippet}\n");
}

/// A block header keeps the spelling of what is not an expression — the `as`
/// pattern and the index name — and normalizes the rest.
#[test]
fn writes_block_headers() {
    check("{#each  items  as item}{item}{/each}\n", "{#each items as item}{item}{/each}\n");
    check(
        "{#each items as item, i (item.id)}{item}{/each}\n",
        "{#each items as item, i (item.id)}{item}{/each}\n",
    );
    check("{#each items}x{:else}none{/each}\n", "{#each items}x{:else}none{/each}\n");
    check("{#key  value }x{/key}\n", "{#key value}x{/key}\n");
    // Nothing to show while pending collapses into the one-line form.
    check("{#await p}{:then v}ok{/await}\n", "{#await p then v}ok{/await}\n");
    check("{#await p}…{:then v}ok{/await}\n", "{#await p}…{:then v}ok{/await}\n");
    check("{#await p}{:catch e}bad{/await}\n", "{#await p catch e}bad{/await}\n");
}

/// The `{@…}` tags, and `{const …}`, which declares a binding rather than
/// interpolating one.
#[test]
fn writes_tags() {
    check("{@html  content }\n", "{@html content}\n");
    check("{@render  row(x) }\n", "{@render row(x)}\n");
    check("{@debug}\n", "{@debug}\n");
    check("<div>{@const  n = 1 }</div>\n", "<div>{@const n = 1}</div>\n");
    check("<div>{const  n = 1 }</div>\n", "<div>{const n = 1}</div>\n");
    // `{constant}` is an expression, not a declaration.
    check("<div>{constant}</div>\n", "<div>{constant}</div>\n");
}

/// `{@attach …}` reads as a shorthand attribute in the AST; the `@` is what
/// tells them apart.
#[test]
fn writes_an_attachment_attribute() {
    check("<div {@attach  thing }></div>\n", "<div {@attach thing}></div>\n");
    check("<div {value}></div>\n", "<div {value}></div>\n");
}

/// With no dispatcher there is nothing to format an expression with, so its
/// text is kept — only the padding inside the braces goes, which is not part
/// of what the expression means. The formatted path is measured against
/// `prettier-plugin-svelte` through oxfmt.
#[test]
fn keeps_expressions_when_nothing_can_format_them() {
    check("<div>{  count  }</div>\n", "<div>{count}</div>\n");
    check("<div foo={  bar  }></div>\n", "<div foo={bar}></div>\n");
    check("<div {...props}></div>\n", "<div {...props}></div>\n");
    // An unterminated `{` is left exactly as written; the refusal check has
    // already run, so this only guards the printer itself.
    check("<div>{a}</div>\n", "<div>{a}</div>\n");
}
