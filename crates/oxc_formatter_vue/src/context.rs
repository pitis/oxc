use rustc_hash::FxHashSet;

use oxc_formatter_core::{FormatContext, SourceText};

use crate::options::VueFormatOptions;

/// The longest excerpt of a fragment a warning quotes. There is no span to
/// point at, so the text itself has to identify it, and a whole `<script>`
/// body would drown the message.
const SNIPPET_MAX_LENGTH: usize = 60;

/// Formatting context for a Vue single-file component.
pub struct VueFormatContext<'a> {
    options: VueFormatOptions,
    source_text: SourceText<'a>,
    /// Fragments that failed to parse, in the order they were met, each with
    /// the message that would report it.
    syntax_failures: Vec<(String, String)>,
    /// Fragments some attempt *did* format, which cancels any failure
    /// recorded for the same text.
    formatted_fragments: FxHashSet<String>,
}

impl<'a> VueFormatContext<'a> {
    pub fn new(options: VueFormatOptions, source_code: &'a str) -> Self {
        Self {
            options,
            source_text: SourceText::new(source_code),
            syntax_failures: Vec::new(),
            formatted_fragments: FxHashSet::default(),
        }
    }

    /// The source text with the arena lifetime, so slices taken from it can go
    /// straight into `text(…)` without being copied.
    pub fn source_text(&self) -> SourceText<'a> {
        self.source_text
    }

    /// Record that a fragment could not be formatted, so the user hears about
    /// a syntax error the printer would otherwise pass over in silence.
    ///
    /// `context` names the position in the vocabulary the Prettier-side
    /// channel already uses — `expression-attribute`, `event-handler`,
    /// `vue-script` — so both paths report the same way.
    pub fn report_fragment_failure(&mut self, source: &str, context: &str) {
        if self.syntax_failures.iter().any(|(text, _)| text == source) {
            return;
        }
        let message = format!(
            "syntax error in embedded script ({context} fragment left unformatted: `{}`)",
            snippet(source)
        );
        self.syntax_failures.push((source.to_string(), message));
    }

    /// Record that a fragment did format.
    ///
    /// This is what keeps `@click="a++; b++"` quiet: the handler printer tries
    /// the value as an expression first — which legitimately does not parse —
    /// and only then as statements. A failure some later attempt recovered
    /// from was never the user's problem.
    pub fn report_fragment_success(&mut self, source: &str) {
        if self.syntax_failures.iter().any(|(text, _)| text == source) {
            self.formatted_fragments.insert(source.to_string());
        }
    }

    /// The failures no attempt recovered from.
    pub fn warnings(&self) -> Vec<String> {
        self.syntax_failures
            .iter()
            .filter(|(text, _)| !self.formatted_fragments.contains(text))
            .map(|(_, message)| message.clone())
            .collect()
    }
}

/// A single-line, length-capped excerpt of a fragment.
fn snippet(source: &str) -> String {
    let single_line = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() > SNIPPET_MAX_LENGTH {
        let cut: String = single_line.chars().take(SNIPPET_MAX_LENGTH).collect();
        format!("{cut}...")
    } else {
        single_line
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
