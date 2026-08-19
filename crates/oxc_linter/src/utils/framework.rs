//! Helpers for rules that must account for Svelte / Vue single-file
//! components.
//!
//! Only the `<script>` blocks of a `.svelte` / `.vue` file reach the ordinary
//! rule pass, so plain JavaScript rules can misread framework syntax: markup
//! reads and writes bindings the script never touches, compiler globals look
//! undeclared, and `export` means "prop" rather than "module export".

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

/// Svelte 5 runes. Members are reached through the same identifier
/// (`$state.raw`, `$derived.by`, `$effect.pre`, `$props.id`), so matching the
/// identifier covers those too.
pub const SVELTE_RUNES: [&str; 7] =
    ["$bindable", "$derived", "$effect", "$host", "$inspect", "$props", "$state"];

/// Svelte 3/4 component pseudo-globals.
pub const SVELTE_PSEUDO_GLOBALS: [&str; 3] = ["$$props", "$$restProps", "$$slots"];

/// Vue `<script setup>` compiler macros, which are never imported.
pub const VUE_SETUP_MACROS: [&str; 7] = [
    "defineEmits",
    "defineExpose",
    "defineModel",
    "defineOptions",
    "defineProps",
    "defineSlots",
    "withDefaults",
];

/// Whether `path` is a Svelte component or a Svelte "runes module"
/// (`foo.svelte.ts` / `foo.svelte.js`), both of which may use runes.
pub fn is_svelte_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    name.ends_with(".svelte") || name.contains(".svelte.")
}

/// Whether `path` is a Vue single-file component.
pub fn is_vue_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "vue")
}

/// Parse one Svelte markup expression (the text inside `{…}`) on its own.
///
/// Returns `None` when the text is not a valid expression, which the markup
/// parser tolerates but a rule cannot analyse.
pub fn parse_svelte_expression<'alloc>(
    allocator: &'alloc Allocator,
    text: &'alloc str,
) -> Option<Expression<'alloc>> {
    Parser::new(allocator, text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
        .ok()
}
