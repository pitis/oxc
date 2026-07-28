use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::{
    AstKind,
    ast::{
        Argument, CallExpression, ExportDefaultDeclarationKind, Expression, ObjectExpression,
        Statement,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use oxc_vue_parser::{Sfc, SfcBlock};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{
        VUE2_BUILTIN_COMPONENT_NAMES, VUE3_BUILTIN_COMPONENT_NAMES_EXTRA, find_property,
        is_vue_component_options_call, vue_casing,
    },
    vue_template::{VueSfcRule, VueTemplateContext},
};

fn multi_word_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Component name \"{name}\" should always be multi-word."))
        .with_help(
            "Rename to a multi-word name (e.g. `TodoItem` instead of `Todo`) so template usage can't be confused with a native HTML element.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MultiWordComponentNamesConfig {
    /// Component names to exempt from the multi-word check, in addition to
    /// the built-in `App`/`app`. A `PascalCase` entry also exempts its
    /// kebab-case form (e.g. `"TodoItem"` also allows `"todo-item"`).
    ignores: Vec<String>,
}

// Boxed: `ignores: Vec<String>` alone is 24 bytes, which would blow
// `RuleEnum`'s 16-byte budget (see `no_v_html.rs` for the same pattern).
#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
pub struct MultiWordComponentNames(Box<MultiWordComponentNamesConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires component names to always be multi-word, except for
    /// components named `App`/`app` and Vue's own built-in components
    /// (`Transition`, `KeepAlive`, `Teleport`, `Suspense`, …).
    ///
    /// The component name is taken from the first of: `defineOptions({
    /// name })` (`<script setup>`), `export default { name }` / `Vue.extend`
    /// / `defineComponent` / `Vue.component(name, {...})` (Options API), or
    /// — when no script declares a name — the `.vue` file's own filename.
    ///
    /// Two rarely-used upstream forms are **not** recognized as declaring a
    /// component at all (and so never suppress the filename fallback the
    /// way a recognized-but-nameless form does): a bare `new Vue({...})`
    /// runtime instance, and an object literal preceded by a `// @vue/component`
    /// marker comment. Neither is a realistic pattern for a `.vue` SFC's own
    /// `<script>` block (they're Vue 2 idioms for defining components outside
    /// of an SFC file).
    ///
    /// ### Why is this bad?
    ///
    /// All HTML elements are single words, so a single-word component name
    /// can only ever collide with a current or future native/custom
    /// element, or shadow a plain HTML tag silently.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <script>
    /// export default {
    ///   name: 'Todo',
    /// };
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <script>
    /// export default {
    ///   name: 'TodoItem',
    /// };
    /// </script>
    /// ```
    MultiWordComponentNames,
    vue,
    correctness,
    config = MultiWordComponentNames,
    version = "1.77.0",
    short_description = "Require component names to be always multi-word.",
);

impl Rule for MultiWordComponentNames {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueSfcRule for MultiWordComponentNames {
    fn run_on_sfc<'a>(&self, sfc: &Sfc<'a>, path: &Path, ctx: &mut VueTemplateContext<'a>) {
        let has_script_setup =
            sfc.blocks.iter().any(|block| block.name == "script" && block.has_attribute("setup"));

        // Upstream's `hasVue` starts `true` whenever a `<script setup>`
        // block exists at all, independent of its content — see
        // `isScriptSetup` in `multi-word-component-names.js`.
        let mut has_vue = has_script_setup;
        let mut has_name = false;
        let mut literal_name: Option<(String, Span)> = None;
        // Upstream's guard is `node.body.length > 0` — the *parsed AST's*
        // statement count, not the block's raw text. A comment-only script
        // (`<script>// hi</script>`) is textually non-empty but has zero AST
        // statements, so it must NOT suppress the fallback.
        let mut script_body_nonempty = false;

        for block in &sfc.blocks {
            if has_name {
                break;
            }
            if block.name != "script" {
                continue;
            }
            if block.has_attribute("setup") {
                scan_script_setup(block, &mut has_name, &mut literal_name);
            } else {
                scan_script(
                    block,
                    &mut has_vue,
                    &mut has_name,
                    &mut literal_name,
                    &mut script_body_nonempty,
                );
            }
        }

        let ignores = build_ignores(&self.0.ignores);

        if has_name {
            if let Some((value, span)) = literal_name
                && !is_valid_component_name(&value, &ignores)
            {
                ctx.diagnostic(multi_word_diagnostic(&value, span));
            }
            return;
        }

        // No script declared a name at all: upstream only falls back to the
        // filename when there was no recognizable Vue component definition
        // (`hasVue`), UNLESS the script that produced no definition was
        // itself empty (or absent) — see the `Program:exit` guard in
        // `multi-word-component-names.js`.
        if !has_vue && script_body_nonempty {
            return;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { return };
        if !is_valid_component_name(stem, &ignores) {
            // Upstream reports at `{ line: 1, column: 0 }` — the document
            // start — since there's no name node to point at.
            ctx.diagnostic(multi_word_diagnostic(stem, Span::empty(0)));
        }
    }
}

/// eslint-plugin-vue's `isValidComponentName`: exempt from the multi-word
/// check if `name` is in the (user + built-in `App`/`app`) ignore set, is a
/// Vue built-in component name, or its kebab-case form has more than one
/// `-`-separated segment.
fn is_valid_component_name(name: &str, ignores: &FxHashSet<String>) -> bool {
    if ignores.contains(name)
        || VUE2_BUILTIN_COMPONENT_NAMES.contains(name)
        || VUE3_BUILTIN_COMPONENT_NAMES_EXTRA.contains(name)
    {
        return true;
    }
    vue_casing::kebab_case(name).split('-').count() > 1
}

/// Builds the ignore set: `App`/`app` plus each configured `ignores` entry —
/// and, when that entry is itself `PascalCase`, its kebab-case form too
/// (mirrors upstream: `if (isPascalCase(ignore)) ignores.add(kebabCase(ignore))`).
fn build_ignores(user_ignores: &[String]) -> FxHashSet<String> {
    let mut ignores = FxHashSet::default();
    ignores.insert("App".to_string());
    ignores.insert("app".to_string());
    for ignore in user_ignores {
        ignores.insert(ignore.clone());
        if vue_casing::is_pascal_case(ignore) {
            ignores.insert(vue_casing::kebab_case(ignore));
        }
    }
    ignores
}

/// Scans a `<script setup>` block for a top-level `defineOptions({ name })`
/// call — mirrors upstream's `defineScriptSetupVisitor`'s
/// `onDefineOptionsEnter`, which only recognizes the macro as the direct
/// expression of a top-level statement (or a top-level variable
/// initializer), not an arbitrarily nested call.
fn scan_script_setup(
    block: &SfcBlock<'_>,
    has_name: &mut bool,
    literal_name: &mut Option<(String, Span)>,
) {
    let Some(source_type) = script_source_type(block.lang()) else { return };
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, block.content, source_type).parse();
    let offset = block.content_span.start;

    for stmt in &ret.program.body {
        let Some(call) = top_level_call(stmt) else { continue };
        if !is_define_options_call(call) {
            continue;
        }
        // Upstream: `arguments.length === 0` -> no report; first argument
        // must be a (non-nested) `ObjectExpression`; it must have a `name`
        // property, otherwise `hasName` is never set for this call.
        let Some(first) = call.arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        let Expression::ObjectExpression(obj) = first.get_inner_expression() else { return };
        apply_name_property(obj, offset, has_name, literal_name);
        return;
    }
}

/// Scans a plain `<script>` block's whole AST (not just top-level
/// statements — upstream's `executeOnVue`/`executeOnCallVueComponent`
/// visit every matching node regardless of nesting depth) for a Vue
/// component options object or a `.component(...)` registration call, and
/// records whether the block's *parsed* body has any statements at all
/// (`body_nonempty`, mirroring upstream's `Program:exit` guard, which reads
/// `node.body.length` — the AST, not the raw source text: a comment-only
/// script has non-empty text but a zero-length body).
///
/// On a parse failure (unrecognized `lang`, so no `SourceType` could even be
/// derived) `body_nonempty` is left untouched — the block is treated as
/// though it weren't there at all, same as the rest of this function's
/// no-op-on-failure behavior and the `.vue` loader silently excluding such a
/// block from the sub-host script pass.
fn scan_script(
    block: &SfcBlock<'_>,
    has_vue: &mut bool,
    has_name: &mut bool,
    literal_name: &mut Option<(String, Span)>,
    body_nonempty: &mut bool,
) {
    let Some(source_type) = script_source_type(block.lang()) else { return };
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, block.content, source_type).parse();
    let offset = block.content_span.start;
    *body_nonempty |= !ret.program.body.is_empty();
    let semantic = SemanticBuilder::new_linter().build(&ret.program).semantic;

    for node in semantic.nodes() {
        match node.kind() {
            AstKind::ExportDefaultDeclaration(export) => {
                if let Some(obj) = vue_object_in_export_default(&export.declaration) {
                    *has_vue = true;
                    if !*has_name {
                        apply_name_property(obj, offset, has_name, literal_name);
                    }
                }
            }
            AstKind::CallExpression(call) => {
                scan_component_call(call, offset, has_vue, has_name, literal_name);
            }
            _ => {}
        }
    }
}

/// `export default {...}` (a direct object literal) or
/// `export default defineComponent({...})` / `Vue.extend({...})` / etc.
/// (an object-literal last argument to a recognized component-definition
/// call) — mirrors upstream `getVueObjectType`'s `"export"`/`"definition"`
/// cases.
fn vue_object_in_export_default<'a, 'b>(
    decl: &'b ExportDefaultDeclarationKind<'a>,
) -> Option<&'b ObjectExpression<'a>> {
    match decl {
        ExportDefaultDeclarationKind::ObjectExpression(obj) => Some(obj),
        ExportDefaultDeclarationKind::CallExpression(call)
            if is_recognized_component_definition_call(call) =>
        {
            match call.arguments.last().and_then(Argument::as_expression)?.get_inner_expression() {
                Expression::ObjectExpression(obj) => Some(obj),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `<ident>.component(...)` (upstream `executeOnCallVueComponent`: any
/// `.component(...)` call with at least one argument sets `hasVue`; the
/// name is only extracted — and `hasName` only set — when there are exactly
/// two arguments) and standalone `createApp(...)` / `defineComponent(...)` /
/// `Vue.extend(...)` / etc. calls with an object-literal last argument
/// (`getVueComponentDefinitionType`) not already covered by an
/// `export default`.
fn scan_component_call(
    call: &CallExpression<'_>,
    offset: u32,
    has_vue: &mut bool,
    has_name: &mut bool,
    literal_name: &mut Option<(String, Span)>,
) {
    if is_dot_component_call(call) && !call.arguments.is_empty() {
        *has_vue = true;
        if call.arguments.len() == 2 && !*has_name {
            *has_name = true;
            *literal_name = call
                .arguments
                .first()
                .and_then(Argument::as_expression)
                .and_then(extract_literal)
                .map(|(value, span)| (value, shift(span, offset)));
        }
        return;
    }

    if is_recognized_component_definition_call(call)
        && let Some(last) = call.arguments.last().and_then(Argument::as_expression)
        && let Expression::ObjectExpression(obj) = last.get_inner_expression()
        && !*has_name
    {
        *has_vue = true;
        apply_name_property(obj, offset, has_name, literal_name);
    }
}

/// Like `utils::is_vue_component_options_call`, but additionally enforces
/// upstream's `getVueComponentDefinitionType` guard for a member-call form
/// (`<obj>.component(...)` / `<obj>.mixin(...)` / `Vue.extend(...)`):
/// `if (calleeObject.type === "Identifier") { ... }` wraps that whole
/// branch upstream — a non-identifier object (`this`, `a.b`, …) never
/// matches at all. The shared utility is intentionally more lenient for its
/// other callers (it doesn't enforce this), so it isn't reused as-is here.
/// Bare-identifier calls (`createApp(...)`, `defineComponent(...)`, …) are
/// unaffected — they have no "object" to restrict in the first place.
fn is_recognized_component_definition_call(call: &CallExpression<'_>) -> bool {
    if call.callee.get_identifier_reference().is_some() {
        return is_vue_component_options_call(call);
    }
    call.callee.get_member_expr().is_some_and(|member| {
        matches!(member.object().get_inner_expression(), Expression::Identifier(_))
    }) && is_vue_component_options_call(call)
}

/// `<ident>.component(...)` — upstream's selector is
/// `CallExpression > MemberExpression > Identifier[name='component']`
/// gated on `skipTSAsExpression(callee.object).type === "Identifier"`, i.e.
/// the member expression's *object* must itself be a bare identifier
/// (`Vue`, `app`, any name) — `this.component(...)` (a `ThisExpression`
/// object) and `a.b.component(...)` (a nested `MemberExpression` object) do
/// **not** match, even though their `.component(...)` shape looks similar.
fn is_dot_component_call(call: &CallExpression<'_>) -> bool {
    let Some(member) = call.callee.get_member_expr() else { return false };
    member.static_property_name().is_some_and(|name| name == "component")
        && matches!(member.object().get_inner_expression(), Expression::Identifier(_))
}

fn is_define_options_call(call: &CallExpression<'_>) -> bool {
    call.callee.get_identifier_reference().is_some_and(|ident| ident.name == "defineOptions")
}

/// Finds `obj`'s `name` property and, if present, sets `has_name` — mirrors
/// `hasName = true` firing whenever `findProperty(obj, "name")` succeeds,
/// independent of whether the value turns out to be a string literal.
fn apply_name_property(
    obj: &ObjectExpression<'_>,
    offset: u32,
    has_name: &mut bool,
    literal_name: &mut Option<(String, Span)>,
) {
    let Some(prop) = find_property(obj, "name") else { return };
    *has_name = true;
    *literal_name = extract_literal(&prop.value).map(|(value, span)| (value, shift(span, offset)));
}

/// Upstream's `validateName` only accepts an ESTree `Literal` node (i.e. our
/// `StringLiteral`) — not a template literal, identifier, or anything else.
fn extract_literal(expr: &Expression<'_>) -> Option<(String, Span)> {
    match expr.get_inner_expression() {
        Expression::StringLiteral(lit) => Some((lit.value.to_string(), lit.span)),
        _ => None,
    }
}

fn shift(span: Span, offset: u32) -> Span {
    Span::new(span.start + offset, span.end + offset)
}

fn top_level_call<'a, 'b>(stmt: &'b Statement<'a>) -> Option<&'b CallExpression<'a>> {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => match &expr_stmt.expression {
            Expression::CallExpression(call) => Some(call),
            _ => None,
        },
        Statement::VariableDeclaration(decl) => {
            decl.declarations.iter().find_map(|declarator| match declarator.init.as_ref()? {
                Expression::CallExpression(call) => Some(call.as_ref()),
                _ => None,
            })
        }
        _ => None,
    }
}

/// Mirrors the `.vue` script-block loader's source-type derivation
/// (`VuePartialLoader::parse_script` in `loader/partial_loader/vue.rs`):
/// `lang` defaults to `"mjs"`, an unambiguous source type is upgraded to a
/// module (Vue script blocks are always ESM), and a non-`x` lang is forced
/// to `standard` (non-JSX) parsing. Returns `None` for an unrecognized
/// `lang` — the caller then treats the block as if it weren't there at all,
/// same as the loader silently excluding it from the sub-host script pass.
fn script_source_type(lang: Option<&str>) -> Option<SourceType> {
    let lang = lang.unwrap_or("mjs");
    let mut source_type = SourceType::from_extension(lang).ok()?;
    if source_type.is_unambiguous() {
        source_type = source_type.with_module(true);
    }
    if !lang.contains('x') {
        source_type = source_type.with_standard(true);
    }
    Some(source_type)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::MultiWordComponentNames;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Multi-word filename, no script at all.
            ("<template><div/></template>", None, None, Some(PathBuf::from("TodoItem.vue"))),
            ("<template><div/></template>", None, None, Some(PathBuf::from("todo-item.vue"))),
            // Built-in `App`/`app` exemption.
            ("<template><div/></template>", None, None, Some(PathBuf::from("App.vue"))),
            ("<template><div/></template>", None, None, Some(PathBuf::from("app.vue"))),
            // Vue built-in component name (single word, exempted).
            ("<template><div/></template>", None, None, Some(PathBuf::from("component.vue"))),
            // Explicit multi-word name via `<script setup>` overrides a
            // single-word filename.
            (
                "<template><div/></template><script setup>defineOptions({ name: 'TodoItem' });</script>",
                None,
                None,
                Some(PathBuf::from("todo.vue")),
            ),
            // Explicit multi-word name via Options API `export default`.
            (
                "<template><div/></template><script>export default { name: 'TodoItem' };</script>",
                None,
                None,
                Some(PathBuf::from("todo.vue")),
            ),
            // `export default defineComponent({ name: ... })`.
            (
                "<template><div/></template><script>export default defineComponent({ name: 'TodoItem' });</script>",
                None,
                None,
                Some(PathBuf::from("todo.vue")),
            ),
            // `Vue.component('TodoItem', {...})` two-arg legacy registration.
            (
                "<template><div/></template><script>Vue.component('TodoItem', {});</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `Vue.component(someVar, {...})`: name isn't a string literal,
            // so `hasName` is still set (no fallback), but nothing is
            // reported either.
            (
                "<template><div/></template><script>Vue.component(someVar, {});</script>",
                None,
                None,
                Some(PathBuf::from("todo.vue")),
            ),
            // `this.component(...)`: the member expression's object is a
            // `ThisExpression`, not a bare identifier, so upstream's
            // selector doesn't match this at all — no `hasVue`, and (since
            // the script is otherwise non-empty and defines nothing
            // recognizable) no fallback report either.
            (
                r"<template><div/></template><script>this.component('foo', {}); console.log(1);</script>",
                None,
                None,
                Some(PathBuf::from("todo.vue")),
            ),
            // A non-empty plain `<script>` that doesn't define a Vue
            // component at all: `hasVue` stays false, so the (single-word)
            // filename fallback never runs.
            (
                r#"<template><div/></template><script>import Foo from "./foo"; console.log(Foo);</script>"#,
                None,
                None,
                Some(PathBuf::from("todo.vue")),
            ),
            // `ignores` option: exact match.
            (
                "<template><div/></template>",
                Some(json!([{ "ignores": ["Todo"] }])),
                None,
                Some(PathBuf::from("Todo.vue")),
            ),
            // `ignores` option: a `PascalCase` entry also exempts its
            // kebab-case filename form.
            (
                "<template><div/></template>",
                Some(json!([{ "ignores": ["Todo"] }])),
                None,
                Some(PathBuf::from("todo.vue")),
            ),
        ];

        let fail = vec![
            // No script at all: falls back to the (single-word) filename.
            ("<template><div/></template>", None, None, Some(PathBuf::from("todo.vue"))),
            // `<script setup>` with no `defineOptions` name: still falls
            // back to the filename (`hasVue` is `true` for any
            // `<script setup>`, regardless of its content).
            (
                "<template><div/></template><script setup></script>",
                None,
                None,
                Some(PathBuf::from("widget.vue")),
            ),
            // A wholly empty plain `<script>` block: falls back too (empty
            // body, so the `!hasVue && body.length > 0` skip doesn't apply).
            (
                "<template><div/></template><script></script>",
                None,
                None,
                Some(PathBuf::from("gadget.vue")),
            ),
            // A comment-only plain `<script>` block: textually non-empty,
            // but the parsed AST body has zero statements — the guard must
            // read `body.length`, not raw text, so this still falls back to
            // the filename (real eslint-plugin-vue reports this).
            (
                "<template><div/></template><script>// just a comment</script>",
                None,
                None,
                Some(PathBuf::from("gizmo.vue")),
            ),
            // `export default {}` with no `name` property: `hasVue` is
            // `true` (a component was recognized) but `hasName` stays
            // false, so the filename fallback still runs.
            (
                "<template><div/></template><script>export default {};</script>",
                None,
                None,
                Some(PathBuf::from("thing.vue")),
            ),
            // Explicit single-word name via `<script setup>`.
            (
                "<template><div/></template><script setup>defineOptions({ name: 'Todo' });</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Explicit single-word name via Options API `export default`.
            (
                "<template><div/></template><script>export default { name: 'Todo' };</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `export default defineComponent({ name: 'Todo' })`.
            (
                "<template><div/></template><script>export default defineComponent({ name: 'Todo' });</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `Vue.component('Todo', {...})` two-arg legacy registration.
            (
                "<template><div/></template><script>Vue.component('Todo', {});</script>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // No internal casing boundary: `kebabCase("Todoitem")` is still
            // a single segment.
            ("<template><div/></template>", None, None, Some(PathBuf::from("Todoitem.vue"))),
            // `ignores` configured, but for a different name.
            (
                "<template><div/></template>",
                Some(json!([{ "ignores": ["Other"] }])),
                None,
                Some(PathBuf::from("todo.vue")),
            ),
        ];

        Tester::new(MultiWordComponentNames::NAME, MultiWordComponentNames::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
