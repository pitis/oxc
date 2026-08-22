//! The embedded routing table ([`route`]) and the `FormatDispatcher` assembly shared by every build.
//!
//! Each language maps to a Rust formatter where available;
//! the [`PrettierLanguage`] set goes to the napi-only Prettier Doc→IR channel ([`super::prettier_doc`]) when one is supplied,
//! and is deliberately preserved as-is otherwise (pure Rust build); everything else stays as-is in every build.

use std::{
    any::Any,
    sync::{Arc, OnceLock},
};

use tracing::{debug, debug_span};

use oxc_formatter::{
    CssInJsTemplate, ExpressionRootKind, FragmentContext, JsFormatOptions, SortImportsOptions,
    TypeParameterAmbiguity,
};
use oxc_formatter_core::{
    CoreFormatOptions, DispatchPayload, DispatchRequest, DispatchResponse, EmbeddedIr,
    ExpressionHugsDelimiters, FormatDispatcher, FormatSession, ScriptInComponentFile,
};
use oxc_formatter_core::{FormatOptions, PrinterOptions};
use oxc_formatter_css::{CssFormatOptions, CssFragmentKind, CssVariant};
use oxc_formatter_graphql::GraphqlFormatOptions;
use oxc_formatter_json::{JsonFormatOptions, JsonVariant};
use oxc_formatter_yaml::YamlFormatOptions;
use oxc_span::SourceType;

use crate::core::{
    options::{
        to_oxc_formatter, to_oxc_formatter_css, to_oxc_formatter_graphql, to_oxc_formatter_json,
        to_oxc_formatter_yaml,
    },
    oxfmtrc::FormatConfig,
};

/// The native half of the routing table:
/// a request/fence language routed to [`Route::Native`] parses to its Rust formatter branch here.
pub enum NativeLanguage {
    Graphql,
    /// JS/TS, as a `.svelte` component's `<script>` is.
    Js(SourceType),
    /// A piece of JS/TS that is not a whole program: a `.svelte` component's
    /// `{…}`, a `.vue` component's `{{ … }}` or `:prop` value, the parameter
    /// list a `v-slot` declares. The [`FragmentContext`] says which, and
    /// carries its own input contract — some of them expect the caller to
    /// have wrapped the text already.
    JsFragment(SourceType, FragmentContext),
    /// The fence-derived variant;
    /// the css-in-js typed context overrides it to Scss + placeholders at dispatch time (see the css branch).
    /// `kind` is what the fragment is: a whole stylesheet, or an HTML `style`
    /// attribute's declaration list.
    Css(CssVariant, CssFragmentKind),
    Yaml,
    Json(JsonVariant),
}

/// Languages Prettier still formats for us (no Rust formatter yet).
///
/// [`PrettierDocFallback`] receives this instead of a raw string,
/// so the fallback can never be handed a language the table did not route to it.
/// The set shrinks as Rust ports land (markdown first), and the type disappears with the last port.
#[derive(Clone, Copy)]
pub enum PrettierLanguage {
    Html,
    Angular,
    Markdown,
    /// Handlebars, which a `.vue` file reaches through a custom block
    /// declaring `type="text/x-handlebars-template"`.
    Glimmer,
}

#[cfg(feature = "napi")]
impl PrettierLanguage {
    /// The Prettier `parser` name injected into the options JSON.
    ///
    /// NOTE: language identifiers happen to overlap with some Prettier parser names,
    /// but `oxc_formatter` treats them as generic language names;
    /// this method is the only place mapping EMBEDDED language identifiers to Prettier parsers
    /// (the whole-file Tier 3/4 path has its own filename-keyed map in `core::support`).
    pub fn parser(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Angular => "angular",
            Self::Markdown => "markdown",
            Self::Glimmer => "glimmer",
        }
    }

    /// Whether the Doc→IR conversion must surface `HtmlEmbedMeta`
    /// (`htmlHasMultipleRootElements`) to the embed site.
    pub fn wants_html_meta(self) -> bool {
        matches!(self, Self::Html | Self::Angular)
    }
}

/// Where a language identifier routes.
pub enum Route {
    /// A Rust formatter branch in [`build_dispatcher`]; never re-routed to Prettier.
    Native(NativeLanguage),
    /// Prettier serves it (napi Doc→IR fallback / string channel);
    /// the pure build preserves it as-is.
    Prettier(PrettierLanguage),
    /// No formatter anywhere: the part deliberately stays as-is in every build.
    Unsupported,
}

/// THE routing table: "which formatter serves this language?" answered in one place.
/// [`build_dispatcher`] and the napi string channel's fence routing both consult it,
/// so their notions of who formats what can never drift,
/// and aliases (`"gql"` / `"yml"` / `"md"`) are resolved here and nowhere else.
pub fn route(language: &str) -> Route {
    match language {
        "graphql" | "gql" => Route::Native(NativeLanguage::Graphql),
        "js" => Route::Native(NativeLanguage::Js(SourceType::mjs())),
        // `.with_module(true)` rather than the default unambiguous kind: an
        // embedded body or expression comes from a component, which is always
        // a module, and a fragment of one rarely carries the `import` that
        // would let the parser work that out for itself. Without it `await x`
        // parses as a call on an identifier named `await`.
        "ts" => Route::Native(NativeLanguage::Js(SourceType::ts().with_module(true))),
        // `<script lang="jsx">` / `<script lang="tsx">` in a component. A
        // JSX-free `tsx` block parses fine as plain `ts`, so the distinction
        // only matters for one that is not.
        "jsx" => Route::Native(NativeLanguage::Js(SourceType::jsx())),
        "tsx" => Route::Native(NativeLanguage::Js(SourceType::tsx().with_module(true))),
        "js-expression" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: false,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        // A `{…}` inside a Svelte attribute value is spliced the same way, so
        // it needs the same treatment; the Vue attribute route is separate and
        // keeps the host indent.
        "svelte-attribute-expression" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: true,
                vue_expression: false,
                host_indents: false,
                sequence_parens: true,
            },
        )),
        // Svelte's `{…}` splices the expression between two braces and adds
        // no indent of its own, so a binaryish chain that breaks there has to
        // supply one. Every other expression host wraps and indents the value.
        "svelte-expression" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: false,
                host_indents: false,
                sequence_parens: true,
            },
        )),
        "ts-expression" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: false,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        "js-attribute-expression" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::Expression {
                in_html_attribute: true,
                vue_expression: false,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        "ts-attribute-expression" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: true,
                vue_expression: false,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        // The `.vue` family. Always TypeScript: a template may carry `as`
        // casts and non-null assertions whether or not its `<script>` declares
        // `lang="ts"`, and TS is a superset of what plain JavaScript allows
        // there.
        //
        // `vue-expression` and `vue-attribute-expression` are Prettier's
        // `__vue_expression` — a `{{ … }}` interpolation and a `:prop` value,
        // which get the Vue 2 filter layout for a top-level `|` chain and hug
        // their delimiters when the expression is a template or string
        // literal. A `v-if` value is NOT one of those: it routes to
        // `ts-attribute-expression`, exactly as Prettier sends it to
        // `__ts_expression`.
        //
        // Each comes in two flavours, because which one a template gets is
        // decided by the component's own `<script lang>`, exactly as Prettier
        // decides it. It is not only about `as` casts being left alone: TS and
        // JS disagree on what `foo < bar > (baz)` even is — a call with type
        // arguments, or two comparisons — so parsing a plain-JS component's
        // template as TypeScript can change the layout of valid JavaScript.
        "vue-expression" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: true,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        "vue-js-expression" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: true,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        "vue-attribute-expression" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: true,
                vue_expression: true,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        "vue-js-attribute-expression" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::Expression {
                in_html_attribute: true,
                vue_expression: true,
                host_indents: true,
                sequence_parens: true,
            },
        )),
        // `@click="count++; log()"`, which is statements rather than one
        // expression.
        "vue-event-handler" => {
            Route::Native(NativeLanguage::JsFragment(ts(), FragmentContext::EventHandlerStatements))
        }
        "vue-js-event-handler" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::EventHandlerStatements,
        )),
        // `v-slot="{ item }"` and `<script setup="…">`: a binding list, which
        // the caller wraps as `function _(…) {}`.
        "vue-binding-params" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::FunctionParamsAsBinding,
        )),
        "vue-js-binding-params" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::FunctionParamsAsBinding,
        )),
        // The left of `v-for="(item, index) in items"`, which is a binding
        // list whose parentheses are forced when it declares more than one.
        "vue-v-for-left" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::FunctionParamsAsBindingLhs,
        )),
        "vue-js-v-for-left" => Route::Native(NativeLanguage::JsFragment(
            SourceType::mjs(),
            FragmentContext::FunctionParamsAsBindingLhs,
        )),
        // `<script setup generic="T extends object">`, wrapped by the caller
        // as `type T<…> = any`.
        "vue-generic" => {
            Route::Native(NativeLanguage::JsFragment(ts(), FragmentContext::TypeParameters))
        }
        // The two Svelte slots where a top-level comma is Svelte's own grammar
        // rather than JavaScript's sequence operator: `{#each expr, index}`
        // names the index with it, and `bind:x={get, set}` names the pair of
        // functions. Both arrive whole, so the comma has to be left alone.
        "svelte-each-subject" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: false,
                host_indents: false,
                sequence_parens: false,
            },
        )),
        // A `bind:` value carries an indent from the embed site, unlike the
        // other Svelte routes.
        "svelte-bind-value" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::Expression {
                in_html_attribute: false,
                vue_expression: false,
                host_indents: true,
                sequence_parens: false,
            },
        )),
        // A `{#snippet name(params)}` header, wrapped by the caller as
        // `function name(params) {}`: it is a function signature, and its
        // parameters are parameters rather than arguments.
        // `{#each … as PATTERN}` / `{:then PATTERN}`: a binding, re-serialized
        // the way `prettier-plugin-svelte`'s `expandNode` does.
        "svelte-binding-pattern" => Route::Native(NativeLanguage::JsFragment(
            ts(),
            FragmentContext::BindingPatternAsWritten,
        )),
        // `{const x = 1}` / `{let a = 1, b = 2}`: a declaration, not an
        // expression. Always TypeScript for the same reason the other Svelte
        // routes are.
        "svelte-declaration-tag" => {
            Route::Native(NativeLanguage::JsFragment(ts(), FragmentContext::VariableDeclarators))
        }
        "svelte-snippet-signature" => {
            Route::Native(NativeLanguage::JsFragment(ts(), FragmentContext::FunctionSignature))
        }
        "css" => Route::Native(NativeLanguage::Css(CssVariant::Css, CssFragmentKind::Stylesheet)),
        "scss" => Route::Native(NativeLanguage::Css(CssVariant::Scss, CssFragmentKind::Stylesheet)),
        "less" => Route::Native(NativeLanguage::Css(CssVariant::Less, CssFragmentKind::Stylesheet)),
        // `style="color: red"`: declarations with no rule around them, laid
        // out to fit on the attribute's line.
        "css-style-attribute" => {
            Route::Native(NativeLanguage::Css(CssVariant::Css, CssFragmentKind::StyleAttribute))
        }
        "yaml" | "yml" => Route::Native(NativeLanguage::Yaml),
        "json" => Route::Native(NativeLanguage::Json(JsonVariant::Json)),
        "jsonc" => Route::Native(NativeLanguage::Json(JsonVariant::Jsonc)),
        "json5" => Route::Native(NativeLanguage::Json(JsonVariant::Json5)),
        "html" => Route::Prettier(PrettierLanguage::Html),
        "angular" => Route::Prettier(PrettierLanguage::Angular),
        "markdown" | "md" => Route::Prettier(PrettierLanguage::Markdown),
        "glimmer" => Route::Prettier(PrettierLanguage::Glimmer),
        _ => Route::Unsupported,
    }
}

/// Per-root context shared by every embedded service (dispatcher, string embedder, Tailwind sorter):
/// the host file's resolved config plus lazily-mapped per-language options.
///
/// Language options are NOT built up front: an embed-free file pays only for empty cells,
/// and a host where every language is embeddable (Markdown-scale) maps exactly the languages that actually appear,
/// once each (`OnceLock` memoizes and is safe under the rayon-parallel format runs).
pub struct ResolvedDispatchConfig {
    /// Resolved config of the HOST file (its overrides / editorconfig applied).
    /// Embedded children inherit it, mirroring Prettier's `textToDoc` (parent-options spread);
    /// never a re-resolution for a virtual path.
    config: Arc<FormatConfig>,
    /// Core options validated once by the config-resolution gate (`options::validate`).
    /// Holding them pre-validated is what lets the per-language mappers be infallible.
    core: CoreFormatOptions,
    /// Import sorting for an embedded `<script>`, which only a root whose
    /// file *is* that script passes on. See [`Self::js_options`].
    sort_imports: Option<SortImportsOptions>,
    graphql: OnceLock<GraphqlFormatOptions>,
    js: OnceLock<JsFormatOptions>,
    /// One cell per [`CssVariant`]: JSDoc fences dispatch css/scss/less as-is, while css-in-js always uses Scss.
    css: [OnceLock<CssFormatOptions>; 3],
    yaml: OnceLock<YamlFormatOptions>,
    /// One cell per fence-reachable [`JsonVariant`] (json / jsonc / json5; `JsonStringify` is `package.json`-only).
    json: [OnceLock<JsonFormatOptions>; 3],
    /// The options handed to Prettier; see [`PrettierOptions`].
    #[cfg(feature = "napi")]
    prettier: PrettierOptions,
}

/// The lazily-built options JSON handed to Prettier (+ plugins),
/// consumed by the Doc→IR / string paths and the Tailwind sorter.
/// `path` is an ingredient, not a sibling datum: it becomes the JSON's `filepath` at last
/// (see [`crate::core::options::build_prettier_options`]).
///
/// NOTE: The late merge is load-bearing: the JSON must derive from the RESOLVED per-file config
/// (a pre-built Value loses overrides, #18246), and `path` is the one per-file ingredient,
/// keeping it out of the config is what keeps the config shareable across files.
/// Lazy so an embed-free file never builds the JSON at all.
#[cfg(feature = "napi")]
#[derive(Default)]
struct PrettierOptions {
    path: std::path::PathBuf,
    options: OnceLock<serde_json::Value>,
}

impl ResolvedDispatchConfig {
    /// `core` is the pre-validated bundle carried from the config-resolution gate (`options::validate`);
    /// it never gets re-derived here.
    /// Private so [`Self::for_root`] stays the only construction recipe.
    fn new(
        config: Arc<FormatConfig>,
        core: CoreFormatOptions,
        sort_imports: Option<SortImportsOptions>,
    ) -> Self {
        Self {
            config,
            core,
            sort_imports,
            graphql: OnceLock::new(),
            js: OnceLock::new(),
            css: [OnceLock::new(), OnceLock::new(), OnceLock::new()],
            yaml: OnceLock::new(),
            json: [OnceLock::new(), OnceLock::new(), OnceLock::new()],
            #[cfg(feature = "napi")]
            prettier: PrettierOptions::default(),
        }
    }

    /// The one construction recipe for a root formatter run at `path`:
    /// [`Self::new`] plus the napi-only path recording
    /// (the pure build has no JS-side consumers, so `path` goes unused there).
    pub fn for_root(
        config: &Arc<FormatConfig>,
        core: CoreFormatOptions,
        sort_imports: Option<SortImportsOptions>,
        path: &std::path::Path,
    ) -> Arc<Self> {
        let dispatch_config = Self::new(Arc::clone(config), core, sort_imports);
        #[cfg(feature = "napi")]
        let dispatch_config = dispatch_config.with_path(path.to_path_buf());
        #[cfg(not(feature = "napi"))]
        let _ = path;
        Arc::new(dispatch_config)
    }

    /// Assembles the root's `FormatDispatcher` behind the off-gate:
    /// `None` under `embeddedLanguageFormatting: off`,
    /// so a root cannot install the registry without honoring the off-semantics.
    /// `fallback` is the one build-dependent datum (the napi Prettier Doc→IR path).
    pub fn root_dispatcher(
        self: &Arc<Self>,
        fallback: Option<PrettierDocFallback>,
    ) -> Option<FormatDispatcher> {
        self.is_embedded_formatting_enabled().then(|| build_dispatcher(Arc::clone(self), fallback))
    }

    /// The single off-predicate: [`Self::root_dispatcher`] and both build's `services::for_root` definitions consult it,
    /// so the off-semantics can never diverge between channels or builds.
    pub fn is_embedded_formatting_enabled(&self) -> bool {
        self.config.is_embedded_formatting_enabled()
    }

    pub fn graphql_options(&self) -> GraphqlFormatOptions {
        *self.graphql.get_or_init(|| to_oxc_formatter_graphql(&self.config, self.core))
    }

    pub fn css_options(&self, variant: CssVariant) -> CssFormatOptions {
        let cell = match variant {
            CssVariant::Css => &self.css[0],
            CssVariant::Scss => &self.css[1],
            CssVariant::Less => &self.css[2],
        };
        *cell.get_or_init(|| to_oxc_formatter_css(&self.config, self.core, variant))
    }

    /// Options for an embedded `<script>`.
    ///
    /// Import sorting is a whole-*module* transform, so it is passed on only
    /// by a root that says its embedded script is the module: a `.svelte`
    /// component's, whose `<script>` holds the file's only imports. A CSS
    /// file's css-in-js and a Markdown fence pass `None` — there is no module
    /// there to sort.
    pub fn js_options(&self) -> JsFormatOptions {
        self.js
            .get_or_init(|| to_oxc_formatter(&self.config, self.core, self.sort_imports.clone()))
            .clone()
    }

    pub fn yaml_options(&self) -> YamlFormatOptions {
        *self.yaml.get_or_init(|| to_oxc_formatter_yaml(&self.config, self.core))
    }

    pub fn json_options(&self, variant: JsonVariant) -> JsonFormatOptions {
        let cell = match variant {
            JsonVariant::Json => &self.json[0],
            JsonVariant::Jsonc => &self.json[1],
            JsonVariant::Json5 => &self.json[2],
            JsonVariant::JsonStringify => {
                unreachable!(
                    "JsonStringify is the package.json pipeline's variant, never dispatched"
                )
            }
        };
        *cell.get_or_init(|| to_oxc_formatter_json(&self.config, self.core, variant))
    }

    /// Printer options from the shared resolved core bundle;
    /// the fence adapter ([`super::jsdoc_fence`]) derives its per-fence options from these
    /// (width overridden to the fence's effective width).
    pub fn print_options(&self) -> PrinterOptions {
        self.core.as_print_options()
    }
}

/// Napi-only methods: the [`PrettierOptions`] accessors and the Tailwind predicate.
#[cfg(feature = "napi")]
impl ResolvedDispatchConfig {
    /// Sets the host file path for `filepath` injection into [`Self::prettier_options`];
    /// chained by [`Self::for_root`].
    fn with_path(mut self, path: std::path::PathBuf) -> Self {
        self.prettier.path = path;
        self
    }

    /// The single Tailwind predicate, same pattern as
    /// [`Self::is_embedded_formatting_enabled`]: every sorter-assembly site consults it
    /// (the sorter is napi-only; the pure build has no JS-side class order source).
    pub fn is_tailwind_enabled(&self) -> bool {
        self.config.is_tailwind_enabled()
    }

    /// The options JSON handed to Prettier
    /// (see [`crate::core::options::build_prettier_options`]).
    pub fn prettier_options(&self) -> &serde_json::Value {
        self.prettier.options.get_or_init(|| {
            crate::core::options::build_prettier_options(&self.config, &self.prettier.path)
        })
    }
}

/// Fallback invoked for [`Route::Prettier`] languages.
/// Same shape as `FormatDispatcher` minus the request envelope
/// (the Doc path consumes neither `input_kind` nor `parent_context` today;
/// re-examine if it ever serves envelope-bearing inputs).
///
/// Assembled only in napi builds ([`super::prettier_doc`]);
/// the pure Rust build passes `None` and these languages are deliberately preserved as-is.
pub type PrettierDocFallback = Arc<
    dyn for<'a> Fn(
            &FormatSession<'a>,
            PrettierLanguage,
            &str,
        ) -> Result<DispatchResponse<'a>, String>
        + Send
        + Sync,
>;

/// Build the `FormatDispatcher` carried by the root's `FormatSession`:
/// Rust formatters for the [`Route::Native`] branches (never re-routed to Prettier, even on failure),
/// `fallback` for the [`Route::Prettier`] set, deliberate preservation for the rest.
pub fn build_dispatcher(
    dispatch_config: Arc<ResolvedDispatchConfig>,
    fallback: Option<PrettierDocFallback>,
) -> FormatDispatcher {
    Arc::new(move |session: &FormatSession<'_>, request: DispatchRequest<'_>| {
        let text = request.text;
        match route(request.language) {
            Route::Native(NativeLanguage::Graphql) => Ok(format_native("graphql", || {
                oxc_formatter_graphql::format_to_ir(
                    session,
                    text,
                    dispatch_config.graphql_options(),
                )
            })),
            Route::Native(NativeLanguage::Css(variant, kind)) => {
                // css-in-js (typed `CssInJsTemplate` context) is always parsed as SCSS with `${}` placeholder markers.
                // Any other caller gets the fence/request language's variant and the kind its route named.
                let (variant, kind) = if request
                    .parent_context
                    .is_some_and(|c| c.downcast_ref::<CssInJsTemplate>().is_some())
                {
                    (CssVariant::Scss, CssFragmentKind::Template)
                } else {
                    (variant, kind)
                };
                Ok(format_native("css", || {
                    oxc_formatter_css::format_to_ir(
                        session,
                        text,
                        dispatch_config.css_options(variant),
                        kind,
                    )
                }))
            }
            Route::Native(NativeLanguage::Js(source_type)) => {
                // A component's `<script>` is not a file of its own, and
                // Prettier's trailing-comma rule for a lone type parameter
                // keys on the file's extension rather than the script's.
                let type_parameters =
                    if request.parent_context.is_some_and(<dyn Any>::is::<ScriptInComponentFile>) {
                        TypeParameterAmbiguity::NeedsTrailingComma
                    } else {
                        TypeParameterAmbiguity::None
                    };
                Ok(format_native("js", || {
                    oxc_formatter::format_to_ir(
                        session,
                        text,
                        source_type,
                        dispatch_config.js_options(),
                        type_parameters,
                    )
                }))
            }
            Route::Native(NativeLanguage::JsFragment(source_type, context)) => {
                Ok(debug_span!("oxfmt::embed::format_to_ir", language = "js-fragment").in_scope(
                    || {
                        let result = oxc_formatter::format_fragment_to_ir(
                            session,
                            text,
                            source_type,
                            dispatch_config.js_options(),
                            context,
                        );
                        match result {
                            Ok((embedded, expression_root)) => {
                                // The hug-vs-expand answer Prettier gets from
                                // its `__onHtmlBindingRoot` hook. Svelte
                                // ignores it; Vue's attribute and
                                // interpolation layouts turn on it.
                                let mut payload = DispatchPayload::from(embedded);
                                payload.child_context = Some(Box::new(hugs_delimiters(
                                    expression_root,
                                    context,
                                )));
                                DispatchResponse::Formatted(payload)
                            }
                            Err(err) => {
                                debug!(
                                    "native 'js-fragment' format_to_ir failed, part stays as-is: {err}"
                                );
                                DispatchResponse::PreserveOriginal
                            }
                        }
                    },
                ))
            }
            Route::Native(NativeLanguage::Yaml) => Ok(format_native("yaml", || {
                oxc_formatter_yaml::format_to_ir(session, text, dispatch_config.yaml_options())
            })),
            Route::Native(NativeLanguage::Json(variant)) => Ok(format_native("json", || {
                oxc_formatter_json::format_to_ir(
                    session,
                    text,
                    dispatch_config.json_options(variant),
                )
            })),

            // Prettier-served languages: Doc→IR fallback when available (napi),
            // deliberate skip otherwise (pure build).
            Route::Prettier(language) => {
                if let Some(fallback) = &fallback {
                    fallback(session, language, text)
                } else {
                    debug!(
                        "No fallback for Prettier language '{}' in this build, part stays as-is",
                        request.language
                    );
                    Ok(DispatchResponse::PreserveOriginal)
                }
            }

            // A language without a formatter is a deliberate skip, in every build.
            Route::Unsupported => {
                debug!("No formatter for language '{}', part stays as-is", request.language);
                Ok(DispatchResponse::PreserveOriginal)
            }
        }
    })
}

/// TypeScript in module mode, which is what every embedded JS fragment from a
/// component is: it comes from a module, and a fragment of one rarely carries
/// the `import` that would let the parser work that out for itself. Without it
/// `await x` parses as a call on an identifier named `await`.
fn ts() -> SourceType {
    SourceType::ts().with_module(true)
}

/// Whether the fragment's own brackets can stand in for the break its host
/// would otherwise put around it — Prettier's `shouldHugJsExpression`.
///
/// A template or string literal only hugs for the Vue expression flavour,
/// where the value really is inside quotes; object and array literals hug
/// everywhere.
fn hugs_delimiters(
    root: Option<ExpressionRootKind>,
    context: FragmentContext,
) -> ExpressionHugsDelimiters {
    let Some(root) = root else {
        // A context that is not an expression never reaches Prettier's hook,
        // leaving its `shouldHug` at the `true` it starts as.
        return ExpressionHugsDelimiters(true);
    };
    let hugs = match root {
        ExpressionRootKind::ObjectExpression | ExpressionRootKind::ArrayExpression => true,
        ExpressionRootKind::TemplateLiteral | ExpressionRootKind::StringLiteral => {
            matches!(context, FragmentContext::Expression { vue_expression: true, .. })
        }
        ExpressionRootKind::Other => false,
    };
    ExpressionHugsDelimiters(hugs)
}

/// Runs one native branch: a parse failure is a deliberate skip
/// (the embedded part stays as-is), never an operational error.
/// The `From<EmbeddedIr>` conversion carries the child's Tailwind classes.
fn format_native<'a, E: std::fmt::Display>(
    language: &'static str,
    format_to_ir: impl FnOnce() -> Result<EmbeddedIr<'a>, E>,
) -> DispatchResponse<'a> {
    debug_span!("oxfmt::embed::format_to_ir", language).in_scope(|| match format_to_ir() {
        Ok(embedded) => DispatchResponse::Formatted(embedded.into()),
        Err(err) => {
            debug!("native '{language}' format_to_ir failed, part stays as-is: {err}");
            DispatchResponse::PreserveOriginal
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use oxc_allocator::Allocator;
    use oxc_formatter_core::{
        CoreFormatOptions, DispatchRequest, DispatchResponse, FormatSession, InputKind,
        SessionServices,
    };

    use super::{ResolvedDispatchConfig, build_dispatcher};
    use crate::core::oxfmtrc::FormatConfig;

    fn dispatch_config() -> Arc<ResolvedDispatchConfig> {
        Arc::new(ResolvedDispatchConfig::new(
            Arc::new(FormatConfig::default()),
            CoreFormatOptions::default(),
            None,
        ))
    }

    /// Every fence language the routing table claims as native must format
    /// WITHOUT a fallback installed
    /// (an accidentally dropped [`super::route`] entry would fall through to `PreserveOriginal` and fail here).
    #[test]
    fn every_native_language_dispatches() {
        let allocator = Allocator::default();
        let session = FormatSession::with_services(
            &allocator,
            InputKind::PhysicalFile,
            SessionServices {
                dispatcher: Some(build_dispatcher(dispatch_config(), None)),
                ..SessionServices::default()
            },
        );

        for language in
            ["graphql", "gql", "css", "scss", "less", "yaml", "yml", "json", "jsonc", "json5"]
        {
            let text = match language {
                "graphql" | "gql" => "{ a }",
                "css" | "scss" | "less" => "a { color: red }",
                "yaml" | "yml" => "a: 1",
                "json" | "jsonc" | "json5" => "{ \"a\": 1 }",
                other => panic!("no sample input for native language '{other}'"),
            };
            let response = session.dispatch(DispatchRequest {
                language,
                text,
                input_kind: InputKind::Fragment,
                parent_context: None,
            });
            assert!(
                matches!(response, Ok(DispatchResponse::Formatted(_))),
                "language '{language}' did not dispatch natively"
            );
        }
    }

    /// Pure-build criterion: the native registry dispatches YAML with no fallback installed.
    #[test]
    fn native_yaml_dispatch_works_without_fallback() {
        let allocator = Allocator::default();
        let session = FormatSession::with_services(
            &allocator,
            InputKind::PhysicalFile,
            SessionServices {
                dispatcher: Some(build_dispatcher(dispatch_config(), None)),
                ..SessionServices::default()
            },
        );

        let response = session.dispatch(DispatchRequest {
            language: "yaml",
            text: "a:   1",
            input_kind: InputKind::Fragment,
            parent_context: None,
        });
        assert!(matches!(response, Ok(DispatchResponse::Formatted(_))));
    }

    /// Both non-native routes preserve without a fallback installed:
    /// a Prettier-served language (pure-build behavior) and a fully unsupported one.
    #[test]
    fn non_native_language_without_fallback_preserves_original() {
        let allocator = Allocator::default();
        let session = FormatSession::with_services(
            &allocator,
            InputKind::PhysicalFile,
            SessionServices {
                dispatcher: Some(build_dispatcher(dispatch_config(), None)),
                ..SessionServices::default()
            },
        );

        for (language, text) in [("html", "<div></div>"), ("toml", "a = 1")] {
            let response = session.dispatch(DispatchRequest {
                language,
                text,
                input_kind: InputKind::Fragment,
                parent_context: None,
            });
            assert!(
                matches!(response, Ok(DispatchResponse::PreserveOriginal)),
                "language '{language}' should be preserved without a fallback"
            );
        }
    }
}
