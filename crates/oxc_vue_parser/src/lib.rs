//! Vue SFC and template parser.
//!
//! This crate is the shared foundation for native Vue support in the
//! formatter and the linter:
//!
//! - [`parse_sfc`] splits a `.vue` file into its top-level blocks
//!   (`<template>`, `<script>`, `<style>`, custom blocks) without touching
//!   their contents. Raw-text blocks (`script`/`style`) never nest;
//!   `<template>` does.
//! - [`parse_template`] parses template source into a [`ast::Node`] tree:
//!   elements with fully decomposed attributes (directives, `v-bind`/`:`,
//!   `v-on`/`@`, `v-slot`/`#`, modifiers), `{{ ... }}` interpolations,
//!   comments, and text runs.
//!
//! The parser never fails: malformed markup degrades to [`ast::Node::Raw`]
//! pass-through spans rather than errors, so consumers (formatter, linter)
//! always receive a tree that covers the whole input.
//!
//! Embedded JavaScript/TypeScript (directive values, interpolation
//! expressions) is intentionally NOT parsed here — spans point into the
//! source and consumers hand them to `oxc_parser` in the mode they need.

pub mod ast;
mod parser;
mod sfc;

pub use parser::parse_template;
pub use sfc::{Sfc, SfcBlock, parse_sfc};

#[cfg(test)]
mod tests;
