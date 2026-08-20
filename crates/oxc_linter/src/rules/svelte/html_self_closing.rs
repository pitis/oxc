use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{Element, Node};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn require_closing_diagnostic(kind: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Require self-closing on {kind}."))
        .with_help("Write the empty element as `<name />`.")
        .with_label(span)
}

fn disallow_closing_diagnostic(kind: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Disallow self-closing on {kind}."))
        .with_help("Write the element with a closing tag instead.")
        .with_label(span)
}

/// Which kind of element a tag is, in the rule's own terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Void,
    Normal,
    Svg,
    Math,
    Component,
    Svelte,
}

impl ElementKind {
    /// How the rule names this kind in its messages.
    fn noun(self) -> &'static str {
        match self {
            Self::Void => "HTML void elements",
            Self::Normal => "HTML elements",
            Self::Svg => "SVG elements",
            Self::Math => "MathML elements",
            Self::Component => "Svelte custom components",
            Self::Svelte => "Svelte special elements",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Style {
    /// Require `<name />`.
    Always,
    /// Require a closing tag.
    #[default]
    Never,
    /// Leave this kind alone.
    Ignore,
}

/// The per-kind form, as an object of six independent settings.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase", default, deny_unknown_fields)]
pub struct Kinds {
    void: Style,
    normal: Style,
    svg: Style,
    math: Style,
    component: Style,
    svelte: Style,
}

impl Default for Kinds {
    fn default() -> Self {
        Self {
            void: Style::Always,
            normal: Style::Never,
            svg: Style::Always,
            math: Style::Never,
            component: Style::Always,
            svelte: Style::Always,
        }
    }
}

impl Kinds {
    fn style(self, kind: ElementKind) -> Style {
        match kind {
            ElementKind::Void => self.void,
            ElementKind::Normal => self.normal,
            ElementKind::Svg => self.svg,
            ElementKind::Math => self.math,
            ElementKind::Component => self.component,
            ElementKind::Svelte => self.svelte,
        }
    }
}

/// The shorthand form, naming a whole preset at once.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// Self-close everything that can be.
    All,
    /// Self-close only what HTML itself would.
    Html,
    /// Never self-close.
    None,
}

impl Preset {
    fn kinds(self) -> Kinds {
        match self {
            Self::All => Kinds {
                void: Style::Always,
                normal: Style::Always,
                svg: Style::Always,
                math: Style::Always,
                component: Style::Always,
                svelte: Style::Always,
            },
            Self::Html => Kinds {
                void: Style::Always,
                normal: Style::Never,
                svg: Style::Always,
                math: Style::Never,
                component: Style::Never,
                svelte: Style::Always,
            },
            Self::None => Kinds {
                void: Style::Never,
                normal: Style::Never,
                svg: Style::Never,
                math: Style::Never,
                component: Style::Never,
                svelte: Style::Never,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum HtmlSelfClosing {
    /// `"all"`, `"html"` or `"none"`.
    Preset(Preset),
    /// A setting per element kind.
    Kinds(Kinds),
}

impl Default for HtmlSelfClosing {
    fn default() -> Self {
        Self::Kinds(Kinds::default())
    }
}

impl HtmlSelfClosing {
    fn kinds(self) -> Kinds {
        match self {
            Self::Preset(preset) => preset.kinds(),
            Self::Kinds(kinds) => kinds,
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces whether an element with no content is written self-closing.
    ///
    /// ### Why is this bad?
    ///
    /// Writing `<Widget></Widget>` in one place and `<Widget />` in another
    /// is noise, and `<div />` is misleading in HTML, where a `div` is never
    /// actually self-closing.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <Widget></Widget>
    /// <input>
    /// <div />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <Widget />
    /// <input />
    /// <div></div>
    /// ```
    ///
    /// ### Options
    ///
    /// Either a preset — `"all"`, `"html"` or `"none"` — or a setting per
    /// element kind, each `"always"`, `"never"` or `"ignore"`. The defaults
    /// are `void`, `svg`, `component` and `svelte` `"always"`, `normal` and
    /// `math` `"never"`.
    ///
    /// ```json
    /// { "svelte/html-self-closing": ["error", { "normal": "always" }] }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream rewrites the tag; the Svelte markup pass reports only.
    HtmlSelfClosing,
    svelte,
    style,
    config = HtmlSelfClosing,
    version = "1.80.0",
    short_description = "Enforce whether an empty element is self-closing.",
);

impl Rule for HtmlSelfClosing {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for HtmlSelfClosing {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let kinds = self.kinds();
        let mut diagnostics = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            // `<script>` and `<style>` are separate node types upstream, so
            // the rule never sees them.
            if element.name.eq_ignore_ascii_case("script")
                || element.name.eq_ignore_ascii_case("style")
            {
                return;
            }
            if !is_empty(element) {
                return;
            }
            let kind = element_kind(element);
            let should_self_close = match kinds.style(kind) {
                Style::Ignore => return,
                Style::Always => true,
                Style::Never => false,
            };
            if should_self_close == element.self_closing {
                return;
            }
            // Point from the `/` (or the `>` when there is none) to the end
            // of the element, as upstream does.
            let start = element.open_tag_end - if element.self_closing { 2 } else { 1 };
            let span = Span::new(start, element.span.end);
            diagnostics.push(if should_self_close {
                require_closing_diagnostic(kind.noun(), span)
            } else {
                disallow_closing_diagnostic(kind.noun(), span)
            });
        });
        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

fn element_kind(element: &Element<'_>) -> ElementKind {
    if element.is_component_like() {
        ElementKind::Component
    } else if element.svelte_name().is_some() {
        ElementKind::Svelte
    } else if VOID_ELEMENTS.contains(element.name) {
        ElementKind::Void
    } else if SVG_ELEMENTS.contains(element.name) {
        ElementKind::Svg
    } else if MATHML_ELEMENTS.contains(element.name) {
        ElementKind::Math
    } else {
        ElementKind::Normal
    }
}

/// Whether the element has nothing in it but whitespace.
fn is_empty(element: &Element<'_>) -> bool {
    element.children.iter().all(|child| match child {
        Node::Text(text) => text.value.trim().is_empty(),
        _ => false,
    })
}

// Upstream's own tables, matched exactly so the classification agrees. The
// void list differs from the markup parser's by `menuitem`, which
// eslint-plugin-svelte counts as void and the Svelte compiler does not; the
// rule follows eslint-plugin-svelte.
static VOID_ELEMENTS: phf::Set<&'static str> = phf::phf_set! {
    "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "menuitem",
    "meta", "param", "source", "track", "wbr",
};

static SVG_ELEMENTS: phf::Set<&'static str> = phf::phf_set! {
    "altGlyph", "altGlyphDef", "altGlyphItem", "animate", "animateColor", "animateMotion",
    "animateTransform", "circle", "clipPath", "color-profile", "cursor", "defs", "desc",
    "discard", "ellipse", "feBlend", "feColorMatrix", "feComponentTransfer", "feComposite",
    "feConvolveMatrix", "feDiffuseLighting", "feDisplacementMap", "feDistantLight",
    "feDropShadow", "feFlood", "feFuncA", "feFuncB", "feFuncG", "feFuncR", "feGaussianBlur",
    "feImage", "feMerge", "feMergeNode", "feMorphology", "feOffset", "fePointLight",
    "feSpecularLighting", "feSpotLight", "feTile", "feTurbulence", "filter", "font", "font-face",
    "font-face-format", "font-face-name", "font-face-src", "font-face-uri", "foreignObject", "g",
    "glyph", "glyphRef", "hatch", "hatchpath", "hkern", "image", "line", "linearGradient",
    "marker", "mask", "mesh", "meshgradient", "meshpatch", "meshrow", "metadata", "missing-glyph",
    "mpath", "path", "pattern", "polygon", "polyline", "radialGradient", "rect", "set",
    "solidcolor", "stop", "svg", "switch", "symbol", "text", "textPath", "tref", "tspan",
    "unknown", "use", "view", "vkern",
};

static MATHML_ELEMENTS: phf::Set<&'static str> = phf::phf_set! {
    "annotation", "annotation-xml", "maction", "math", "merror", "mfrac", "mi", "mmultiscripts",
    "mn", "mo", "mover", "mpadded", "mphantom", "mprescripts", "mroot", "mrow", "ms", "mspace",
    "msqrt", "mstyle", "msub", "msubsup", "msup", "mtable", "mtd", "mtext", "mtr", "munder",
    "munderover", "semantics",
};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HtmlSelfClosing;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let all = || Some(serde_json::json!(["all"]));
        let none = || Some(serde_json::json!(["none"]));
        let normal_always = || Some(serde_json::json!([{ "normal": "always" }]));
        let ignore_all = || {
            Some(serde_json::json!([{
                "void": "ignore", "normal": "ignore", "svg": "ignore",
                "math": "ignore", "component": "ignore", "svelte": "ignore"
            }]))
        };
        let pass = vec![
            ("<div></div>", None, None, path()),
            ("<input />", None, None, path()),
            ("<Widget />", None, None, path()),
            ("<svelte:self />", None, None, path()),
            ("<circle />", None, None, path()),
            ("<mi></mi>", None, None, path()),
            // Not empty, so the rule leaves it alone.
            ("<div>text</div>", None, None, path()),
            ("<Widget>text</Widget>", None, None, path()),
            ("<div />", normal_always(), None, path()),
            ("<div></div>", none(), None, path()),
            ("<input>", none(), None, path()),
            ("<div />", ignore_all(), None, path()),
            ("<Widget></Widget>", ignore_all(), None, path()),
            ("<div />", all(), None, path()),
            // Whitespace-only content still counts as empty, and `<div>` is
            // meant to keep its closing tag.
            ("<div>\n</div>", None, None, path()),
        ];
        let fail = vec![
            ("<div />", None, None, path()),
            ("<input>", None, None, path()),
            ("<Widget></Widget>", None, None, path()),
            ("<svelte:self></svelte:self>", None, None, path()),
            ("<circle></circle>", None, None, path()),
            ("<mi />", None, None, path()),
            ("<div></div>", all(), None, path()),
            ("<div></div>", normal_always(), None, path()),
            ("<Widget />", none(), None, path()),
            // Whitespace-only content is still empty.
            ("<Widget>\n</Widget>", None, None, path()),
        ];

        Tester::new(HtmlSelfClosing::NAME, HtmlSelfClosing::PLUGIN, pass, fail).test_and_snapshot();
    }
}
