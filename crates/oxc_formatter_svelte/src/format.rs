use oxc_allocator::Allocator;
use oxc_diagnostics::OxcDiagnostic;
use oxc_formatter_core::{
    Buffer, Document, Format, FormatSession, FormatState, Formatted, InputKind, VecBuffer,
    builders::{hard_line_break, text},
    write,
};
use svelte_markup_parser::{ParseResult, ast::Node, parse};

use crate::{
    context::SvelteFormatContext,
    options::SvelteFormatOptions,
    print::{SvelteFormatter, write_root},
};

/// Parse `source_text` as a Svelte component and build its formatter IR.
///
/// # Errors
/// Returns an [`OxcDiagnostic`] when the markup is not well-formed. The
/// markup parser never fails — it recovers — so this is the parser's
/// `recovered` flag rather than a hard error: a component the Svelte
/// compiler would reject is left untouched instead of being rewritten from
/// a guess at what was meant.
pub fn format<'a>(
    allocator: &'a Allocator,
    source_text: &str,
    options: SvelteFormatOptions,
) -> Result<Formatted<'a, SvelteFormatContext<'a>>, OxcDiagnostic> {
    // Compatibility wrapper: a service-less session, so `<script>` and
    // `<style>` stay as-is. Hosts that install services (oxfmt) use
    // [`format_with_session`].
    format_with_session(
        &FormatSession::new(allocator, InputKind::PhysicalFile),
        source_text,
        options,
    )
}

/// Like [`format()`], but on a caller-supplied [`FormatSession`] — the one
/// carrying the host's services, which is what lets a component's
/// `<script>` and `<style>` reach the formatters that own them.
///
/// # Errors
/// Same as [`format()`].
pub fn format_with_session<'a>(
    session: &FormatSession<'a>,
    source_text: &str,
    options: SvelteFormatOptions,
) -> Result<Formatted<'a, SvelteFormatContext<'a>>, OxcDiagnostic> {
    let allocator = session.allocator();
    let (has_bom, source_text) = oxc_formatter_core::spec::split_bom(source_text);
    // The printer re-applies the configured line ending, and `text` rejects a
    // lone `\r`, so normalize before anything looks at the source.
    let source_text = oxc_formatter_core::normalize_newlines(source_text, ['\r']);
    let source: &'a str = allocator.alloc_str(&source_text);

    let ParseResult { nodes, recovered } = parse(source, 0);
    if recovered {
        return Err(OxcDiagnostic::error(
            "Cannot format: the markup is not well-formed, and reformatting it would \
             change what it means.",
        ));
    }

    let context = SvelteFormatContext::new(options, source);
    let mut state = FormatState::new_with_session(context, session.clone());
    let capacity = (source.len() * 3 / 10).max(1024);
    let mut buffer = VecBuffer::with_capacity(capacity, &mut state);

    write!(&mut buffer, FormatSvelteRoot { nodes: &nodes, has_bom });

    let elements = buffer.into_vec();
    let context = state.into_context();

    Ok(Formatted::new(Document::new(elements, Vec::new()), context))
}

/// The whole component.
///
/// `'n` is the borrow of the node tree, which only has to outlive the IR
/// build; `'a` is the arena the source and the IR live in.
struct FormatSvelteRoot<'n, 'a> {
    nodes: &'n [Node<'a>],
    has_bom: bool,
}

impl<'a> Format<'a, SvelteFormatContext<'a>> for FormatSvelteRoot<'_, 'a> {
    fn fmt(&self, f: &mut SvelteFormatter<'_, 'a>) {
        if self.has_bom {
            write!(f, text("\u{feff}"));
        }
        write_root(self.nodes, f);
        // POSIX convention: a formatted file ends with a newline.
        write!(f, hard_line_break());
    }
}
