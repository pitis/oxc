use oxc_ast::Comment;
use oxc_formatter_core::{FormatElement, SourceText};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashMap;

use crate::{TypeParameterAmbiguity, options::JsFormatOptions};

use super::Comments;

/// Entry in the Tailwind context stack, tracking whether we're inside a Tailwind class context.
#[derive(Clone, Copy, Debug)]
pub struct TailwindContextEntry {
    /// Whether to preserve whitespace (newlines) in template literals.
    pub preserve_whitespace: bool,
    /// Whether we're inside a template literal expression (between `${` and `}`).
    /// If true, we need to consider whitespace in adjacent quasis.
    pub in_template_expression: bool,
    /// Whether the quasi before this expression ends with whitespace.
    /// Only relevant when `in_template_expression` is true.
    pub quasi_before_has_trailing_ws: bool,
    /// Whether the quasi after this expression starts with whitespace.
    /// Only relevant when `in_template_expression` is true.
    pub quasi_after_has_leading_ws: bool,
    /// Whether this is the first quasi in a template literal.
    /// Used for template element boundary detection.
    pub is_first_quasi: bool,
    /// Whether this is the last quasi in a template literal.
    /// Used for template element boundary detection.
    pub is_last_quasi: bool,
    /// Whether Tailwind sorting is disabled in this context.
    /// Used to prevent sorting strings inside nested non-Tailwind call expressions.
    /// For example, in `classNames("a", x.includes("\n") ? "b" : "c")`, the `"\n"`
    /// inside `includes()` should NOT be sorted as a Tailwind class.
    pub disabled: bool,
}

impl TailwindContextEntry {
    /// Create a new context entry for JSX attributes or function calls.
    pub fn new(preserve_whitespace: bool) -> Self {
        Self {
            preserve_whitespace,
            in_template_expression: false,
            quasi_before_has_trailing_ws: true, // Default: can collapse
            quasi_after_has_leading_ws: true,   // Default: can collapse
            is_first_quasi: true,
            is_last_quasi: true,
            disabled: false,
        }
    }

    /// Create a new context entry for template literal expressions.
    /// Inherits `preserve_whitespace` from the parent context.
    pub fn template_expression(
        parent: TailwindContextEntry,
        quasi_before_has_trailing_ws: bool,
        quasi_after_has_leading_ws: bool,
    ) -> Self {
        Self {
            preserve_whitespace: parent.preserve_whitespace,
            in_template_expression: true,
            quasi_before_has_trailing_ws,
            quasi_after_has_leading_ws,
            is_first_quasi: true,
            is_last_quasi: true,
            disabled: false,
        }
    }

    /// Create a new context entry with updated quasi position.
    /// Used when formatting individual quasis to track their position in the template.
    #[must_use]
    pub fn with_quasi_position(mut self, is_first: bool, is_last: bool) -> Self {
        self.is_first_quasi = is_first;
        self.is_last_quasi = is_last;
        self
    }
}

/// Context object storing data relevant when formatting an object.
pub struct JsFormatContext<'ast> {
    options: JsFormatOptions,

    source_text: SourceText<'ast>,

    source_type: SourceType,

    comments: Comments<'ast>,

    cached_elements: FxHashMap<Span, FormatElement<'ast>>,

    /// Tracks whether quotes are needed for properties in the current object-like node.
    ///
    /// When [`JsFormatOptions::quote_properties`] is [`crate::QuoteProperties::Consistent`], each entry indicates
    /// whether at least one property key requires quotes. A stack is used to handle nested object-like
    /// structures (e.g., `{ a: { "b-c": 1 } }` where only the inner object needs quoted keys).
    quote_needed_stack: Vec<bool>,

    /// Collected Tailwind CSS class strings from JSX attributes.
    /// These will be sorted by an external callback and replaced during printing.
    tailwind_classes: Vec<String>,

    /// Stack tracking whether we're inside a Tailwind class context.
    /// When non-empty, StringLiterals should be sorted as Tailwind classes.
    tailwind_context_stack: Vec<TailwindContextEntry>,

    /// Whether the formatted code sits inside a double-quoted HTML attribute
    /// (js-in-xxx fragments). String literals are then forced to single quotes
    /// regardless of their content, mirroring Prettier's `__isInHtmlAttribute`:
    /// a swap to double quotes would be entity-escaped to `&quot;` by the host.
    embedded_in_html_attribute: bool,

    /// Whether the formatted code is a Vue expression fragment (`v-bind` values
    /// and `{{ ... }}` interpolations, Prettier's `__vue_expression` /
    /// `__vue_ts_expression` parsers). Enables the Vue 2 filter-sequence layout
    /// for top-level `|` chains (`{{ msg | uppercase }}`), which Prettier prints
    /// with the line break before the operator.
    embedded_vue_expression: bool,
    /// Whether the host embedding this fragment indents a broken expression
    /// itself, in which case a binaryish chain here must not indent again.
    /// See [`crate::FragmentContext::Expression`].
    fragment_host_indents: bool,
    /// Whether a lone type parameter needs a disambiguating trailing comma —
    /// see [`crate::TypeParameterAmbiguity`]. Set by the embedded entry point,
    /// since it is a fact about the *host file*, not about this source.
    type_parameters: TypeParameterAmbiguity,

    /// Whether this source is an embedded *fragment* — a template expression, a
    /// directive value, a snippet header — rather than a whole program.
    ///
    /// Prettier hands fragments to `babel`/`babel-ts` and whole files to the
    /// parser their extension names, and a JSDoc type cast is honoured by the
    /// former and dropped by the latter. See [`crate::utils::typecast`].
    embedded_fragment: bool,

    /// Whether the formatted code sits inside an HTML `{{ ... }}` interpolation,
    /// mirroring Prettier's `__isInHtmlInterpolation`. With `bracketSpacing:
    /// false`, an object expression whose closing `}` would touch a following
    /// `}` is parenthesized to avoid a premature `}}` in the template.
    embedded_in_html_interpolation: bool,
}

impl std::fmt::Debug for JsFormatContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsFormatContext")
            .field("options", &self.options)
            .field("source_text", &self.source_text)
            .field("source_type", &self.source_type)
            .field("comments", &self.comments)
            .field("cached_elements", &self.cached_elements)
            .field("quote_needed_stack", &self.quote_needed_stack)
            .field("tailwind_classes", &self.tailwind_classes)
            .finish()
    }
}

impl oxc_formatter_core::FormatContext for JsFormatContext<'_> {
    type Options = JsFormatOptions;

    fn options(&self) -> &JsFormatOptions {
        &self.options
    }

    fn source_code(&self) -> &str {
        &self.source_text
    }

    fn get_tailwind_class(&self, idx: usize) -> Option<&str> {
        self.tailwind_classes.get(idx).map(String::as_str)
    }
}

impl<'ast> JsFormatContext<'ast> {
    pub fn new(
        source_text: &'ast str,
        source_type: SourceType,
        comments: &'ast [Comment],
        options: JsFormatOptions,
    ) -> Self {
        let source_text = SourceText::new(source_text);
        Self {
            options,
            source_text,
            source_type,
            comments: Comments::new(source_text, comments),
            cached_elements: FxHashMap::default(),
            quote_needed_stack: Vec::new(),
            tailwind_classes: Vec::new(),
            tailwind_context_stack: Vec::new(),
            embedded_in_html_attribute: false,
            embedded_vue_expression: false,
            fragment_host_indents: true,
            type_parameters: TypeParameterAmbiguity::default(),
            embedded_fragment: false,
            embedded_in_html_interpolation: false,
        }
    }

    /// See the `embedded_in_html_attribute` field.
    #[must_use]
    pub fn with_embedded_in_html_attribute(mut self, yes: bool) -> Self {
        self.embedded_in_html_attribute = yes;
        self
    }

    /// See the `embedded_in_html_attribute` field.
    pub fn embedded_in_html_attribute(&self) -> bool {
        self.embedded_in_html_attribute
    }

    /// See the `embedded_vue_expression` field.
    #[must_use]
    pub fn with_embedded_vue_expression(mut self, yes: bool) -> Self {
        self.embedded_vue_expression = yes;
        self
    }

    /// See the `embedded_vue_expression` field.
    pub fn embedded_vue_expression(&self) -> bool {
        self.embedded_vue_expression
    }

    /// See the `fragment_host_indents` field.
    #[must_use]
    pub fn with_fragment_host_indents(mut self, yes: bool) -> Self {
        self.fragment_host_indents = yes;
        self
    }

    /// See the `fragment_host_indents` field.
    pub fn fragment_host_indents(&self) -> bool {
        self.fragment_host_indents
    }

    /// See [`TypeParameterAmbiguity`].
    #[must_use]
    pub fn with_type_parameter_ambiguity(mut self, ambiguity: TypeParameterAmbiguity) -> Self {
        self.type_parameters = ambiguity;
        self
    }

    /// See [`TypeParameterAmbiguity`].
    pub fn type_parameters(&self) -> TypeParameterAmbiguity {
        self.type_parameters
    }

    /// See the `embedded_fragment` field.
    #[must_use]
    pub fn with_embedded_fragment(mut self, yes: bool) -> Self {
        self.embedded_fragment = yes;
        self
    }

    /// See the `embedded_fragment` field.
    pub fn embedded_fragment(&self) -> bool {
        self.embedded_fragment
    }

    /// See the `embedded_in_html_interpolation` field.
    #[must_use]
    pub fn with_embedded_in_html_interpolation(mut self, yes: bool) -> Self {
        self.embedded_in_html_interpolation = yes;
        self
    }

    /// See the `embedded_in_html_interpolation` field.
    pub fn embedded_in_html_interpolation(&self) -> bool {
        self.embedded_in_html_interpolation
    }

    /// Returns a reference to the program's comments.
    pub fn comments(&self) -> &Comments<'ast> {
        &self.comments
    }

    /// Returns a reference to the program's comments.
    pub fn comments_mut(&mut self) -> &mut Comments<'ast> {
        &mut self.comments
    }

    /// Returns the source text wrapper
    pub fn source_text(&self) -> SourceText<'ast> {
        self.source_text
    }

    /// Returns the source type
    pub fn source_type(&self) -> SourceType {
        self.source_type
    }

    /// Returns the cached formatted element for the given key.
    pub(crate) fn get_cached_element<T: GetSpan>(&self, key: &T) -> Option<FormatElement<'ast>> {
        self.cached_elements.get(&key.span()).cloned()
    }

    /// Caches the formatted element for the given key.
    pub(crate) fn cache_element<T: GetSpan>(&mut self, key: &T, formatted: FormatElement<'ast>) {
        self.cached_elements.insert(key.span(), formatted);
    }

    /// Pushes a new quote needed state onto the stack.
    pub fn push_quote_needed(&mut self, needed: bool) {
        debug_assert!(
            self.options.quote_properties.is_consistent(),
            "`push_quote_needed` should only be used when `self.options.quote_properties.is_consistent()` is true"
        );
        self.quote_needed_stack.push(needed);
    }

    /// Pops the top quote needed state from the stack.
    pub fn pop_quote_needed(&mut self) {
        debug_assert!(
            self.options.quote_properties.is_consistent(),
            "`pop_quote_needed` should only be used when `self.options.quote_properties.is_consistent()` is true"
        );
        self.quote_needed_stack.pop();
    }

    pub fn is_quote_needed(&self) -> bool {
        *self.quote_needed_stack.last().unwrap_or(&false)
    }

    /// Set the collected Tailwind CSS classes.
    pub fn set_tailwind_classes(&mut self, classes: Vec<String>) {
        self.tailwind_classes = classes;
    }

    /// Push a Tailwind context entry onto the stack.
    /// Call this when entering a JSXAttribute or CallExpression with Tailwind class context.
    pub fn push_tailwind_context(&mut self, entry: TailwindContextEntry) {
        self.tailwind_context_stack.push(entry);
    }

    /// Pop a Tailwind context entry from the stack.
    /// Call this when leaving a JSXAttribute or CallExpression with Tailwind class context.
    pub fn pop_tailwind_context(&mut self) {
        self.tailwind_context_stack.pop();
    }

    /// Get the current Tailwind context, if any.
    /// Returns `Some` if we're inside a Tailwind class context (JSXAttribute or CallExpression).
    pub fn tailwind_context(&self) -> Option<&TailwindContextEntry> {
        self.tailwind_context_stack.last()
    }

    /// Get a mutable reference to the current Tailwind context, if any.
    pub fn tailwind_context_mut(&mut self) -> Option<&mut TailwindContextEntry> {
        self.tailwind_context_stack.last_mut()
    }
}
