use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    ast_util::outermost_paren_parent,
    context::{ContextHost, LintContext},
    module_record::ImportImportName,
    rule::{DefaultRuleConfig, Rule},
};

fn no_unnecessary_state_wrap_diagnostic(span: Span, class_name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "{class_name} is already reactive, $state wrapping is unnecessary."
    ))
    .with_help("Remove the `$state(...)` wrapper and use the reactive class directly.")
    .with_label(span)
}

/// Reactive classes exported by `svelte/reactivity`.
const REACTIVE_CLASSES: [&str; 6] =
    ["SvelteSet", "SvelteMap", "SvelteURL", "SvelteURLSearchParams", "SvelteDate", "MediaQuery"];

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoUnnecessaryStateWrapConfig {
    /// Additional class names that are considered already reactive and are
    /// reported when wrapped in `$state`, matched by identifier name.
    additional_reactive_classes: Vec<String>,
    /// When `true`, `$state` wrapping is allowed for variables that are
    /// reassigned somewhere in the script.
    allow_reassign: bool,
}

// Boxed: the `Vec<String>` option would blow `RuleEnum`'s 16-byte budget
// unboxed (same pattern as `svelte/no-unknown-style-directive-property`).
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct NoUnnecessaryStateWrap(Box<NoUnnecessaryStateWrapConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows wrapping instances of already-reactive classes from
    /// `svelte/reactivity` (such as `SvelteSet`, `SvelteMap`, `SvelteURL`,
    /// `SvelteURLSearchParams`, `SvelteDate` and `MediaQuery`) in `$state`.
    ///
    /// ### Why is this bad?
    ///
    /// The classes exported by `svelte/reactivity` are already deeply
    /// reactive, so wrapping them in `$state` adds no reactivity. It only
    /// creates an extra state binding and suggests a misunderstanding of how
    /// these classes work.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { SvelteSet } from 'svelte/reactivity';
    ///
    ///   const set = $state(new SvelteSet());
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { SvelteSet } from 'svelte/reactivity';
    ///
    ///   const set = new SvelteSet();
    /// </script>
    /// ```
    NoUnnecessaryStateWrap,
    svelte,
    suspicious,
    suggestion,
    config = NoUnnecessaryStateWrap,
    version = "1.80.0",
    short_description = "Disallow unnecessary `$state` wrapping of reactive built-in classes.",
);

impl Rule for NoUnnecessaryStateWrap {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // Handles `additionalReactiveClasses`, which are matched by bare
        // identifier name with no import tracking, like upstream.
        if self.0.additional_reactive_classes.is_empty() {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !matches!(&call.callee, Expression::Identifier(ident) if ident.name == "$state") {
            return;
        }

        for argument in &call.arguments {
            let Some(arg_expr) = argument.as_expression() else {
                continue;
            };
            let (callee, arg_span) = match arg_expr.get_inner_expression() {
                Expression::NewExpression(new_expr) => (&new_expr.callee, new_expr.span),
                Expression::CallExpression(call_expr) => (&call_expr.callee, call_expr.span),
                _ => continue,
            };
            let Expression::Identifier(class_ident) = callee else {
                continue;
            };
            if !self
                .0
                .additional_reactive_classes
                .iter()
                .any(|name| class_ident.name.as_str() == name)
            {
                continue;
            }
            self.report_unnecessary_state_wrap(node, call.span, arg_span, &class_ident.name, ctx);
        }
    }

    fn run_once(&self, ctx: &LintContext) {
        let scoping = ctx.scoping();

        for entry in &ctx.module_record().import_entries {
            if entry.module_request.name() != "svelte/reactivity" {
                continue;
            }

            match &entry.import_name {
                ImportImportName::Name(name_span)
                    if REACTIVE_CLASSES.contains(&name_span.name()) =>
                {
                    let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                    else {
                        continue;
                    };
                    for reference in scoping.get_resolved_references(symbol_id) {
                        let ident_node = ctx.nodes().get_node(reference.node_id());
                        self.check_reactive_class_use(
                            ident_node,
                            ident_node.span(),
                            name_span.name(),
                            ctx,
                        );
                    }
                }
                ImportImportName::NamespaceObject => {
                    let Some(symbol_id) = scoping.get_root_binding(entry.local_name.name().into())
                    else {
                        continue;
                    };
                    for reference in scoping.get_resolved_references(symbol_id) {
                        let ident_node = ctx.nodes().get_node(reference.node_id());
                        // Looking for `ns.SvelteMap` used as a constructor/call.
                        let Some(member_node) = outermost_paren_parent(ident_node, ctx.semantic())
                        else {
                            continue;
                        };
                        let AstKind::StaticMemberExpression(member) = member_node.kind() else {
                            continue;
                        };
                        if !REACTIVE_CLASSES.contains(&member.property.name.as_str())
                            || member.object.get_inner_expression().span() != ident_node.span()
                        {
                            continue;
                        }
                        self.check_reactive_class_use(
                            member_node,
                            member.span,
                            &member.property.name,
                            ctx,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

impl NoUnnecessaryStateWrap {
    /// `callee_node` is a reference to a reactive class (spanning
    /// `callee_span`). If it is constructed or called directly inside a
    /// `$state(...)` argument, report it.
    fn check_reactive_class_use<'a>(
        &self,
        callee_node: &AstNode<'a>,
        callee_span: Span,
        class_name: &str,
        ctx: &LintContext<'a>,
    ) {
        let Some(use_node) = outermost_paren_parent(callee_node, ctx.semantic()) else {
            return;
        };
        let use_span = match use_node.kind() {
            AstKind::NewExpression(new_expr)
                if new_expr.callee.get_inner_expression().span() == callee_span =>
            {
                new_expr.span
            }
            AstKind::CallExpression(call_expr)
                if call_expr.callee.get_inner_expression().span() == callee_span =>
            {
                call_expr.span
            }
            _ => return,
        };

        let Some(state_call_node) = outermost_paren_parent(use_node, ctx.semantic()) else {
            return;
        };
        let AstKind::CallExpression(state_call) = state_call_node.kind() else {
            return;
        };
        if !matches!(&state_call.callee, Expression::Identifier(ident) if ident.name == "$state") {
            return;
        }

        self.report_unnecessary_state_wrap(
            state_call_node,
            state_call.span,
            use_span,
            class_name,
            ctx,
        );
    }

    /// Reports `$state(new Class())` (the call node spanning `state_span`,
    /// wrapping `target_span`) when the `$state` call initializes a variable
    /// declared with a plain identifier.
    fn report_unnecessary_state_wrap<'a>(
        &self,
        state_call_node: &AstNode<'a>,
        state_span: Span,
        target_span: Span,
        class_name: &str,
        ctx: &LintContext<'a>,
    ) {
        let Some(parent) = outermost_paren_parent(state_call_node, ctx.semantic()) else {
            return;
        };
        let AstKind::VariableDeclarator(declarator) = parent.kind() else {
            return;
        };
        let Some(binding_ident) = declarator.id.get_binding_identifier() else {
            return;
        };

        if self.0.allow_reassign {
            let symbol_id = binding_ident.symbol_id();
            if ctx
                .scoping()
                .get_resolved_references(symbol_id)
                .any(oxc_semantic::Reference::is_write)
            {
                return;
            }
        }

        ctx.diagnostic_with_suggestion(
            no_unnecessary_state_wrap_diagnostic(target_span, class_name),
            |fixer| {
                fixer
                    .replace(state_span, fixer.source_range(target_span).to_string())
                    .with_message("Remove unnecessary $state wrapping")
            },
        );
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        (
            "<script>
                import {
                    SvelteSet,
                    SvelteMap,
                    SvelteURL,
                    SvelteURLSearchParams,
                    SvelteDate,
                    MediaQuery
                } from 'svelte/reactivity';

                const set = new SvelteSet();
                const map = new SvelteMap();
                const url = new SvelteURL('https://example.com');
                const params = new SvelteURLSearchParams('key=value');
                const date = new SvelteDate();
                const mediaQuery = new MediaQuery('(min-width: 800px)');

                const regularState = $state(42);
                const stateObject = $state({ foo: 'bar' });
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // A class of the same name from another module is not reactive.
        (
            "<script>
                import { SvelteSet } from 'foo';

                const set = $state(new SvelteSet());
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Wrapping plain built-ins in $state is fine for this rule.
        (
            "<script>
                const map = $state(new Map());
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Aliased import used without $state.
        (
            "<script>
                import { SvelteSet as CustomSet } from 'svelte/reactivity';

                const set = new CustomSet();
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Not assigned to a plain identifier declarator.
        (
            "<script>
                import { SvelteMap } from 'svelte/reactivity';

                const list = [$state(new SvelteMap())];
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // additionalReactiveClasses without $state wrapping.
        (
            "<script>
                import { CustomReactiveClass1, CustomReactiveClass2 } from 'foo';
                const custom1 = new CustomReactiveClass1();
                const custom2 = new CustomReactiveClass2();

                const regularState = $state(42);
            </script>",
            Some(serde_json::json!([{
                "additionalReactiveClasses": ["CustomReactiveClass1", "CustomReactiveClass2"]
            }])),
            None,
            svelte_path(),
        ),
        // allowReassign: reassigned variables are allowed.
        (
            "<script>
                import { SvelteSet, SvelteMap } from 'svelte/reactivity';

                let set = $state(new SvelteSet());
                set = new SvelteSet([1, 2, 3]);

                let map = $state(new SvelteMap());
                map = new SvelteMap([['key', 'value']]);
            </script>",
            Some(serde_json::json!([{ "allowReassign": true }])),
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        (
            "<script>
                import {
                    SvelteSet,
                    SvelteMap,
                    SvelteURL,
                    SvelteURLSearchParams,
                    SvelteDate,
                    MediaQuery
                } from 'svelte/reactivity';

                const set = $state(new SvelteSet());
                const map = $state(new SvelteMap());
                const url = $state(new SvelteURL('https://example.com'));
                const params = $state(new SvelteURLSearchParams('key=value'));
                const date = $state(new SvelteDate());
                const mediaQuery = $state(new MediaQuery('(min-width: 800px)'));

                const regularState = $state(42);
                const stateObject = $state({ foo: 'bar' });
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Aliased imports are reported with the original class name.
        (
            "<script>
                import { SvelteSet as CustomSet, SvelteMap as CustomMap } from 'svelte/reactivity';

                const set = $state(new CustomSet());
                const map = $state(new CustomMap());
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Namespace imports are tracked too.
        (
            "<script>
                import * as reactivity from 'svelte/reactivity';

                const map = $state(new reactivity.SvelteMap());
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // additionalReactiveClasses.
        (
            "<script>
                import { CustomReactiveClass1, CustomReactiveClass2 } from 'foo';

                const custom1 = $state(new CustomReactiveClass1());
                const custom2 = $state(new CustomReactiveClass2());

                const regularState = $state(42);
            </script>",
            Some(serde_json::json!([{
                "additionalReactiveClasses": ["CustomReactiveClass1", "CustomReactiveClass2"]
            }])),
            None,
            svelte_path(),
        ),
        // allowReassign still reports variables that are never reassigned.
        (
            "<script>
                import { SvelteSet, SvelteMap } from 'svelte/reactivity';

                const set = $state(new SvelteSet());
                let map = $state(new SvelteMap());
            </script>",
            Some(serde_json::json!([{ "allowReassign": true }])),
            None,
            svelte_path(),
        ),
    ];

    let fix = vec![
        (
            "<script>
                import { SvelteSet } from 'svelte/reactivity';

                const set = $state(new SvelteSet());
            </script>",
            "<script>
                import { SvelteSet } from 'svelte/reactivity';

                const set = new SvelteSet();
            </script>",
            None,
            Some(PathBuf::from("test.svelte")),
        ),
        (
            "<script>
                import { SvelteURL } from 'svelte/reactivity';

                const url = $state(new SvelteURL('https://example.com'));
            </script>",
            "<script>
                import { SvelteURL } from 'svelte/reactivity';

                const url = new SvelteURL('https://example.com');
            </script>",
            None,
            Some(PathBuf::from("test.svelte")),
        ),
    ];

    Tester::new(NoUnnecessaryStateWrap::NAME, NoUnnecessaryStateWrap::PLUGIN, pass, fail)
        .expect_fix(fix)
        .test_and_snapshot();
}
