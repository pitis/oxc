use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use tracing::{instrument, warn};

use oxc_allocator::Allocator;
use oxc_formatter::FragmentContext;
use oxc_formatter_core::{FormatElement, FormatSession, InputKind};
use oxc_span::SourceType;

use crate::{
    core::{
        EmbeddedCallbackResolved, ExternalServices, JsFormatEmbeddedCb, JsFormatEmbeddedDocCb,
        JsFormatFileCb, JsSortTailwindClassesCb,
        embed::{self, dispatcher::ResolvedDispatchConfig},
        options::to_oxc_formatter_svelte,
        oxfmtrc::FormatConfig,
        resolve_for_embedded_js,
    },
    prettier_compat::to_prettier_doc,
};

/// Fragment kind for embedded JS/TS contexts.
#[derive(Clone, Copy, Debug)]
enum FragmentKind {
    /// `v-for` left-hand side: `(item, index) in items` → formats `item, index` part.
    VueForBindingLeft,
    /// `v-slot` / slot binding: `{ item }` → formats the destructured parameters.
    VueBindings,
    /// `<script generic="T extends Foo">` → formats type parameters without angle brackets.
    VueScriptGeneric,
    /// A bare expression inside a double-quoted attribute, from the plain
    /// `__(js|ts)_expression` parsers: `v-if`/`v-show` and other simple
    /// directives, the `v-for` right-hand side, and the expression form of
    /// `v-on`/`@event`.
    ExpressionAttribute,
    /// A bare expression inside an interpolation from a non-Vue parser.
    /// (Currently unreachable — Vue interpolations arrive as
    /// [`FragmentKind::VueExpressionInterpolation`] — kept for symmetry.)
    ExpressionInterpolation,
    /// A bare expression inside a double-quoted attribute, from the
    /// `__vue(_ts)_expression` parsers: `v-bind`/`:prop` values.
    /// Enables the Vue 2 filter layout (filters are only valid in `v-bind`
    /// and interpolations).
    VueExpressionAttribute,
    /// A bare expression inside an interpolation: `{{ ... }}`, from the
    /// `__vue(_ts)_expression` parsers. Enables the Vue 2 filter layout.
    VueExpressionInterpolation,
    /// Statement(s) of an inline event handler: `v-on`/`@event` values that
    /// did not parse as a single expression.
    EventHandler,
}

/// A classified failure of an embedded-JS/TS `textToDoc()` call.
///
/// Prettier's `printEmbeddedLanguage()` swallows every `textToDoc()` failure in
/// production (the fragment is then emitted unformatted), so the failure has to
/// travel back as *data* for the JS side to surface it on the format result.
/// The distinction matters to users: [`Self::Syntax`] is their own broken input,
/// [`Self::Internal`] is an oxfmt bug that should be reported.
enum EmbedFailure {
    /// The embedded script/expression did not parse.
    Syntax(String),
    /// oxfmt itself failed: malformed plugin payload, config that no longer
    /// validates, an IR → Prettier `Doc` conversion bug, or an unmapped
    /// pseudo-parser.
    Internal(String),
}

impl EmbedFailure {
    fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Syntax(_) => "syntax",
            Self::Internal(_) => "internal",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Syntax(message) | Self::Internal(message) => message,
        }
    }

    /// The JSON payload handed to the JS side (`text-to-doc.ts`).
    fn to_json(&self) -> Value {
        serde_json::json!({ "error": { "kind": self.kind(), "message": self.message() } })
    }
}

/// `js_text_to_doc()` implementation for NAPI API.
///
/// Prettier's `printEmbeddedLanguage()` silently swallows errors thrown from
/// `textToDoc()` in production, so failures are returned as data instead:
/// the JSON payload carries an `error: { kind, message }` object that the JS
/// side turns into a user-visible warning on the format result.
///
/// Returns `None` only when the failure payload itself cannot be serialized.
#[instrument(
    level = "debug",
    name = "oxfmt::text_to_doc",
    skip_all,
    fields(source_ext = %source_ext, parent_context = %parent_context)
)]
pub fn run(
    source_ext: &str,
    source_text: &str,
    oxfmt_plugin_options_json: &str,
    parent_context: &str,
    format_file_cb: JsFormatFileCb,
    format_embedded_cb: JsFormatEmbeddedCb,
    format_embedded_doc_cb: JsFormatEmbeddedDocCb,
    sort_tailwind_classes_cb: JsSortTailwindClassesCb,
) -> Option<String> {
    // Embedded text belongs to the host file (`.vue`, `.md`, ...),
    // so the `SourceType` carries no file extension of its own.
    // `source_ext` selects the parse grammar only,
    // and extension-keyed formatter rules (e.g. the `.mts`/`.cts` trailing comma reservation) must not fire from it.
    //
    // The JS side owns the grammar resolution (including the `lang="tsx"` scan for Vue, see `hasTsxScriptBlock` in `apis.ts`),
    // so there is no parse retry here: a block
    // that fails to parse under its declared grammar is left unformatted
    // (`textToDoc()` error → Prettier keeps the original text).
    // A `.svelte` code block is a whole component, not a JS/TS snippet: it has
    // no parse grammar to select and no fragment kind, so it goes straight to
    // its own formatter.
    if source_ext == "svelte" {
        let result = run_svelte(
            source_text,
            oxfmt_plugin_options_json,
            format_file_cb,
            format_embedded_cb,
            format_embedded_doc_cb,
            sort_tailwind_classes_cb,
        );
        return Some(match result {
            Ok(doc_json) => match serde_json::to_string(&doc_json) {
                Ok(json) => json,
                Err(err) => failure_payload(
                    &EmbedFailure::internal(format!("Doc JSON serialization failed: {err}")),
                    parent_context,
                    None,
                ),
            },
            Err(failure) => failure_payload(&failure, parent_context, None),
        });
    }

    let source_type = match source_ext {
        "jsx" => SourceType::unambiguous().with_jsx(true),
        "ts" => SourceType::ts(),
        "tsx" => SourceType::tsx(),
        _ => {
            unreachable!(
                "text-to-doc.ts should pass `source_ext` as one of 'jsx', 'ts', 'tsx' or 'svelte'"
            )
        }
    };

    let fragment_kind = match parent_context {
        "vue-for-binding-left" => Some(FragmentKind::VueForBindingLeft),
        "vue-bindings" => Some(FragmentKind::VueBindings),
        "vue-script-generic" => Some(FragmentKind::VueScriptGeneric),
        "expression-attribute" => Some(FragmentKind::ExpressionAttribute),
        "expression-interpolation" => Some(FragmentKind::ExpressionInterpolation),
        "vue-expression-attribute" => Some(FragmentKind::VueExpressionAttribute),
        "vue-expression-interpolation" => Some(FragmentKind::VueExpressionInterpolation),
        "event-handler" => Some(FragmentKind::EventHandler),
        // Full-program contexts: "vue-script", "svelte-script", "markdown", "mdx",
        // and any other host parser name forwarded by `detectParentContext()`.
        //
        // A `__`-prefixed name is one of Prettier's pseudo-parsers, which always
        // request a *fragment*. Reaching here means the plugin registered a
        // pseudo-parser that was never mapped to a `FragmentKind`; formatting it
        // as a full program would silently produce wrong output, so fail loudly.
        unmapped if unmapped.starts_with("__") => {
            let failure = EmbedFailure::internal(format!(
                "unmapped pseudo-parser context `{unmapped}`; embedded fragment left unformatted"
            ));
            return Some(failure_payload(&failure, parent_context, None));
        }
        _ => None,
    };

    let result = if let Some(kind) = fragment_kind {
        run_fragment(source_type, source_text, oxfmt_plugin_options_json, kind)
    } else {
        run_full(
            source_type,
            source_text,
            oxfmt_plugin_options_json,
            format_file_cb,
            format_embedded_cb,
            format_embedded_doc_cb,
            sort_tailwind_classes_cb,
        )
    };

    let doc_json = match result {
        Ok(doc_json) => doc_json,
        Err(failure) => return Some(failure_payload(&failure, parent_context, fragment_kind)),
    };

    match serde_json::to_string(&doc_json) {
        Ok(json) => Some(json),
        Err(err) => {
            let failure = EmbedFailure::internal(format!("Doc JSON serialization failed: {err}"));
            Some(failure_payload(&failure, parent_context, fragment_kind))
        }
    }
}

/// Serialize a failure for the JS side.
///
/// This is the single funnel for every `run_full` / `run_fragment` swallow site,
/// so the log lives here: `warn!` (was `debug!`) with the `syntax` vs `internal`
/// kind, which makes `OXC_LOG=oxfmt=warn` enough instead of full `debug`. The
/// user-visible signal is the returned payload, not this log.
///
/// The expression / event-handler fragment kinds additionally report a syntax
/// failure as `parseError` data: Prettier's `v-on` printer needs a recognizable
/// Babel-shaped syntax error to fall back from the expression form to the
/// event-handler (statements) form.
fn failure_payload(
    failure: &EmbedFailure,
    parent_context: &str,
    fragment_kind: Option<FragmentKind>,
) -> String {
    warn!(
        kind = failure.kind(),
        parent_context,
        "oxfmt could not format an embedded JS/TS fragment: {}",
        failure.message()
    );

    let mut payload = failure.to_json();
    let needs_parse_error_marker = matches!(failure, EmbedFailure::Syntax(_))
        && matches!(
            fragment_kind,
            Some(
                FragmentKind::ExpressionAttribute
                    | FragmentKind::ExpressionInterpolation
                    | FragmentKind::VueExpressionAttribute
                    | FragmentKind::VueExpressionInterpolation
                    | FragmentKind::EventHandler
            )
        );
    if needs_parse_error_marker && let Some(object) = payload.as_object_mut() {
        object.insert("parseError".to_string(), Value::Bool(true));
    }

    // A `{ error: { kind, message } }` object is always serializable; the
    // fallback only exists so this helper stays infallible.
    serde_json::to_string(&payload).unwrap_or_else(|_| {
        r#"{"error":{"kind":"internal","message":"failure payload serialization failed"}}"#
            .to_string()
    })
}

// ---

/// Full mode:
/// - Format entire source as IR
/// - Convert IR to Prettier Doc
///
/// NOTE: Why we need to convert IR to Doc instead of just splitting by lines:
/// A simple line-splitting approach might seem sufficient and can cover most cases,
/// but it fails to handle newlines that appear within string, such as `TemplateLiteral`.
///
/// This is critical for `vueIndentScriptAndStyle: true`, (Prettier wraps the `<script>` content with `indent()`)
/// `literalline` (used for template literal content) is not affected by `indent()`,
/// while `hardline` (used for normal code) is.
#[instrument(level = "debug", name = "oxfmt::text_to_doc::full", skip_all, fields(?source_type))]
fn run_full(
    source_type: SourceType,
    source_text: &str,
    oxfmt_plugin_options_json: &str,
    format_file_cb: JsFormatFileCb,
    format_embedded_cb: JsFormatEmbeddedCb,
    format_embedded_doc_cb: JsFormatEmbeddedDocCb,
    sort_tailwind_classes_cb: JsSortTailwindClassesCb,
) -> Result<Value, EmbedFailure> {
    let external_services = ExternalServices::new(
        format_file_cb,
        format_embedded_cb,
        format_embedded_doc_cb,
        sort_tailwind_classes_cb,
    );

    // The TSFNs must be released on every exit path, including the failure ones.
    let result =
        format_full(&external_services, source_type, source_text, oxfmt_plugin_options_json);
    external_services.cleanup();
    result
}

fn format_full(
    external_services: &ExternalServices,
    source_type: SourceType,
    source_text: &str,
    oxfmt_plugin_options_json: &str,
) -> Result<Value, EmbedFailure> {
    // Tailwind paths in the payload are already absolute (resolved by the host before serialization),
    // so no `cwd` is threaded through here.
    let (config, parent_filepath) = parse_payload(oxfmt_plugin_options_json)?;

    let EmbeddedCallbackResolved { format_options, config, core, parent_filepath } =
        resolve_for_embedded_js(config, parent_filepath).map_err(|err| {
            EmbedFailure::internal(format!(
                "`_oxfmtPluginOptionsJson` carries invalid config: {err}"
            ))
        })?;

    // Per-language options (and the Prettier options JSON with the Tailwind payload)
    // are mapped lazily at dispatch time; `core` was validated during resolution.
    let dispatch_config = ResolvedDispatchConfig::for_root(&config, core, None, &parent_filepath);

    let services = embed::services::for_root(external_services, &dispatch_config);

    let allocator = Allocator::default();
    let session = FormatSession::with_services(
        &allocator,
        // A Vue/Svelte `<script>` block is a complete document the host passes as embedded input,
        // never the owner of file envelopes (BOM / front matter).
        InputKind::VirtualDocument,
        services,
    );
    let formatted = match tokio::task::block_in_place(|| {
        oxc_formatter::format_with_session(&session, source_text, source_type, *format_options)
    }) {
        Ok(formatted) => formatted,
        // `oxc_formatter::format()` only fails when the source does not parse.
        Err(err) => return Err(EmbedFailure::Syntax(format!("{err}"))),
    };

    let (elements, sorted_tailwind_classes) =
        formatted.into_final_document().into_elements_and_tailwind_classes();

    to_prettier_doc::format_elements_to_prettier_doc(elements, &sorted_tailwind_classes).map_err(
        |err| {
            EmbedFailure::internal(format!("Formatter IR to Prettier Doc conversion failed: {err}"))
        },
    )
}

// ---

/// A whole `.svelte` component embedded in another document — the ` ```svelte `
/// code block a Markdown or MDX file may contain.
///
/// Same shape as [`run_full`], with `oxc_formatter_svelte` in place of the JS
/// formatter: the same services, so a `<script>` inside the block still reaches
/// `oxc_formatter` and a `<style>` still reaches `oxc_formatter_css`.
#[instrument(level = "debug", name = "oxfmt::text_to_doc::svelte", skip_all)]
fn run_svelte(
    source_text: &str,
    oxfmt_plugin_options_json: &str,
    format_file_cb: JsFormatFileCb,
    format_embedded_cb: JsFormatEmbeddedCb,
    format_embedded_doc_cb: JsFormatEmbeddedDocCb,
    sort_tailwind_classes_cb: JsSortTailwindClassesCb,
) -> Result<Value, EmbedFailure> {
    let external_services = ExternalServices::new(
        format_file_cb,
        format_embedded_cb,
        format_embedded_doc_cb,
        sort_tailwind_classes_cb,
    );
    // The TSFNs must be released on every exit path, including the failure ones.
    let result = format_svelte(&external_services, source_text, oxfmt_plugin_options_json);
    external_services.cleanup();
    result
}

fn format_svelte(
    external_services: &ExternalServices,
    source_text: &str,
    oxfmt_plugin_options_json: &str,
) -> Result<Value, EmbedFailure> {
    let (config, parent_filepath) = parse_payload(oxfmt_plugin_options_json)?;
    // `resolve_for_embedded_js` is the shared validation gate for an embedded
    // payload, not a JS-only step; only its `format_options` are unused here.
    let EmbeddedCallbackResolved { format_options: js_options, config, core, parent_filepath } =
        resolve_for_embedded_js(config, parent_filepath).map_err(|err| {
            EmbedFailure::internal(format!(
                "`_oxfmtPluginOptionsJson` carries invalid config: {err}"
            ))
        })?;

    let format_options = to_oxc_formatter_svelte(&config, core);
    // A component's `<script>` holds its imports, in a code block as much as
    // in a file of its own.
    let dispatch_config = ResolvedDispatchConfig::for_root(
        &config,
        core,
        js_options.sort_imports.clone(),
        &parent_filepath,
    );
    let services = embed::services::for_root(external_services, &dispatch_config);

    let allocator = Allocator::default();
    let session = FormatSession::with_services(
        &allocator,
        // A code block is a complete document the host passes as embedded
        // input, never the owner of file envelopes (BOM / front matter).
        InputKind::VirtualDocument,
        services,
    );
    let formatted = match tokio::task::block_in_place(|| {
        oxc_formatter_svelte::format_with_session(&session, source_text, format_options)
    }) {
        Ok(formatted) => formatted,
        // The only failure is markup the Svelte compiler would reject, which
        // Prettier then leaves as the author wrote it.
        Err(err) => return Err(EmbedFailure::Syntax(format!("{err}"))),
    };

    let (elements, sorted_tailwind_classes) =
        formatted.into_final_document().into_elements_and_tailwind_classes();

    // A component ends with the newline a file wants; a code block does not,
    // because the Markdown printer writes the fence's own line after this doc.
    let mut end = elements.len();
    while end > 0 && matches!(elements[end - 1], FormatElement::Line(_)) {
        end -= 1;
    }

    to_prettier_doc::format_elements_to_prettier_doc(&elements[..end], &sorted_tailwind_classes)
        .map_err(|err| {
            EmbedFailure::internal(format!("Formatter IR to Prettier Doc conversion failed: {err}"))
        })
}

// ---

/// Fragment mode:
/// - Parse pre-wrapped source
///   - Prettier already wraps the fragment text before calling `textToDoc()`
///     - v-for / v-slot: `function _(PARAMS) {}`
///     - generic: `type T<PARAMS> = any`
/// - Extract target node
/// - Format as IR
/// - Convert to Prettier Doc JSON
#[instrument(level = "debug", name = "oxfmt::text_to_doc::fragment", skip_all, fields(?source_type, ?kind))]
fn run_fragment(
    source_type: SourceType,
    source_text: &str,
    oxfmt_plugin_options_json: &str,
    kind: FragmentKind,
) -> Result<Value, EmbedFailure> {
    let (config, parent_filepath) = parse_payload(oxfmt_plugin_options_json)?;
    // Reuses the same config resolver as `run_full()`, but only `format_options` is needed here,
    // since `run_fragment()` does not dispatch external services callbacks.
    let resolved = resolve_for_embedded_js(config, parent_filepath).map_err(|err| {
        EmbedFailure::internal(format!("`_oxfmtPluginOptionsJson` carries invalid config: {err}"))
    })?;
    let format_options = resolved.format_options;

    // Map the Prettier-side fragment kind to the formatter's usage context.
    // The parens-vs-no-parens / quote-style decisions live inside `format_fragment`.
    let context = match kind {
        FragmentKind::VueForBindingLeft => FragmentContext::FunctionParamsAsBindingLhs,
        FragmentKind::VueBindings => FragmentContext::FunctionParamsAsBinding,
        FragmentKind::VueScriptGeneric => FragmentContext::TypeParameters,
        FragmentKind::ExpressionAttribute => {
            FragmentContext::Expression { in_html_attribute: true, vue_expression: false }
        }
        FragmentKind::ExpressionInterpolation => {
            FragmentContext::Expression { in_html_attribute: false, vue_expression: false }
        }
        FragmentKind::VueExpressionAttribute => {
            FragmentContext::Expression { in_html_attribute: true, vue_expression: true }
        }
        FragmentKind::VueExpressionInterpolation => {
            FragmentContext::Expression { in_html_attribute: false, vue_expression: true }
        }
        FragmentKind::EventHandler => FragmentContext::EventHandlerStatements,
    };

    let allocator = Allocator::default();
    let fragment = match oxc_formatter::format_fragment(
        &allocator,
        source_text,
        source_type,
        *format_options,
        context,
    ) {
        Ok(fragment) => fragment,
        // `oxc_formatter::format_fragment()` fails when the pre-wrapped source
        // does not parse (the dominant case) or does not match the shape the
        // context expects; both are reported as a syntax failure, and `run()`
        // adds the `parseError` marker the `v-on` fallback depends on.
        Err(err) => return Err(EmbedFailure::Syntax(format!("{err}"))),
    };

    let expression_root = fragment.expression_root;
    let (elements, sorted_tailwind_classes) =
        fragment.formatted.into_final_document().into_elements_and_tailwind_classes();
    let mut doc_json =
        to_prettier_doc::format_elements_to_prettier_doc(elements, &sorted_tailwind_classes)
            .map_err(|err| {
                EmbedFailure::internal(format!(
                    "Formatter IR to Prettier Doc conversion failed: {err}"
                ))
            })?;

    // Report the root expression kind so the JS side can feed Prettier's
    // `__onHtmlBindingRoot` hook (hug-vs-expand layout for attribute values).
    if let (Some(root), Some(object)) = (expression_root, doc_json.as_object_mut()) {
        use oxc_formatter::ExpressionRootKind;
        let estree_type = match root {
            ExpressionRootKind::ObjectExpression => "ObjectExpression",
            ExpressionRootKind::ArrayExpression => "ArrayExpression",
            ExpressionRootKind::TemplateLiteral => "TemplateLiteral",
            ExpressionRootKind::StringLiteral => "StringLiteral",
            // Any type outside the hug list behaves the same; `Unknown` is not
            // a real estree type, it only needs to miss `shouldHugJsExpression`.
            ExpressionRootKind::Other => "Unknown",
        };
        object.insert("expressionRoot".to_string(), Value::String(estree_type.to_string()));
    }
    Ok(doc_json)
}

// ---

/// Deserialize `_oxfmtPluginOptionsJson` into the typed config + parent filepath.
fn parse_payload(oxfmt_plugin_options_json: &str) -> Result<(FormatConfig, PathBuf), EmbedFailure> {
    #[derive(Deserialize)]
    struct Payload {
        config: FormatConfig,
        filepath: String,
    }
    let payload: Payload = serde_json::from_str(oxfmt_plugin_options_json).map_err(|err| {
        EmbedFailure::internal(format!("`_oxfmtPluginOptionsJson` failed to deserialize: {err}"))
    })?;
    Ok((payload.config, PathBuf::from(payload.filepath)))
}

#[cfg(test)]
mod tests {
    use super::{EmbedFailure, FragmentKind, failure_payload};

    #[test]
    fn syntax_failure_payload_marks_parse_error_for_expression_fragments() {
        let failure = EmbedFailure::Syntax("Unexpected token".to_string());
        let payload = failure_payload(
            &failure,
            "expression-attribute",
            Some(FragmentKind::ExpressionAttribute),
        );
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(payload["parseError"], serde_json::json!(true));
        assert_eq!(payload["error"]["kind"], "syntax");
        assert_eq!(payload["error"]["message"], "Unexpected token");
    }

    #[test]
    fn internal_failure_payload_never_marks_parse_error() {
        let failure = EmbedFailure::internal("boom");
        let payload = failure_payload(
            &failure,
            "expression-attribute",
            Some(FragmentKind::ExpressionAttribute),
        );
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();

        assert!(payload.get("parseError").is_none());
        assert_eq!(payload["error"]["kind"], "internal");
        assert_eq!(payload["error"]["message"], "boom");
    }

    #[test]
    fn full_mode_syntax_failure_has_no_parse_error_marker() {
        let failure = EmbedFailure::Syntax("Unexpected token".to_string());
        let payload = failure_payload(&failure, "vue-script", None);
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();

        assert!(payload.get("parseError").is_none());
        assert_eq!(payload["error"]["kind"], "syntax");
    }
}
