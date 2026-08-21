use std::cell::Cell;

use oxc_formatter_core::{FormatContext, SourceText};

use crate::options::SvelteFormatOptions;

/// Formatting context for a Svelte component.
pub struct SvelteFormatContext<'a> {
    options: SvelteFormatOptions,
    source_text: SourceText<'a>,
    in_pre: Cell<bool>,
}

impl<'a> SvelteFormatContext<'a> {
    pub fn new(options: SvelteFormatOptions, source_code: &'a str) -> Self {
        Self { options, source_text: SourceText::new(source_code), in_pre: Cell::new(false) }
    }

    /// The source text with the arena lifetime, so slices taken from it can
    /// go straight into `text(…)` without being copied.
    pub fn source_text(&self) -> SourceText<'a> {
        self.source_text
    }
}

impl SvelteFormatContext<'_> {
    /// Whether a `<pre>` or `<textarea>` encloses what is being printed, at
    /// any depth. Text renders as written there, so it is never laid out.
    ///
    /// A flag rather than an argument because the question is about
    /// ancestors, and what sits between an ancestor and a text node is not
    /// always an element: a `{#if}` inside a `<pre>` has branches of its own,
    /// and their text is still inside the `<pre>`. Prettier asks it the same
    /// way, by walking up the path (`isPreTagContent`).
    pub fn is_in_pre(&self) -> bool {
        self.in_pre.get()
    }

    /// Sets the flag, returning what it was for the caller to put back.
    pub fn set_in_pre(&self, yes: bool) -> bool {
        self.in_pre.replace(yes)
    }
}

impl FormatContext for SvelteFormatContext<'_> {
    type Options = SvelteFormatOptions;

    fn options(&self) -> &Self::Options {
        &self.options
    }

    fn source_code(&self) -> &str {
        &self.source_text
    }
}
