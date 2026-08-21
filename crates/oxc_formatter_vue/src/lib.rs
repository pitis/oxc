//! Vue single-file component formatter built on top of `oxc_formatter_core`.
//!
//! Parses with [`vue_sfc_parser`] and prints the component, handing each
//! embedded language to the formatter that owns it: `<script>` to
//! `oxc_formatter`, `<style>` to `oxc_formatter_css`, and the `<template>`
//! markup printed here.
//!
//! A component whose blocks are not well-formed is refused rather than
//! rewritten: the parser recovers from anything, so the tree for such a file
//! is a guess, and reformatting a guess changes what the file means.

mod context;
mod format;
mod options;
pub(crate) mod print;

#[cfg(test)]
mod tests;

pub use crate::{
    context::VueFormatContext,
    format::{format, format_with_session},
    options::{VueFormatOptions, WhitespaceSensitivity},
};
