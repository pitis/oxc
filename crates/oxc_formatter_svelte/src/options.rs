use oxc_formatter_core::{
    CoreFormatOptions, FormatOptions, IndentStyle, IndentWidth, LineEnding, LineWidth,
};

/// Format options for Svelte components.
///
/// The four core layout options plus the ones `prettier-plugin-svelte`
/// defines, so a project's existing configuration keeps its meaning when it
/// moves onto the native printer.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct SvelteFormatOptions {
    pub indent_style: IndentStyle,
    pub indent_width: IndentWidth,
    pub line_width: LineWidth,
    pub line_ending: LineEnding,
    /// The order the component's top-level sections are printed in.
    pub sort_order: SortOrder,
    /// Whether `foo={foo}` may be written `{foo}`.
    pub allow_shorthand: AllowShorthand,
    /// Whether the bodies of `<script>` and `<style>` are indented one level
    /// inside their tags.
    pub indent_script_and_style: IndentScriptAndStyle,
    /// How much of the whitespace around an element is taken to matter.
    pub whitespace_sensitivity: WhitespaceSensitivity,
    /// Whether a tag's `>` stays on the last attribute's line instead of
    /// going onto one of its own.
    pub bracket_same_line: BracketSameLine,
    /// Whether the host sorts Tailwind classes, which is what decides whether
    /// a `class` attribute is worth collecting.
    pub sort_tailwind_classes: bool,
}

/// Which elements' surrounding whitespace is significant. Mirrors Prettier's
/// `htmlWhitespaceSensitivity`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WhitespaceSensitivity {
    /// What CSS says: whitespace shows around an inline element and not
    /// around a block one.
    #[default]
    Css,
    /// All of it matters, so nothing is ever laid out on its own line.
    Strict,
    /// None of it does, so everything may be.
    Ignore,
}


/// Where each top-level section goes. Mirrors `prettier-plugin-svelte`'s
/// `svelteSortOrder`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum SortOrder {
    /// `options-scripts-markup-styles` (the default).
    #[default]
    OptionsScriptsMarkupStyles,
    /// `options-scripts-styles-markup`.
    OptionsScriptsStylesMarkup,
    /// `options-markup-styles-scripts`.
    OptionsMarkupStylesScripts,
    /// `options-markup-scripts-styles`.
    OptionsMarkupScriptsStyles,
    /// `options-styles-markup-scripts`.
    OptionsStylesMarkupScripts,
    /// `options-styles-scripts-markup`.
    OptionsStylesScriptsMarkup,
    /// `none` — every section stays where it was written.
    None,
}

impl SortOrder {
    /// The sections in the order they are printed. `None` never reaches
    /// here: it keeps every section where the author put it.
    pub fn sections(self) -> [crate::print::Section; 4] {
        use crate::print::Section::{Markup, Options, Scripts, Styles};
        match self {
            Self::OptionsScriptsMarkupStyles | Self::None => [Options, Scripts, Markup, Styles],
            Self::OptionsScriptsStylesMarkup => [Options, Scripts, Styles, Markup],
            Self::OptionsMarkupStylesScripts => [Options, Markup, Styles, Scripts],
            Self::OptionsMarkupScriptsStyles => [Options, Markup, Scripts, Styles],
            Self::OptionsStylesMarkupScripts => [Options, Styles, Markup, Scripts],
            Self::OptionsStylesScriptsMarkup => [Options, Styles, Scripts, Markup],
        }
    }

    /// Parse the hyphen-joined spelling the config uses, or `None` when it
    /// names an order that does not exist.
    pub fn from_config_str(value: &str) -> Option<Self> {
        Some(match value {
            "options-scripts-markup-styles" => Self::OptionsScriptsMarkupStyles,
            "options-scripts-styles-markup" => Self::OptionsScriptsStylesMarkup,
            "options-markup-styles-scripts" => Self::OptionsMarkupStylesScripts,
            "options-markup-scripts-styles" => Self::OptionsMarkupScriptsStyles,
            "options-styles-markup-scripts" => Self::OptionsStylesMarkupScripts,
            "options-styles-scripts-markup" => Self::OptionsStylesScriptsMarkup,
            "none" => Self::None,
            _ => return None,
        })
    }
}

/// Whether `foo={foo}` is shortened to `{foo}`. Mirrors
/// `prettier-plugin-svelte`'s `svelteAllowShorthand`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AllowShorthand(bool);

impl AllowShorthand {
    pub fn is_enabled(self) -> bool {
        self.0
    }
}

impl Default for AllowShorthand {
    fn default() -> Self {
        Self(true)
    }
}

impl From<bool> for AllowShorthand {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

/// Whether `<script>` and `<style>` bodies are indented inside their tags.
/// Mirrors `prettier-plugin-svelte`'s `svelteIndentScriptAndStyle`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IndentScriptAndStyle(bool);

impl IndentScriptAndStyle {
    pub fn is_enabled(self) -> bool {
        self.0
    }
}

impl Default for IndentScriptAndStyle {
    fn default() -> Self {
        Self(true)
    }
}

impl From<bool> for IndentScriptAndStyle {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

/// Whether a tag's closing `>` stays on the last attribute's line. Mirrors
/// Prettier's `bracketSameLine`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct BracketSameLine(bool);

impl BracketSameLine {
    pub fn is_enabled(self) -> bool {
        self.0
    }
}

impl From<bool> for BracketSameLine {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

impl FormatOptions for SvelteFormatOptions {
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
