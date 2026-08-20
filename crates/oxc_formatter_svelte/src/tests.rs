use oxc_allocator::Allocator;

use crate::{format, options::SvelteFormatOptions};

fn run(source: &str) -> Result<String, String> {
    let allocator = Allocator::new();
    let formatted = format(&allocator, source, SvelteFormatOptions::default())
        .map_err(|error| error.to_string())?;
    formatted.print().map(oxc_formatter_core::Printed::into_code).map_err(|error| error.to_string())
}

/// S1 prints the component back byte for byte. Every later stage replaces a
/// piece of this with real printing, so this is the baseline the conformance
/// harness measures against.
#[test]
fn reprints_a_component_verbatim() {
    for source in [
        "<div>x</div>\n",
        "<script>\n\tlet a = 1;\n</script>\n\n<div class=\"a\">{a}</div>\n\n<style>\n\t.a { color: red }\n</style>\n",
        "{#if a}\n\t<p>y</p>\n{:else}\n\t<p>n</p>\n{/if}\n",
        "<div    class='a'   ></div>\n",
        "",
    ] {
        assert_eq!(run(source).as_deref(), Ok(source), "for {source:?}");
    }
}

/// A byte-order mark survives, and CRLF is normalized the way every other
/// formatter here normalizes it.
#[test]
fn handles_bom_and_crlf() {
    assert_eq!(run("\u{feff}<div>x</div>\n").as_deref(), Ok("\u{feff}<div>x</div>\n"));
    assert_eq!(run("<div>x</div>\r\n").as_deref(), Ok("<div>x</div>\n"));
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
