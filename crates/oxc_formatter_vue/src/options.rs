use oxc_formatter_core::{
    CoreFormatOptions, FormatOptions, IndentStyle, IndentWidth, LineEnding, LineWidth,
};

/// Format options for Vue single-file components.
///
/// The four core layout options plus the two Prettier defines for Vue, so a
/// project's existing `.prettierrc` keeps its meaning when `.vue` moves onto
/// the native printer.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct VueFormatOptions {
    pub indent_style: IndentStyle,
    pub indent_width: IndentWidth,
    pub line_width: LineWidth,
    pub line_ending: LineEnding,
    /// Whether the bodies of `<script>` and `<style>` are indented one level
    /// inside their tags. Prettier's `vueIndentScriptAndStyle`, default off.
    pub indent_script_and_style: bool,
    /// How much of the whitespace around an element is taken to matter.
    /// Prettier's `htmlWhitespaceSensitivity`.
    pub whitespace_sensitivity: WhitespaceSensitivity,
    /// Whether a tag's `>` stays on the last attribute's line instead of
    /// going onto one of its own. Prettier's `bracketSameLine`.
    pub bracket_same_line: bool,
    /// Whether a tag with more than one attribute always puts each on its own
    /// line. Prettier's `singleAttributePerLine`, default off. The
    /// component's own block tags are exempt, as they are for Prettier.
    pub single_attribute_per_line: bool,
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

impl FormatOptions for VueFormatOptions {
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
