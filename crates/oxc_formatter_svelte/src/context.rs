use oxc_formatter_core::{FormatContext, SourceText, TailwindCollector};

use crate::options::SvelteFormatOptions;

/// Formatting context for a Svelte component.
pub struct SvelteFormatContext<'a> {
    options: SvelteFormatOptions,
    source_text: SourceText<'a>,
    /// Tailwind classes a dispatched child collected, in this document's
    /// index space. The markup printer does not collect any of its own yet;
    /// the list exists so a child's can be remapped into it.
    tailwind_classes: Vec<String>,
}

impl<'a> SvelteFormatContext<'a> {
    pub fn new(options: SvelteFormatOptions, source_code: &'a str) -> Self {
        Self { options, source_text: SourceText::new(source_code), tailwind_classes: Vec::new() }
    }

    /// The source text with the arena lifetime, so slices taken from it can
    /// go straight into `text(…)` without being copied.
    pub fn source_text(&self) -> SourceText<'a> {
        self.source_text
    }
}

/// Lets a dispatched child's classes remap into this document's index space
/// (`DispatchPayload::into_doc`).
impl TailwindCollector for SvelteFormatContext<'_> {
    fn add_class(&mut self, class: String) -> usize {
        self.tailwind_classes.push(class);
        self.tailwind_classes.len() - 1
    }
}

impl SvelteFormatContext<'_> {
    /// Take the collected classes for the document to sort.
    pub fn take_tailwind_classes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.tailwind_classes)
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
