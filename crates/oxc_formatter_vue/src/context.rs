use oxc_formatter_core::{FormatContext, SourceText, TailwindCollector};

use crate::options::VueFormatOptions;

/// Formatting context for a Vue single-file component.
pub struct VueFormatContext<'a> {
    options: VueFormatOptions,
    source_text: SourceText<'a>,
    /// Tailwind classes a dispatched child collected, in this document's
    /// index space.
    tailwind_classes: Vec<String>,
}

impl<'a> VueFormatContext<'a> {
    pub fn new(options: VueFormatOptions, source_code: &'a str) -> Self {
        Self { options, source_text: SourceText::new(source_code), tailwind_classes: Vec::new() }
    }

    /// The source text with the arena lifetime, so slices taken from it can go
    /// straight into `text(…)` without being copied.
    pub fn source_text(&self) -> SourceText<'a> {
        self.source_text
    }

    /// Take the collected classes for the document to sort.
    pub fn take_tailwind_classes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.tailwind_classes)
    }
}

/// Lets a dispatched child's classes remap into this document's index space.
impl TailwindCollector for VueFormatContext<'_> {
    fn add_class(&mut self, class: String) -> usize {
        self.tailwind_classes.push(class);
        self.tailwind_classes.len() - 1
    }
}

impl FormatContext for VueFormatContext<'_> {
    type Options = VueFormatOptions;

    fn options(&self) -> &Self::Options {
        &self.options
    }

    fn source_code(&self) -> &str {
        &self.source_text
    }
}
