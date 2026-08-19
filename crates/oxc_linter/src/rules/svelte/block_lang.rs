use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeValue, Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{get_plain_attribute, svelte_start_tag_span, walk_svelte_elements},
};

fn block_lang_diagnostic(message: String, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(message)
        .with_help("Set the block's `lang` attribute to one of the configured languages.")
        .with_label(span)
}

/// One entry of the `script` / `style` option: a language name, or `null`
/// meaning "no `lang` attribute at all".
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum Lang {
    Name(String),
    /// `null` in the config: the block must have no `lang` attribute.
    None_,
}

/// The `script` / `style` option accepts a single value or a list.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum Langs {
    One(Lang),
    Many(Vec<Lang>),
}

impl Default for Langs {
    fn default() -> Self {
        Self::One(Lang::None_)
    }
}

impl Langs {
    fn as_slice(&self) -> &[Lang] {
        match self {
            Self::One(lang) => std::slice::from_ref(lang),
            Self::Many(langs) => langs,
        }
    }

    /// Whether `lang` (the block's `lang` attribute, lowercased, or `None`)
    /// is allowed.
    fn allows(&self, lang: Option<&str>) -> bool {
        self.as_slice().iter().any(|allowed| match (allowed, lang) {
            (Lang::None_, None) => true,
            (Lang::Name(name), Some(lang)) => name.eq_ignore_ascii_case(lang),
            _ => false,
        })
    }

    /// Upstream's `prettyPrintLangs`, for the message text.
    fn pretty(&self) -> String {
        let names: Vec<String> = self
            .as_slice()
            .iter()
            .map(|lang| match lang {
                Lang::None_ => "not set".to_string(),
                Lang::Name(name) => format!("\"{name}\""),
            })
            .collect();
        match names.as_slice() {
            [] => "not set".to_string(),
            [one] => one.clone(),
            [head @ .., last] => format!("{} or {last}", head.join(", ")),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct BlockLangConfig {
    /// Require a `<script>` block to be present at all.
    enforce_script_present: bool,
    /// Require a `<style>` block to be present at all.
    enforce_style_present: bool,
    /// The `lang` value(s) allowed on `<script>`; `null` means no `lang`.
    script: Langs,
    /// The `lang` value(s) allowed on `<style>`; `null` means no `lang`.
    style: Langs,
}

// Boxed: the option lists would blow `RuleEnum`'s 16-byte budget unboxed.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BlockLang(Box<BlockLangConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Restricts the `lang` attribute of a component's `<script>` and
    /// `<style>` blocks to the configured languages, and optionally requires
    /// those blocks to be present.
    ///
    /// ### Why is this bad?
    ///
    /// A codebase that has settled on TypeScript and SCSS wants that applied
    /// uniformly; a stray plain-JS `<script>` or plain-CSS `<style>` is
    /// usually an oversight rather than a decision.
    ///
    /// ### Examples
    ///
    /// With `["error", { "script": "ts", "style": "scss" }]`:
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script></script>
    /// <style></style>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script lang="ts"></script>
    /// <style lang="scss"></style>
    /// ```
    ///
    /// ### Options
    ///
    /// - `script` / `style`: an allowed language, `null` for "no `lang`
    ///   attribute", or an array mixing both. Both default to `null`.
    /// - `enforceScriptPresent` / `enforceStylePresent` (default `false`):
    ///   also report when the block is missing entirely.
    ///
    /// ```json
    /// {
    ///   "svelte/block-lang": ["error", { "script": ["ts", null], "style": "scss" }]
    /// }
    /// ```
    BlockLang,
    svelte,
    restriction,
    config = BlockLang,
    version = "1.80.0",
    short_description = "Restrict the `lang` of `<script>` and `<style>` blocks.",
);

impl Rule for BlockLang {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for BlockLang {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let config = &self.0;
        // Collected as (lang, start-tag span) so nothing borrows the tree
        // past the walk.
        let mut scripts: Vec<(Option<&str>, Span)> = Vec::new();
        let mut styles: Vec<(Option<&str>, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            if element.name.eq_ignore_ascii_case("script") {
                scripts.push((lang_of(element), svelte_start_tag_span(element)));
            } else if element.name.eq_ignore_ascii_case("style") {
                styles.push((lang_of(element), svelte_start_tag_span(element)));
            }
        });

        let mut diagnostics = Vec::new();
        // A missing block is reported at the start of the file, like upstream.
        let file_start = Span::empty(0);
        if scripts.is_empty() && config.enforce_script_present {
            diagnostics.push(block_lang_diagnostic(
                format!(
                    "The <script> block should be present and its lang attribute should be {}.",
                    config.script.pretty()
                ),
                file_start,
            ));
        }
        if styles.is_empty() && config.enforce_style_present {
            diagnostics.push(block_lang_diagnostic(
                format!(
                    "The <style> block should be present and its lang attribute should be {}.",
                    config.style.pretty()
                ),
                file_start,
            ));
        }
        for (elements, langs, block) in
            [(scripts, &config.script, "script"), (styles, &config.style, "style")]
        {
            for (lang, span) in elements {
                if !langs.allows(lang) {
                    diagnostics.push(block_lang_diagnostic(
                        format!(
                            "The lang attribute of the <{block}> block should be {}.",
                            langs.pretty()
                        ),
                        span,
                    ));
                }
            }
        }
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

/// The block's `lang` attribute when it is a plain literal.
fn lang_of<'a>(element: &Element<'a>) -> Option<&'a str> {
    get_plain_attribute(element, "lang")
        .and_then(|(_, value)| value.and_then(AttributeValue::as_static_text))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::BlockLang;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let ts_scss = || Some(serde_json::json!([{ "script": "ts", "style": "scss" }]));
        let pass = vec![
            // Default: both blocks must have no `lang`.
            ("<script></script>\n<style></style>", None, None, path()),
            ("<div></div>", None, None, path()),
            (
                "<script lang=\"ts\"></script>\n<style lang=\"scss\"></style>",
                ts_scss(),
                None,
                path(),
            ),
            // A list may include `null` for "no lang".
            (
                "<script></script>",
                Some(serde_json::json!([{ "script": ["ts", null] }])),
                None,
                path(),
            ),
            // Matching is case-insensitive.
            (
                "<script lang=\"TS\"></script>\n<style lang=\"scss\"></style>",
                ts_scss(),
                None,
                path(),
            ),
            // Blocks are only required when asked for.
            ("<div></div>", ts_scss(), None, path()),
        ];
        let fail = vec![
            ("<script lang=\"ts\"></script>", None, None, path()),
            ("<script></script>\n<style></style>", ts_scss(), None, path()),
            (
                "<script lang=\"js\"></script>",
                Some(serde_json::json!([{ "script": ["ts", "tsx"] }])),
                None,
                path(),
            ),
            // Missing blocks.
            (
                "<div></div>",
                Some(serde_json::json!([{ "script": "ts", "enforceScriptPresent": true }])),
                None,
                path(),
            ),
            (
                "<div></div>",
                Some(serde_json::json!([{ "style": "scss", "enforceStylePresent": true }])),
                None,
                path(),
            ),
        ];

        Tester::new(BlockLang::NAME, BlockLang::PLUGIN, pass, fail).test_and_snapshot();
    }
}
