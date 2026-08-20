use oxc_formatter_core::{CoreFormatOptions, FormatOptions};
use oxc_formatter_svelte::{SortOrder, SvelteFormatOptions};

use super::super::oxfmtrc::{FormatConfig, SvelteUserConfig};

/// Convert `FormatConfig` into `SvelteFormatOptions` for `oxc_formatter_svelte`.
///
/// The `svelte` config key is shared with `prettier-plugin-svelte`, so a
/// project keeps one set of options whichever printer runs.
///
/// NOTE: Pure field translation:
/// `core` comes pre-validated from the config-resolution gate (`validate()`), so this cannot fail.
/// An unknown `sortOrder` spelling keeps the default rather than failing, matching how the
/// plugin's own option validation is not re-run here.
pub fn to_oxc_formatter_svelte(
    config: &FormatConfig,
    core_options: CoreFormatOptions,
) -> SvelteFormatOptions {
    let mut options = SvelteFormatOptions::default();
    options.apply_core(core_options);

    let Some(svelte) = config.svelte.clone().and_then(SvelteUserConfig::into_config) else {
        return options;
    };

    // [prettier-plugin-svelte] svelteSortOrder: string
    if let Some(sort_order) = svelte.sort_order.as_deref().and_then(SortOrder::from_config_str) {
        options.sort_order = sort_order;
    }
    // [prettier-plugin-svelte] svelteAllowShorthand: boolean
    if let Some(allow_shorthand) = svelte.allow_shorthand {
        options.allow_shorthand = allow_shorthand.into();
    }
    // [prettier-plugin-svelte] svelteIndentScriptAndStyle: boolean
    if let Some(indent) = svelte.indent_script_and_style {
        options.indent_script_and_style = indent.into();
    }

    options
}
