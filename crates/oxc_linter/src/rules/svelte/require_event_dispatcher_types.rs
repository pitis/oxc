use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    ast_util::outermost_paren_parent,
    context::{ContextHost, LintContext},
    module_record::ImportImportName,
    rule::Rule,
};

fn require_event_dispatcher_types_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Type parameters missing for the `createEventDispatcher` function call.")
        .with_help(
            "Add an event map type parameter, e.g. `createEventDispatcher<{ change: string }>()`.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireEventDispatcherTypes;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires type parameters on `createEventDispatcher` calls in TypeScript
    /// Svelte components.
    ///
    /// ### Why is this bad?
    ///
    /// Without a type parameter, the event dispatcher created by
    /// `createEventDispatcher` accepts any event name with any payload, so
    /// typos in event names and wrong payload types are not caught by the
    /// TypeScript compiler.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script lang="ts">
    ///   import { createEventDispatcher } from 'svelte';
    ///
    ///   const dispatch = createEventDispatcher();
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script lang="ts">
    ///   import { createEventDispatcher } from 'svelte';
    ///
    ///   const dispatch = createEventDispatcher<{ click: null }>();
    /// </script>
    /// ```
    RequireEventDispatcherTypes,
    svelte,
    suspicious,
    version = "1.80.0",
    short_description = "Require type parameters on `createEventDispatcher`.",
);

impl Rule for RequireEventDispatcherTypes {
    fn run_once(&self, ctx: &LintContext) {
        let scoping = ctx.scoping();

        for entry in &ctx.module_record().import_entries {
            if entry.module_request.name() != "svelte" {
                continue;
            }

            match &entry.import_name {
                ImportImportName::Name(name_span)
                    if name_span.name() == "createEventDispatcher" =>
                {
                    let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                    else {
                        continue;
                    };
                    for reference in scoping.get_resolved_references(symbol_id) {
                        let ident_node = ctx.nodes().get_node(reference.node_id());
                        if let Some(call_span) =
                            get_untyped_call_span(ident_node, ident_node.span(), ctx)
                        {
                            ctx.diagnostic(require_event_dispatcher_types_diagnostic(call_span));
                        }
                    }
                }
                ImportImportName::NamespaceObject => {
                    let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                    else {
                        continue;
                    };
                    for reference in scoping.get_resolved_references(symbol_id) {
                        let ident_node = ctx.nodes().get_node(reference.node_id());
                        // Looking for `ns.createEventDispatcher(...)`.
                        let Some(member_node) = outermost_paren_parent(ident_node, ctx.semantic())
                        else {
                            continue;
                        };
                        let AstKind::StaticMemberExpression(member) = member_node.kind() else {
                            continue;
                        };
                        if member.property.name != "createEventDispatcher"
                            || member.object.get_inner_expression().span() != ident_node.span()
                        {
                            continue;
                        }
                        if let Some(call_span) =
                            get_untyped_call_span(member_node, member.span, ctx)
                        {
                            ctx.diagnostic(require_event_dispatcher_types_diagnostic(call_span));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        // The dispatcher can only be typed in TypeScript, so plain JS
        // `<script>` blocks are skipped, like upstream.
        ctx.file_extension().is_some_and(|ext| ext == "svelte") && ctx.source_type().is_typescript()
    }
}

/// If `callee_node` (spanning `callee_span`) is called directly and the call
/// has no type arguments, returns the call's span.
fn get_untyped_call_span<'a>(
    callee_node: &AstNode<'a>,
    callee_span: Span,
    ctx: &LintContext<'a>,
) -> Option<Span> {
    let parent = outermost_paren_parent(callee_node, ctx.semantic())?;
    let AstKind::CallExpression(call) = parent.kind() else {
        return None;
    };
    if call.callee.get_inner_expression().span() != callee_span {
        return None;
    }
    if call.type_arguments.is_some() { None } else { Some(call.span) }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        (
            r#"<script lang="ts">
                import { createEventDispatcher } from 'svelte';

                const dispatch1 = createEventDispatcher<{ one: never; two: number }>();
                const dispatch2 = createEventDispatcher<Record<string, never>>();
                const dispatch3 = createEventDispatcher<any>();
                const dispatch4 = createEventDispatcher<unknown>();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // In plain JS scripts the call cannot be typed, so the rule does not run.
        (
            "<script>
                import { createEventDispatcher } from 'svelte';

                const dispatch = createEventDispatcher();
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Dispatcher factory comes from somewhere else.
        (
            r#"<script lang="ts">
                import { createEventDispatcher } from './unknown';

                const dispatch = createEventDispatcher();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // Namespace import with type parameters.
        (
            r#"<script lang="ts">
                import * as svelte from 'svelte';

                const dispatch = svelte.createEventDispatcher<{ click: null }>();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // Not a call.
        (
            r#"<script lang="ts">
                import { createEventDispatcher } from 'svelte';

                console.log(createEventDispatcher);
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // Local binding shadows nothing; no svelte import involved.
        (
            r#"<script lang="ts">
                const createEventDispatcher = () => () => {};
                const dispatch = createEventDispatcher();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            r#"<script lang="ts">
                import { createEventDispatcher } from 'svelte';

                const dispatch = createEventDispatcher();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // Aliased import.
        (
            r#"<script lang="ts">
                import { createEventDispatcher as ced } from 'svelte';

                const dispatch = ced();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // Namespace import without type parameters.
        (
            r#"<script lang="ts">
                import * as svelte from 'svelte';

                const dispatch = svelte.createEventDispatcher();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
        // Module script blocks are checked too.
        (
            r#"<script module lang="ts">
                import { createEventDispatcher } from 'svelte';

                const dispatch = createEventDispatcher();
            </script>"#,
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(RequireEventDispatcherTypes::NAME, RequireEventDispatcherTypes::PLUGIN, pass, fail)
        .test_and_snapshot();
}
