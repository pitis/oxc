//! The printer. S1 re-emits the component verbatim, so this is only the
//! formatter alias every later stage's printing hangs off.

use oxc_formatter_core::Formatter;

use crate::context::SvelteFormatContext;

pub type SvelteFormatter<'buf, 'a> = Formatter<'buf, 'a, SvelteFormatContext<'a>>;
