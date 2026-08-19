use oxc_ast::{
    AstKind,
    ast::{Expression, TSSignature, TSType, TSTypeName},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    context::{ContextHost, LintContext},
    rule::{DefaultRuleConfig, Rule},
};

fn non_prefixed_function_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Component event name must start with \"on\".")
        .with_help("Rename the prop so it reads as an event, like `onclick`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct RequireEventPrefix {
    /// Also report a prop whose method signature returns a `Promise`.
    check_async_functions: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a component's function-typed props — the Svelte 5 way of
    /// declaring events — to be named starting with `on`.
    ///
    /// ### Why is this bad?
    ///
    /// `onclick={…}` reads at the call site as the event it is. A callback
    /// prop named `clicked` looks like data until you follow its type.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script lang="ts">
    ///   interface Props {
    ///     clicked: () => void;
    ///   }
    ///   let { clicked }: Props = $props();
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script lang="ts">
    ///   interface Props {
    ///     onclick: () => void;
    ///   }
    ///   let { onclick }: Props = $props();
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// `checkAsyncFunctions` (default `false`) also reports a prop declared
    /// as a method signature returning a `Promise`.
    ///
    /// ```json
    /// { "svelte/require-event-prefix": ["error", { "checkAsyncFunctions": true }] }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// Upstream reads the props type through the TypeScript checker. oxlint
    /// works from the syntax alone, so it sees the annotation written on the
    /// `$props()` destructuring — an inline object type, or a local
    /// `interface`/`type` in the same `<script>`. It does not follow an
    /// imported type, a generic parameter, an intersection or `extends`
    /// clause, or a mapped type, and so reports nothing for those.
    RequireEventPrefix,
    svelte,
    style,
    config = RequireEventPrefix,
    version = "1.80.0",
    short_description = "Require component event props to start with `on`.",
);

impl Rule for RequireEventPrefix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        // A Svelte 5 concept, and `$props()` only means this in a component.
        ctx.file_extension().is_some_and(|extension| extension == "svelte")
    }

    fn run_once(&self, ctx: &LintContext) {
        // The script's own type declarations, so an annotation naming one
        // can be resolved to its members.
        let mut declarations = Declarations::default();
        for node in ctx.nodes() {
            match node.kind() {
                AstKind::TSTypeAliasDeclaration(alias) => {
                    declarations.aliases.insert(alias.id.name.as_str(), &alias.type_annotation);
                }
                AstKind::TSInterfaceDeclaration(interface) => {
                    declarations
                        .interfaces
                        .insert(interface.id.name.as_str(), interface.body.body.as_slice());
                }
                _ => {}
            }
        }

        for node in ctx.nodes() {
            let AstKind::VariableDeclarator(declarator) = node.kind() else { continue };
            if !is_props_call(declarator.init.as_ref()) {
                continue;
            }
            let Some(annotation) = &declarator.type_annotation else { continue };
            for signature in resolve_members(&annotation.type_annotation, &declarations) {
                if let Some(span) = self.offending_member(signature) {
                    ctx.diagnostic(non_prefixed_function_diagnostic(span));
                }
            }
        }
    }
}

impl RequireEventPrefix {
    /// The span to report, when this member is a function-typed prop whose
    /// name does not start with `on`.
    fn offending_member(self, signature: &TSSignature<'_>) -> Option<Span> {
        let (name, span, is_method) = match signature {
            TSSignature::TSPropertySignature(property) => {
                // Only a property whose type is written as a function type
                // counts; upstream's check is the same shape.
                let is_function = property.type_annotation.as_ref().is_some_and(|annotation| {
                    matches!(annotation.type_annotation, TSType::TSFunctionType(_))
                });
                if !is_function {
                    return None;
                }
                (property.key.static_name()?, property.span, false)
            }
            TSSignature::TSMethodSignature(method) => {
                (method.key.static_name()?, method.span, true)
            }
            _ => return None,
        };
        if name.starts_with("on") {
            return None;
        }
        // Upstream only ever treats a *method* signature as async, so a
        // property typed `() => Promise<void>` is reported either way.
        if is_method && !self.check_async_functions && returns_promise(signature) {
            return None;
        }
        Some(span)
    }
}

/// Whether an initialiser is a bare `$props()` call.
fn is_props_call(init: Option<&Expression<'_>>) -> bool {
    matches!(init, Some(Expression::CallExpression(call))
        if matches!(&call.callee, Expression::Identifier(callee) if callee.name == "$props"))
}

/// The `type` and `interface` declarations the script writes out.
#[derive(Default)]
struct Declarations<'t, 'a> {
    aliases: FxHashMap<&'a str, &'t TSType<'a>>,
    interfaces: FxHashMap<&'a str, &'t [TSSignature<'a>]>,
}

/// The members of a props type, following a reference to a declaration the
/// same script writes out.
fn resolve_members<'t, 'a>(
    ts_type: &'t TSType<'a>,
    declarations: &Declarations<'t, 'a>,
) -> &'t [TSSignature<'a>] {
    match ts_type {
        TSType::TSTypeLiteral(literal) => literal.members.as_slice(),
        TSType::TSTypeReference(reference) => {
            let TSTypeName::IdentifierReference(name) = &reference.type_name else {
                return &[];
            };
            let name = name.name.as_str();
            if let Some(aliased) = declarations.aliases.get(name) {
                // One step only: an alias chain would need a cycle guard for
                // no real benefit.
                return match aliased {
                    TSType::TSTypeLiteral(literal) => literal.members.as_slice(),
                    _ => &[],
                };
            }
            declarations.interfaces.get(name).copied().unwrap_or(&[])
        }
        _ => &[],
    }
}

/// Whether a method signature is declared to return a `Promise`.
fn returns_promise(signature: &TSSignature<'_>) -> bool {
    let TSSignature::TSMethodSignature(method) = signature else { return false };
    method.return_type.as_ref().is_some_and(|annotation| {
        matches!(&annotation.type_annotation, TSType::TSTypeReference(reference)
            if matches!(&reference.type_name, TSTypeName::IdentifierReference(name)
                if name.name == "Promise"))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireEventPrefix;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let path = || Some(PathBuf::from("test.svelte"));
        let check_async = || Some(serde_json::json!([{ "checkAsyncFunctions": true }]));
        let pass = vec![
            (
                "<script lang=\"ts\">\n\tinterface Props { onclick: () => void }\n\tlet { onclick }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // A data prop is not an event.
            (
                "<script lang=\"ts\">\n\tinterface Props { value: string }\n\tlet { value }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // An async method signature is left alone by default.
            (
                "<script lang=\"ts\">\n\tinterface Props { load(): Promise<void> }\n\tlet { load }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // No annotation to read.
            ("<script lang=\"ts\">\n\tlet { click } = $props();\n</script>", None, None, path()),
            // An imported type is not resolvable from the syntax.
            (
                "<script lang=\"ts\">\n\timport type { Props } from './props';\n\tlet { click }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
        ];
        let fail = vec![
            (
                "<script lang=\"ts\">\n\tinterface Props { clicked: () => void }\n\tlet { clicked }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // An inline object type.
            (
                "<script lang=\"ts\">\n\tlet { clicked }: { clicked: () => void } = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // A `type` alias.
            (
                "<script lang=\"ts\">\n\ttype Props = { clicked: () => void };\n\tlet { clicked }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // A method signature.
            (
                "<script lang=\"ts\">\n\tinterface Props { clicked(): void }\n\tlet { clicked }: Props = $props();\n</script>",
                None,
                None,
                path(),
            ),
            // With `checkAsyncFunctions`, the async one is reported too.
            (
                "<script lang=\"ts\">\n\tinterface Props { load(): Promise<void> }\n\tlet { load }: Props = $props();\n</script>",
                check_async(),
                None,
                path(),
            ),
        ];

        Tester::new(RequireEventPrefix::NAME, RequireEventPrefix::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
