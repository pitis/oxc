use oxc_formatter_core::{FormatContext, SourceText};

use crate::options::SvelteFormatOptions;

/// Formatting context for a Svelte component.
pub struct SvelteFormatContext<'a> {
    options: SvelteFormatOptions,
    source_text: SourceText<'a>,
}

impl<'a> SvelteFormatContext<'a> {
    pub fn new(options: SvelteFormatOptions, source_code: &'a str) -> Self {
        Self { options, source_text: SourceText::new(source_code) }
    }

    /// The source text with the arena lifetime, so slices taken from it can
    /// go straight into `text(…)` without being copied.
    pub fn source_text(&self) -> SourceText<'a> {
        self.source_text
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
