#![expect(clippy::print_stdout)]
//! Print a `.svelte` component's IR and its formatted output.
//!
//! The markup layer only: this runs on a service-less session, so a
//! `<script>`, a `<style>` and a `{…}` stay exactly as written. That is what
//! makes it useful for a layout question — the document is the markup
//! printer's own decisions and nothing else.
//!
//! ```bash
//! cargo run -p oxc_formatter_svelte --example ir -- [--print-width 80] file.svelte
//! ```

use std::{fs, path::Path};

use oxc_allocator::Allocator;
use oxc_formatter_core::LineWidth;
use oxc_formatter_svelte::{SvelteFormatOptions, format};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let print_width = args
        .iter()
        .position(|arg| arg == "--print-width")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<u16>().ok());
    let name = args
        .iter()
        .rfind(|arg| !arg.starts_with("--") && arg.parse::<u16>().is_err())
        .cloned()
        .unwrap_or_else(|| "test.svelte".to_string());

    let source_text =
        fs::read_to_string(Path::new(&name)).map_err(|_| format!("Missing '{name}'"))?;

    let mut options = SvelteFormatOptions::default();
    if let Some(print_width) = print_width {
        options.line_width = LineWidth::try_from(print_width).map_err(|error| error.to_string())?;
    }

    let allocator = Allocator::default();
    let formatted =
        format(&allocator, &source_text, options).map_err(|diagnostic| diagnostic.to_string())?;

    println!("--- IR ---");
    println!("{}", formatted.document().display(&source_text));
    println!("--- Output ---");
    println!("{}", formatted.print().map_err(|error| error.to_string())?.into_code());

    Ok(())
}
