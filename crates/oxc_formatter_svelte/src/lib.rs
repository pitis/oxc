//! Svelte component formatter built on top of `oxc_formatter_core`.
//!
//! Parses with [`svelte_markup_parser`] and prints the component's markup,
//! handing each embedded language to the formatter that owns it: `<script>`
//! to `oxc_formatter`, `<style>` to `oxc_formatter_css`, and every `{…}` to
//! `oxc_formatter`'s expression entry.
//!
//! A component whose markup is not well-formed is refused rather than
//! rewritten: the markup parser recovers from anything, so the tree for such
//! a file is a guess, and reformatting a guess changes what the file means.
//!
//! ```ignore
//! use oxc_allocator::Allocator;
//! use oxc_formatter_svelte::{SvelteFormatOptions, format};
//!
//! let allocator = Allocator::new();
//! let formatted = format(&allocator, "<div>x</div>\n", SvelteFormatOptions::default()).unwrap();
//! let out = formatted.print().unwrap().into_code();
//! assert_eq!(out, "<div>x</div>\n");
//! ```

mod context;
mod format;
mod options;
pub(crate) mod print;

#[cfg(test)]
mod tests;

pub use crate::{
    context::SvelteFormatContext,
    format::{format, format_with_session},
    options::{
        AllowShorthand, BracketSameLine, IndentScriptAndStyle, SortOrder, SvelteFormatOptions,
        WhitespaceSensitivity,
    },
};
