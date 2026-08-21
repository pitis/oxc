//! Embedded-language formatting infrastructure.
//!
//! All formatters are peers:
//! any formatter may act as a parent (containing embedded code) or as a child (being embedded).
//!
//! Only the entry formatter is called directly by the orchestrator (oxfmt);
//! every further embedded call goes through a [`FormatDispatcher`] that the orchestrator assembles,
//! mapping a language name to a formatter implementation (or a fallback).
//!
//! Core only carries the shared plumbing (arena, group-id space, Tailwind
//! class space, recursion handle);
//! anything truly language-pair specific crosses as a `dyn Any` passthrough.
//! Core knows nothing about any concrete language.

use std::{any::Any, sync::Arc};

use oxc_allocator::ArenaVec;

use crate::{FormatContext, FormatElement, FormatSession, Formatter, InputKind};

/// One embedded-language formatting request, as the host formatter states it.
pub struct DispatchRequest<'r> {
    /// Generic language identifier (e.g. `"css"`, `"graphql"`);
    /// the dispatcher implementation maps it to its own parser/language names.
    pub language: &'r str,
    /// The code to format.
    pub text: &'r str,
    /// Envelope semantics of the child input, as declared by the host.
    pub input_kind: InputKind,
    /// Parent→child language-pair specific data,
    /// downcast by the implementation (`None` for most pairs; e.g. JS's `CssInJsTemplate`).
    /// The borrowed counterpart of [`DispatchPayload::child_context`]
    /// (borrowed because the parent outlives the dispatch call).
    pub parent_context: Option<&'r dyn Any>,
}

/// The dispatcher's answer to a [`DispatchRequest`].
///
/// [`Self::PreserveOriginal`] is the DELIBERATE "do not format" answer
/// (unsupported language, child parse failure, an envelope the child refuses,
/// embedded formatting turned off): the caller keeps the original source as-is.
/// `Result::Err` around this enum is reserved for operational failures (transport / internal errors);
/// optional-embed callers degrade the same way for both,
/// but the two must never be conflated at the source.
pub enum DispatchResponse<'a> {
    /// The child formatted the request; consume [`DispatchPayload`].
    Formatted(DispatchPayload<'a>),
    /// Deliberately not formatted; keep the original source untouched.
    PreserveOriginal,
}

/// Dispatcher resolving a language name to a formatter implementation.
///
/// Assembled by the orchestrator (oxfmt), which knows all languages;
/// formatter crates only invoke it via [`FormatSession::dispatch`],
/// which owns the recursion limit and the no-dispatcher case.
/// The callback receives the CHILD session, already derived from the caller's
/// (same arena / `GroupId` space / dispatcher, the request's `InputKind`, depth + 1).
pub type FormatDispatcher = Arc<
    dyn for<'a, 'r> Fn(
            &FormatSession<'a>,
            DispatchRequest<'r>,
        ) -> Result<DispatchResponse<'a>, String>
        + Send
        + Sync,
>;

/// IR built by a language crate's embedded entry point (`format_to_ir`) for one input text.
/// The orchestrator's dispatcher wraps it into a [`DispatchPayload`].
///
/// Every language crate's `format_to_ir` returns this shape,
/// so a new child language only has to fill in the fields (no per-crate tuple conventions).
pub struct EmbeddedIr<'a> {
    /// The formatter IR, arena-allocated alongside its elements.
    ///
    /// Any `FormatElement::TailwindClass` in it already indexes the run's
    /// shared collector ([`FormatSession::add_tailwind_class`]), so splicing
    /// this into a parent document needs no renumbering.
    pub ir: ArenaVec<'a, FormatElement<'a>>,
}

impl<'a> From<EmbeddedIr<'a>> for DispatchPayload<'a> {
    fn from(embedded: EmbeddedIr<'a>) -> Self {
        Self { doc: embedded.ir, child_context: None }
    }
}

/// The child's formatted product, carried by [`DispatchResponse::Formatted`].
pub struct DispatchPayload<'a> {
    /// The formatted IR, arena-allocated alongside its elements.
    pub doc: ArenaVec<'a, FormatElement<'a>>,
    /// Child→parent language-pair specific data,
    /// downcast by the parent (`None` for most pairs; e.g. HTML's `has_multiple_root_elements`).
    /// The owned counterpart of [`DispatchRequest::parent_context`]
    /// (owned because it outlives the child's stack frame).
    pub child_context: Option<Box<dyn Any>>,
}

impl<'a> DispatchPayload<'a> {
    /// Hands out the child's doc.
    ///
    /// Nothing has to be merged: the child allocated its Tailwind indices from
    /// the run's shared collector, so they are already the parent document's.
    pub fn into_doc(self) -> ArenaVec<'a, FormatElement<'a>> {
        self.doc
    }
}

/// Child→parent datum for an embedded *expression*: whether its own brackets
/// can stand in for the break the host would otherwise put around it.
///
/// A markup host that embeds an expression in a delimited position — an HTML
/// attribute value, a template interpolation — has two layouts to choose
/// between. An object or array literal already opens and closes with a
/// bracket the reader can hang the indentation on, so it "hugs" the
/// delimiters; anything else needs the host to supply an indented break of
/// its own. Only the language that parsed the fragment can tell the two
/// apart, so it answers here, through [`DispatchPayload::child_context`].
///
/// This is Prettier's `shouldHugJsExpression`. The dispatcher decides it
/// rather than the embed site because the answer also depends on which parser
/// flavour the request named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpressionHugsDelimiters(pub bool);

/// Dispatches one embedded fragment and hands its IR to the caller.
///
/// `None` covers [`DispatchResponse::PreserveOriginal`] and operational errors alike;
/// the caller keeps its original source.
/// Embed sites that must inspect [`DispatchPayload::child_context`] first stay manual.
pub fn dispatch_fragment_ir<'a, C>(
    f: &Formatter<'_, 'a, C>,
    language: &str,
    text: &str,
    parent_context: Option<&dyn Any>,
) -> Option<ArenaVec<'a, FormatElement<'a>>>
where
    C: FormatContext,
{
    let Ok(DispatchResponse::Formatted(result)) = f.session().dispatch(DispatchRequest {
        language,
        text,
        input_kind: InputKind::Fragment,
        parent_context,
    }) else {
        return None;
    };
    Some(result.into_doc())
}
