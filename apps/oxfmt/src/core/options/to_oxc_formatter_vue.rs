use oxc_formatter_core::{CoreFormatOptions, FormatOptions};
use oxc_formatter_vue::{VueFormatOptions, WhitespaceSensitivity};

use super::super::oxfmtrc::{FormatConfig, HtmlWhitespaceSensitivityConfig};

/// Convert `FormatConfig` into `VueFormatOptions` for `oxc_formatter_vue`.
///
/// Every option here is one Prettier already defines for `.vue`, so a project
/// keeps one set of settings whichever printer runs.
///
/// NOTE: Pure field translation; `core` comes pre-validated from the
/// config-resolution gate, so this cannot fail.
pub fn to_oxc_formatter_vue(
    config: &FormatConfig,
    core_options: CoreFormatOptions,
) -> VueFormatOptions {
    let mut options = VueFormatOptions::default();
    options.apply_core(core_options);

    // [Prettier] htmlWhitespaceSensitivity: "css" | "strict" | "ignore"
    if let Some(sensitivity) = config.html_whitespace_sensitivity {
        options.whitespace_sensitivity = match sensitivity {
            HtmlWhitespaceSensitivityConfig::Css => WhitespaceSensitivity::Css,
            HtmlWhitespaceSensitivityConfig::Strict => WhitespaceSensitivity::Strict,
            HtmlWhitespaceSensitivityConfig::Ignore => WhitespaceSensitivity::Ignore,
        };
    }
    // [Prettier] bracketSameLine: boolean
    if let Some(same_line) = config.bracket_same_line {
        options.bracket_same_line = same_line;
    }
    // [Prettier] vueIndentScriptAndStyle: boolean
    if let Some(indent) = config.vue_indent_script_and_style {
        options.indent_script_and_style = indent;
    }
    // [Oxfmt] sortTailwindcss: the collection is only worth its allocation
    // when something is going to sort the result.
    options.sort_tailwind_classes = config.is_tailwind_enabled();

    options
}
