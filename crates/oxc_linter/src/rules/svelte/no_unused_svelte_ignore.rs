use cow_utils::CowUtils;
use oxc_allocator::Allocator;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use svelte_markup_parser::ast::{BlockKind, Node};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::svelte_scripts,
};

fn missing_code_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("svelte-ignore comment must include the code")
        .with_help(
            "Add the warning code(s) to suppress, e.g. `<!-- svelte-ignore a11y_autofocus -->`.",
        )
        .with_label(span)
}

fn unknown_code_diagnostic(code: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("svelte-ignore comment is used, but not warned")
        .with_help(format!(
            "'{code}' is not a recognized Svelte compiler warning code, so this comment suppresses nothing."
        ))
        .with_label(span)
}

fn dangling_comment_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("svelte-ignore comment is used, but not warned")
        .with_help(
            "No element, block, or tag follows this comment for it to apply to; a `svelte-ignore` comment suppresses warnings on the node after it.",
        )
        .with_label(span)
}

/// Every warning code the Svelte compiler can emit, plus the ignorable
/// runtime warnings.
///
/// Data provenance: `export const codes` in
/// `packages/svelte/src/compiler/warnings.js` and
/// `IGNORABLE_RUNTIME_WARNINGS` in `packages/svelte/src/constants.js`,
/// both from sveltejs/svelte `main` as of 2026-08-19 (Svelte 5). The Svelte
/// compiler checks ignore codes against exactly this combined list
/// (`extract_svelte_ignore.js`).
const KNOWN_CODES: [&str; 89] = [
    // Compiler warnings (warnings.js `codes`).
    "a11y_accesskey",
    "a11y_aria_activedescendant_has_tabindex",
    "a11y_aria_attributes",
    "a11y_autocomplete_valid",
    "a11y_autofocus",
    "a11y_click_events_have_key_events",
    "a11y_consider_explicit_label",
    "a11y_distracting_elements",
    "a11y_figcaption_index",
    "a11y_figcaption_parent",
    "a11y_hidden",
    "a11y_img_redundant_alt",
    "a11y_incorrect_aria_attribute_type",
    "a11y_incorrect_aria_attribute_type_boolean",
    "a11y_incorrect_aria_attribute_type_id",
    "a11y_incorrect_aria_attribute_type_idlist",
    "a11y_incorrect_aria_attribute_type_integer",
    "a11y_incorrect_aria_attribute_type_token",
    "a11y_incorrect_aria_attribute_type_tokenlist",
    "a11y_incorrect_aria_attribute_type_tristate",
    "a11y_interactive_supports_focus",
    "a11y_invalid_attribute",
    "a11y_label_has_associated_control",
    "a11y_media_has_caption",
    "a11y_misplaced_role",
    "a11y_misplaced_scope",
    "a11y_missing_attribute",
    "a11y_missing_content",
    "a11y_mouse_events_have_key_events",
    "a11y_no_abstract_role",
    "a11y_no_interactive_element_to_noninteractive_role",
    "a11y_no_noninteractive_element_interactions",
    "a11y_no_noninteractive_element_to_interactive_role",
    "a11y_no_noninteractive_tabindex",
    "a11y_no_redundant_roles",
    "a11y_no_static_element_interactions",
    "a11y_positive_tabindex",
    "a11y_role_has_required_aria_props",
    "a11y_role_supports_aria_props",
    "a11y_role_supports_aria_props_implicit",
    "a11y_unknown_aria_attribute",
    "a11y_unknown_role",
    "bidirectional_control_characters",
    "legacy_code",
    "unknown_code",
    "options_deprecated_accessors",
    "options_deprecated_immutable",
    "options_missing_custom_element",
    "options_removed_enable_sourcemap",
    "options_removed_hydratable",
    "options_removed_loop_guard_timeout",
    "options_renamed_ssr_dom",
    "custom_element_props_identifier",
    "export_let_unused",
    "legacy_component_creation",
    "non_reactive_update",
    "perf_avoid_inline_class",
    "perf_avoid_nested_class",
    "reactive_declaration_invalid_placement",
    "reactive_declaration_module_script_dependency",
    "state_referenced_locally",
    "store_rune_conflict",
    "css_unused_selector",
    "attribute_avoid_is",
    "attribute_global_event_reference",
    "attribute_illegal_colon",
    "attribute_invalid_property_name",
    "attribute_quoted",
    "bind_invalid_each_rest",
    "block_empty",
    "component_name_lowercase",
    "element_implicitly_closed",
    "element_invalid_self_closing_tag",
    "event_directive_deprecated",
    "node_invalid_placement_ssr",
    "script_context_deprecated",
    "script_unknown_attribute",
    "slot_element_deprecated",
    "svelte_component_deprecated",
    "svelte_element_invalid_this",
    "svelte_self_deprecated",
    // Ignorable runtime warnings (constants.js).
    "await_waterfall",
    "await_reactivity_loss",
    "state_snapshot_uncloneable",
    "binding_property_non_reactive",
    "hydration_attribute_changed",
    "hydration_html_changed",
    "ownership_invalid_binding",
    "ownership_invalid_mutation",
];

/// Legacy (Svelte ≤4) code → Svelte 5 code, for the handful of renames that
/// aren't a plain `-` → `_` respelling. From the `replacements` map in
/// Svelte's `extract_svelte_ignore.js` (identical to eslint-plugin-svelte's
/// `V5_REPLACEMENTS`).
const LEGACY_REPLACEMENTS: [(&str, &str); 9] = [
    ("non-top-level-reactive-declaration", "reactive_declaration_invalid_placement"),
    ("module-script-reactive-declaration", "reactive_declaration_module_script"),
    ("empty-block", "block_empty"),
    ("avoid-is", "attribute_avoid_is"),
    ("invalid-html-attribute", "attribute_invalid_property_name"),
    ("a11y-structure", "a11y_figcaption_parent"),
    ("illegal-attribute-character", "attribute_illegal_colon"),
    ("invalid-rest-eachblock-binding", "bind_invalid_each_rest"),
    ("unused-export-let", "export_let_unused"),
];

#[derive(Debug, Default, Clone)]
pub struct NoUnusedSvelteIgnore;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports `svelte-ignore` comments that cannot suppress anything:
    /// comments with no warning code at all, codes that are not Svelte
    /// compiler (or ignorable runtime) warning codes, and comments with no
    /// following element, block, or tag to apply to.
    ///
    /// Note: this is a subset of eslint-plugin-svelte's rule. Upstream runs
    /// the Svelte compiler and also reports codes that are **valid but did
    /// not match any actual compiler warning**; that requires the Svelte
    /// compiler and is NOT detected here.
    ///
    /// ### Why is this bad?
    ///
    /// A `svelte-ignore` comment that suppresses nothing is dead weight at
    /// best; at worst it hides a typo'd code while the author believes a
    /// real warning is being handled.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <!-- svelte-ignore -->
    /// <img src="x.png" />
    ///
    /// <!-- svelte-ignore a11y_autofocs -->
    /// <input autofocus />
    ///
    /// <div>
    ///   <!-- svelte-ignore a11y_autofocus -->
    /// </div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <!-- svelte-ignore a11y_autofocus -->
    /// <input autofocus />
    /// ```
    NoUnusedSvelteIgnore,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Disallow unusable `svelte-ignore` comments.",
);

impl Rule for NoUnusedSvelteIgnore {}

// Ports a documented subset of eslint-plugin-svelte's
// `no-unused-svelte-ignore`.
//
// Upstream compiles the file with the Svelte compiler and reports every
// ignore code the compiler flagged as unused — including codes that are
// valid but whose warning simply never fired. That compiler round-trip is
// out of reach here, so this port implements exactly three statically
// decidable cases:
//
// (a) a `svelte-ignore` comment with an empty code list
//     (upstream's `missingCode` message);
// (b) codes that are not in Svelte 5's warning-code list (embedded below;
//     legacy hyphenated Svelte-4 spellings are first normalized the way
//     Svelte's own `extract_svelte_ignore` does: via the special-rename
//     map, else `-` → `_`);
// (c) a markup `svelte-ignore` comment with no following sibling other
//     than text and comments — Svelte 5 attaches an ignore comment to the
//     next non-text, non-comment sibling in its fragment, so such a
//     comment provably suppresses nothing (upstream reports these through
//     the compiler's unused-ignore tracking).
//
// Documented deviations:
// - Valid codes whose warning did not actually fire are NOT reported
//   (needs the compiler). This is the rule's main upstream value and the
//   reason this port is a subset.
// - Code-list parsing: words are split Svelte-5 style — every
//   comma-separated word is a code, and after the first word without a
//   trailing comma further words are treated as codes only while they are
//   known warning codes (recovering legacy space-separated lists), with
//   everything after treated as prose. Parenthetical notes (`(…)`) are
//   ignored like upstream. Trailing prose is therefore never flagged, at
//   the cost of missing typos hidden inside prose.
// - `<script>`/`<style>` elements count as attachment targets for (c),
//   although the Svelte compiler keeps them out of the fragment — a
//   conservative choice (fewer reports, never a false one).
// - Inside `<script>` blocks, `// svelte-ignore` / `/* svelte-ignore */`
//   comments get checks (a) and (b) only; placement is not analyzed there.
impl SvelteTemplateRule for NoUnusedSvelteIgnore {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut reports: Vec<Report> = Vec::new();
        check_fragment(nodes, &mut reports);

        // `svelte-ignore` comments inside `<script>` blocks: checks (a) and
        // (b) only.
        for script in svelte_scripts(nodes, ctx.source_text()) {
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let allocator = Allocator::new();
            let parser_ret = Parser::new(&allocator, script.content, source_type).parse();
            if parser_ret.panicked {
                continue;
            }
            for comment in &parser_ret.program.comments {
                let content_span = comment.content_span();
                let text = content_span.source_text(script.content);
                match parse_svelte_ignore(text) {
                    None => {}
                    Some(IgnoreComment::MissingCode) => {
                        reports.push(Report::MissingCode(Span::new(
                            comment.span.start + script.offset,
                            comment.span.end + script.offset,
                        )));
                    }
                    Some(IgnoreComment::Codes(codes)) => {
                        let base = content_span.start + script.offset;
                        for code in codes {
                            if !code.known {
                                reports.push(Report::UnknownCode(shift(code.span, base)));
                            }
                        }
                    }
                }
            }
        }

        reports.sort_unstable_by_key(|report| report.span().start);
        for report in reports {
            ctx.diagnostic(match report {
                Report::MissingCode(span) => missing_code_diagnostic(span),
                Report::UnknownCode(span) => {
                    unknown_code_diagnostic(span.source_text(ctx.source_text()), span)
                }
                Report::Dangling(span) => dangling_comment_diagnostic(span),
            });
        }
    }
}

enum Report {
    /// (a) — spans the whole comment.
    MissingCode(Span),
    /// (b) — spans the unknown code.
    UnknownCode(Span),
    /// (c) — spans each code of the dangling comment.
    Dangling(Span),
}

impl Report {
    fn span(&self) -> Span {
        match self {
            Report::MissingCode(span) | Report::UnknownCode(span) | Report::Dangling(span) => *span,
        }
    }
}

fn shift(span: Span, base: u32) -> Span {
    Span::new(span.start + base, span.end + base)
}

/// Walk one fragment (a sibling list), checking each `svelte-ignore`
/// comment against its following siblings, then recurse into child
/// fragments.
fn check_fragment(nodes: &[Node<'_>], reports: &mut Vec<Report>) {
    for (index, node) in nodes.iter().enumerate() {
        match node {
            Node::Comment(comment) => {
                match parse_svelte_ignore(comment.content) {
                    None => {}
                    Some(IgnoreComment::MissingCode) => {
                        reports.push(Report::MissingCode(comment.span));
                    }
                    Some(IgnoreComment::Codes(codes)) => {
                        // Svelte 5 attaches ignore comments to the next
                        // sibling that is neither a comment nor text
                        // (2-analyze scans back over `Comment` and `Text`
                        // nodes). No such sibling → nothing can be
                        // suppressed.
                        let dangling = !nodes[index + 1..].iter().any(is_attachment_target);
                        let base = comment.content_span.start;
                        for code in codes {
                            if dangling {
                                reports.push(Report::Dangling(shift(code.span, base)));
                            } else if !code.known {
                                reports.push(Report::UnknownCode(shift(code.span, base)));
                            }
                        }
                    }
                }
            }
            Node::Element(element) => check_fragment(&element.children, reports),
            Node::Block(block) => match &block.kind {
                BlockKind::If(if_block) => {
                    for branch in &if_block.branches {
                        check_fragment(&branch.children, reports);
                    }
                }
                BlockKind::Each(each) => {
                    check_fragment(&each.children, reports);
                    if let Some(fallback) = &each.fallback {
                        check_fragment(fallback, reports);
                    }
                }
                BlockKind::Await(await_block) => {
                    check_fragment(&await_block.pending, reports);
                    if let Some(children) = &await_block.then_children {
                        check_fragment(children, reports);
                    }
                    if let Some(children) = &await_block.catch_children {
                        check_fragment(children, reports);
                    }
                }
                BlockKind::Key(key) => check_fragment(&key.children, reports),
                BlockKind::Snippet(snippet) => check_fragment(&snippet.children, reports),
                BlockKind::Unknown(unknown) => check_fragment(&unknown.children, reports),
            },
            _ => {}
        }
    }
}

/// A node an ignore comment can attach to. The compiler skips `Comment` and
/// `Text` siblings; everything else receives the ignore. `Raw` (doctype,
/// orphan markers) is treated as a target conservatively.
fn is_attachment_target(node: &Node<'_>) -> bool {
    !matches!(node, Node::Text(_) | Node::Comment(_))
}

/// One code word inside a `svelte-ignore` comment. `span` is relative to
/// the text handed to [`parse_svelte_ignore`].
struct IgnoreCode {
    span: Span,
    known: bool,
}

enum IgnoreComment {
    MissingCode,
    Codes(Vec<IgnoreCode>),
}

/// Parse a comment body (delimiters excluded). `None` when it is not a
/// `svelte-ignore` comment at all — like the compiler's
/// `/^\s*svelte-ignore\s/`, the marker must be followed by whitespace.
fn parse_svelte_ignore(text: &str) -> Option<IgnoreComment> {
    let indent = text.len() - text.trim_start().len();
    let after_marker = text[indent..].strip_prefix("svelte-ignore")?;
    if !after_marker.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let list_base = text.len() - after_marker.len();

    // Blank out parenthetical notes (`(because …)`) with spaces, keeping
    // offsets intact — same as upstream's PARENTHETICAL_NOTE_PATTERN.
    let mut masked = after_marker.as_bytes().to_vec();
    let mut search = 0;
    while let Some(open) = find_byte(&masked, b'(', search) {
        let Some(close) = find_byte(&masked, b')', open + 1) else { break };
        for byte in &mut masked[open..=close] {
            *byte = b' ';
        }
        search = close + 1;
    }

    // (a): nothing but whitespace (and notes) after the marker.
    if masked.iter().all(u8::is_ascii_whitespace) {
        return Some(IgnoreComment::MissingCode);
    }

    // Split into words of the compiler's `[\w$-]` alphabet, remembering
    // whether a comma directly follows each word.
    let mut words: Vec<(usize, usize, bool)> = Vec::new();
    let mut i = 0;
    while i < masked.len() {
        if is_code_byte(masked[i]) {
            let start = i;
            while i < masked.len() && is_code_byte(masked[i]) {
                i += 1;
            }
            let comma = masked.get(i) == Some(&b',');
            words.push((start, i, comma));
        } else {
            i += 1;
        }
    }

    let mut codes = Vec::new();
    let mut strict = true;
    for &(start, end, comma) in &words {
        let word = &after_marker[start..end];
        let known = is_known_code(word);
        if strict {
            // Svelte 5: every comma-separated word is a code; the first
            // word without a trailing comma is the last certain one.
            codes.push(IgnoreCode {
                span: Span::new(
                    u32::try_from(list_base + start).unwrap_or(u32::MAX),
                    u32::try_from(list_base + end).unwrap_or(u32::MAX),
                ),
                known,
            });
            if !comma {
                strict = false;
            }
        } else if known {
            // Lax continuation for legacy space-separated lists: further
            // words count as codes only while they are known codes.
            codes.push(IgnoreCode {
                span: Span::new(
                    u32::try_from(list_base + start).unwrap_or(u32::MAX),
                    u32::try_from(list_base + end).unwrap_or(u32::MAX),
                ),
                known: true,
            });
        } else {
            // Prose from here on.
            break;
        }
    }
    Some(IgnoreComment::Codes(codes))
}

fn find_byte(bytes: &[u8], needle: u8, from: usize) -> Option<usize> {
    bytes[from..].iter().position(|&b| b == needle).map(|pos| from + pos)
}

fn is_code_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte == b'-'
}

/// Whether a written code is a Svelte warning code, after the same
/// normalization Svelte applies: the special legacy renames, else `-` → `_`.
fn is_known_code(code: &str) -> bool {
    if KNOWN_CODES.contains(&code) {
        return true;
    }
    if let Some((_, replacement)) = LEGACY_REPLACEMENTS.iter().find(|(legacy, _)| *legacy == code) {
        return KNOWN_CODES.contains(replacement);
    }
    if code.contains('-') {
        let respelled = code.cow_replace('-', "_");
        return KNOWN_CODES.contains(&respelled.as_ref());
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoUnusedSvelteIgnore;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Known code applying to the next element (upstream
            // valid/element-ignore01).
            (
                "<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<span tabindex=\"0\">
	<span class=\"element\"></span>
</span>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Legacy kebab spelling normalizes to a known code (upstream
            // valid/kebab-ignore).
            (
                "<!-- svelte-ignore a11y-no-noninteractive-tabindex -->
<span tabindex=\"0\"></span>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Legacy code with a special (non-mechanical) rename.
            (
                "<!-- svelte-ignore empty-block -->
{#if x}{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Space-separated multi-code list, both known (upstream
            // valid/html-comment).
            (
                "<!-- svelte-ignore a11y_autofocus a11y_missing_attribute -->
<img src=\"foo\" autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Comma-separated (Svelte 5 style).
            (
                "<!-- svelte-ignore a11y_autofocus, a11y_missing_attribute -->
<img src=\"foo\" autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Ignorable runtime warning codes are valid too.
            (
                "<!-- svelte-ignore hydration_attribute_changed -->
<img src={src} />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Trailing prose after a code without a comma is prose, not
            // codes (Svelte 5 semantics; subset boundary: typos hidden in
            // prose are not detected).
            (
                "<!-- svelte-ignore a11y_autofocus this input opens the modal -->
<input autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Parenthetical notes are ignored like upstream.
            (
                "<!-- svelte-ignore a11y_autofocus (needed for the modal) -->
<input autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Text between the comment and the element does not detach it
            // (the compiler scans over text siblings).
            (
                "<!-- svelte-ignore a11y_autofocus -->
some text
<input autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Stacked ignore comments all attach to the element.
            (
                "<!-- svelte-ignore a11y_autofocus -->
<!-- svelte-ignore a11y_missing_attribute -->
<img autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // Not an ignore comment at all.
            ("<!-- just a note -->", None, None, Some(PathBuf::from("test.svelte"))),
            // `svelte-ignore` with no following whitespace is not an ignore
            // comment (compiler regex requires it).
            ("<!--svelte-ignorex-->", None, None, Some(PathBuf::from("test.svelte"))),
            // Script comments with known codes (upstream
            // valid/script-comment; placement is not analyzed in scripts).
            (
                "<script>
	let count = $state(0);
	// svelte-ignore state_referenced_locally
	console.log(count);
</script>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // SUBSET BOUNDARY (documented): a valid code whose warning never
            // fires is NOT reported — upstream catches this by running the
            // Svelte compiler.
            (
                "<!-- svelte-ignore a11y_autofocus -->
<img src=\"foo\" alt=\"Foo\" />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];
        let fail = vec![
            // (a) Missing code.
            (
                "<!-- svelte-ignore -->
<img src=\"foo\" autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (a) A parenthetical note alone is still a missing code.
            (
                "<!-- svelte-ignore (see notes) -->
<input autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (b) Typo'd code.
            (
                "<!-- svelte-ignore a11y_autofocs -->
<input autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (b) Unknown code in a comma-separated list.
            (
                "<!-- svelte-ignore a11y_autofocus, foo_bar_baz -->
<input autofocus />",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (c) Dangling at the end of an `{#if}` branch (upstream
            // invalid/invalid-svelte-ignore01; both codes reported).
            (
                "<div>
	{#if true}
		A
		<!-- svelte-ignore a11y_label_has_associated_control a11y_no_noninteractive_tabindex -->
	{:else}
		<label tabindex=\"0\">Click</label>
		<ul tabindex=\"0\"></ul>
	{/if}
</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (c) Dangling at the end of an `{#each}` body before `{:else}`
            // (upstream invalid/invalid-svelte-ignore02).
            (
                "<div>
	{#each [] as e}
		A
		<!-- svelte-ignore a11y_label_has_associated_control a11y_no_noninteractive_tabindex -->
	{:else}
		<label tabindex=\"0\">Click</label>
		<ul tabindex=\"0\"></ul>
	{/each}
</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (c) Dangling at the end of the file.
            (
                "<div />
<!-- svelte-ignore a11y_autofocus -->",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (c) Dangling as the last child of an element.
            (
                "<div>
	<!-- svelte-ignore block_empty -->
</div>",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (b) inside a script comment.
            (
                "<script>
	// svelte-ignore not_a_real_code
	let a = 1;
</script>

{a}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
            // (a) inside a script block comment.
            (
                "<script>
	/* svelte-ignore */
	let a = 1;
</script>

{a}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoUnusedSvelteIgnore::NAME, NoUnusedSvelteIgnore::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
