use oxc_formatter_core::{FormatContext, SourceText};

use crate::{
    comments::{Comments, CssComment},
    options::{CssFormatOptions, CssFragmentKind},
};

/// Formatting context for CSS/SCSS/Less.
pub struct CssFormatContext<'a> {
    options: CssFormatOptions,
    source_text: SourceText<'a>,
    comments: Comments<'a>,
    /// Inside a Less detached ruleset (`@var: { ... }`): property names keep
    /// their case (Prettier checks `parentNode.variable`).
    in_less_detached: std::cell::Cell<bool>,
    /// Inside an ICSS rule (`:import(...)` / `:export`): property names keep
    /// their case (Prettier's `insideIcssRuleNode`).
    in_icss_rule: std::cell::Cell<bool>,
    /// What kind of CSS this is: a whole stylesheet, a css-in-js template,
    /// or an HTML `style` attribute's value. Gates both placeholder handling
    /// and the attribute layout.
    kind: CssFragmentKind,
}

impl<'a> CssFormatContext<'a> {
    pub fn new(
        options: CssFormatOptions,
        source_code: &'a str,
        comments: &'a [CssComment],
        kind: CssFragmentKind,
    ) -> Self {
        Self {
            options,
            source_text: SourceText::new(source_code),
            comments: Comments::new(comments),
            in_less_detached: std::cell::Cell::new(false),
            in_icss_rule: std::cell::Cell::new(false),
            kind,
        }
    }

    /// Whether the source may contain css-in-js `${}` placeholder markers.
    pub fn template_placeholders(&self) -> bool {
        self.kind.has_template_placeholders()
    }

    /// Whether this is an HTML `style` attribute's value, which lays its
    /// declarations out on one line when they fit.
    pub fn is_style_attribute(&self) -> bool {
        self.kind.is_style_attribute()
    }

    pub fn in_less_detached(&self) -> &std::cell::Cell<bool> {
        &self.in_less_detached
    }

    pub fn in_icss_rule(&self) -> &std::cell::Cell<bool> {
        &self.in_icss_rule
    }

    /// Returns the source text with the arena lifetime (vs the trait's borrow-elided `&str`).
    pub fn source_text(&self) -> SourceText<'a> {
        self.source_text
    }

    /// Returns the comment cursor.
    pub fn comments(&self) -> &Comments<'a> {
        &self.comments
    }
}

/// Lets a dispatched child's classes remap into this host's index space (`DispatchPayload::into_doc`).
impl FormatContext for CssFormatContext<'_> {
    type Options = CssFormatOptions;

    fn options(&self) -> &Self::Options {
        &self.options
    }

    fn source_code(&self) -> &str {
        &self.source_text
    }
}
