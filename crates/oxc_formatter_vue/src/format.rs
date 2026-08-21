use oxc_allocator::Allocator;
use oxc_diagnostics::OxcDiagnostic;
use oxc_formatter_core::{
    Buffer, Document, Format, FormatSession, FormatState, Formatted, InputKind, VecBuffer,
    builders::{hard_line_break, text},
    write,
};
use vue_sfc_parser::{Sfc, parse_sfc};

use crate::{
    context::VueFormatContext,
    options::VueFormatOptions,
    print::{VueFormatter, write_root},
};

/// Parse `source_text` as a Vue single-file component and build its IR.
///
/// # Errors
/// Returns an [`OxcDiagnostic`] when the component is not well-formed. The SFC
/// parser recovers rather than failing, so a file it had to guess at is
/// refused instead of rewritten from that guess.
pub fn format<'a>(
    allocator: &'a Allocator,
    source_text: &str,
    options: VueFormatOptions,
) -> Result<Formatted<'a, VueFormatContext<'a>>, OxcDiagnostic> {
    // Compatibility wrapper: a service-less session, so `<script>` and
    // `<style>` stay as written. Hosts that install services (oxfmt) use
    // [`format_with_session`].
    format_with_session(
        &FormatSession::new(allocator, InputKind::PhysicalFile),
        source_text,
        options,
    )
}

/// Like [`format()`], but on a caller-supplied [`FormatSession`] — the one
/// carrying the host's services, which is what lets a component's `<script>`
/// and `<style>` reach the formatters that own them.
///
/// # Errors
/// Same as [`format()`].
pub fn format_with_session<'a>(
    session: &FormatSession<'a>,
    source_text: &str,
    options: VueFormatOptions,
) -> Result<Formatted<'a, VueFormatContext<'a>>, OxcDiagnostic> {
    let allocator = session.allocator();
    let (has_bom, source_text) = oxc_formatter_core::spec::split_bom(source_text);
    // The printer re-applies the configured line ending, and `text` rejects a
    // lone `\r`, so normalize before anything looks at the source.
    let source_text = oxc_formatter_core::normalize_newlines(source_text, ['\r']);
    let source: &'a str = allocator.alloc_str(&source_text);

    let sfc = parse_sfc(source);
    if let Some(reason) = not_well_formed(&sfc) {
        return Err(OxcDiagnostic::error(format!(
            "Cannot format: {reason}, and reformatting it would change what it means."
        )));
    }

    let context = VueFormatContext::new(options, source);
    let mut state = FormatState::new_with_session(context, session.clone());
    let capacity = (source.len() * 3 / 10).max(1024);
    let mut buffer = VecBuffer::with_capacity(capacity, &mut state);

    write!(&mut buffer, FormatVueRoot { sfc: &sfc, has_bom });

    let elements = buffer.into_vec();
    let mut context = state.into_context();
    let tailwind_classes = context.take_tailwind_classes();
    let sorted = session.sort_tailwind_classes(tailwind_classes);

    Ok(Formatted::new(Document::new(elements, sorted), context))
}

/// Why this component cannot be safely reprinted, if it cannot.
fn not_well_formed(sfc: &Sfc<'_>) -> Option<&'static str> {
    if sfc.blocks.iter().any(|block| block.unclosed) {
        return Some("a top-level block is never closed");
    }
    None
}

/// The whole component.
///
/// `'n` is the borrow of the parsed SFC, which only has to outlive the IR
/// build; `'a` is the arena the source and the IR live in.
struct FormatVueRoot<'n, 'a> {
    sfc: &'n Sfc<'a>,
    has_bom: bool,
}

impl<'a> Format<'a, VueFormatContext<'a>> for FormatVueRoot<'_, 'a> {
    fn fmt(&self, f: &mut VueFormatter<'_, 'a>) {
        if self.has_bom {
            write!(f, text("\u{feff}"));
        }
        write_root(self.sfc, f);
        // POSIX convention: a formatted file ends with a newline.
        write!(f, hard_line_break());
    }
}
