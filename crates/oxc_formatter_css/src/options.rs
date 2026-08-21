use std::borrow::Cow;

use cow_utils::CowUtils;

use oxc_formatter_core::{
    CoreFormatOptions, FormatOptions, IndentStyle, IndentWidth, LineEnding, LineWidth,
};

/// CSS dialect variant.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub enum CssVariant {
    /// Prettier's `parser: css` equivalent.
    #[default]
    Css,
    /// Prettier's `parser: scss` equivalent.
    Scss,
    /// Prettier's `parser: less` equivalent.
    Less,
}

impl CssVariant {
    pub(crate) fn to_css_syntax(self) -> oxc_css_parser::Syntax {
        match self {
            Self::Css => oxc_css_parser::Syntax::Css,
            Self::Scss => oxc_css_parser::Syntax::Scss,
            Self::Less => oxc_css_parser::Syntax::Less,
        }
    }
}

/// What kind of CSS an embedded fragment is, which decides both what the
/// parser tolerates and how the result is laid out.
///
/// Only [`Self::Stylesheet`] is a document in its own right. The other two are
/// pieces of one, and both allow declarations where a stylesheet would demand
/// a rule.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub enum CssFragmentKind {
    /// A whole stylesheet, or a fence holding one: rules at the top level,
    /// each on its own line.
    #[default]
    Stylesheet,
    /// A css-in-js template: `` `PLACEHOLDER-N` `` markers stand in for the
    /// interpolations, and declarations may appear at the top level.
    Template,
    /// The value of an HTML `style` attribute — `color: red; margin: 0`.
    ///
    /// Declarations only, and laid out to fit on the attribute's line when
    /// they can: separated by a break that renders as a space, and with the
    /// last one's `;` written only if that break is taken. Prettier's
    /// `__isHTMLStyleAttribute`.
    StyleAttribute,
}

impl CssFragmentKind {
    /// Whether the source may carry css-in-js placeholder markers.
    pub fn has_template_placeholders(self) -> bool {
        matches!(self, Self::Template)
    }

    /// Whether a declaration may stand at the top level. A stylesheet rejects
    /// one as the recoverable error it is; a fragment of a document does not.
    pub fn allows_top_level_declarations(self) -> bool {
        matches!(self, Self::Template | Self::StyleAttribute)
    }

    /// Whether the statements lay out on one line when they fit, which is
    /// what an attribute value has to do.
    pub fn is_style_attribute(self) -> bool {
        matches!(self, Self::StyleAttribute)
    }
}

/// Format options for CSS/SCSS/Less.
///
/// Prettier's CSS languages consume the shared layout options plus
/// `singleQuote` and `trailingComma` (SCSS maps only).
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct CssFormatOptions {
    pub indent_style: IndentStyle,
    pub indent_width: IndentWidth,
    pub line_width: LineWidth,
    pub line_ending: LineEnding,
    pub variant: CssVariant,
    // Used by: CSS, SCSS, Less
    pub single_quote: SingleQuote,
    // Used by: SCSS
    pub trailing_commas: TrailingCommas,
    // Used by: CSS, SCSS, Less
    //
    // NOTE: Only the activation bit lives here.
    // The detailed Tailwind settings (config|stylesheet path, preserve-whitespace|duplicates, etc) are consumed by
    // the host-supplied sorter (`prettier-plugin-tailwindcss/sorter` on the JS side)
    // and travel separately via the host(Oxfmt)'s options payload, not through this struct.
    // `oxc_formatter_css` only needs to know whether to collect `@apply` classes.
    pub sort_tailwindcss: bool,
}

impl CssFormatOptions {
    /// Whether a trailing comma may follow the last item of a multi-line
    /// SCSS map, per [`Self::trailing_commas`].
    pub fn allow_trailing_comma(self) -> bool {
        matches!(self.trailing_commas, TrailingCommas::Always)
    }

    /// The quote byte (`b'"'` / `b'\''`) to enclose a string literal whose body is `inner`
    /// (the content between the quotes), per Prettier's `getPreferredQuote`:
    /// start from the configured preference (`singleQuote`) and flip to the alternate
    /// when that reduces escapes (i.e. when the preferred quote occurs more often in `inner` than the alternate).
    pub fn preferred_quote(&self, inner: &str) -> u8 {
        let (preferred, alternate) =
            if self.single_quote.value() { (b'\'', b'"') } else { (b'"', b'\'') };
        // Count every occurrence (escaped ones included, matching `getPreferredQuote`).
        let (mut preferred_count, mut alternate_count) = (0u32, 0u32);
        for byte in inner.bytes() {
            if byte == preferred {
                preferred_count += 1;
            } else if byte == alternate {
                alternate_count += 1;
            }
        }

        if preferred_count > alternate_count { alternate } else { preferred }
    }
}

/// Whether string literals prefer single quotes (`'`) over double (`"`).
/// Mirrors Prettier's `singleQuote` (default `false`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SingleQuote(bool);

impl SingleQuote {
    pub fn value(self) -> bool {
        self.0
    }

    pub fn as_char(self) -> char {
        if self.0 { '\'' } else { '"' }
    }

    pub fn as_str(self) -> &'static str {
        if self.0 { "'" } else { "\"" }
    }

    /// Prettier's `adjustStrings` for a single token:
    /// if `token` contains only the alternate quote and not the preferred one,
    /// replace alternates with preferreds.
    /// Returns the slice borrowed when no rewrite is needed.
    pub fn requote(self, token: &str) -> Cow<'_, str> {
        let (preferred, other) = if self.0 { ('\'', '"') } else { ('"', '\'') };
        if !token.contains(other) || token.contains(preferred) {
            return Cow::Borrowed(token);
        }
        token.cow_replace(other, preferred.encode_utf8(&mut [0; 4]))
    }
}

impl From<bool> for SingleQuote {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

/// Whether to print a trailing comma after the last item of a multi-line
/// SCSS map (the only CSS construct Prettier's `trailingComma` reaches).
///
/// Mirrors Prettier's `trailingComma`, but the `all`/`es5` distinction is
/// dead for CSS (`shouldPrintTrailingComma` only checks "not none"),
/// so both collapse into `Always`.
#[derive(Clone, Copy, Default, Debug, Eq, Hash, PartialEq)]
pub enum TrailingCommas {
    /// Trailing comma where valid. Maps from Prettier `all`/`es5`.
    #[default]
    Always,
    /// No trailing comma. Maps from Prettier `none`.
    Never,
}

impl FormatOptions for CssFormatOptions {
    fn indent_style(&self) -> IndentStyle {
        self.indent_style
    }

    fn indent_width(&self) -> IndentWidth {
        self.indent_width
    }

    fn line_width(&self) -> LineWidth {
        self.line_width
    }

    fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    fn apply_core(&mut self, core: CoreFormatOptions) {
        self.indent_style = core.indent_style;
        self.indent_width = core.indent_width;
        self.line_width = core.line_width;
        self.line_ending = core.line_ending;
    }
}
