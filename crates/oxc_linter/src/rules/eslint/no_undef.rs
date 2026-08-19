use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::UnaryOperator;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{SVELTE_PSEUDO_GLOBALS, SVELTE_RUNES, VUE_SETUP_MACROS, is_svelte_path, is_vue_path},
};

fn no_undef_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{name}' is not defined."))
        .with_help(format!(
            "Either define '{name}' or remove the reference to it. If '{name}' is a global variable, add it to the 'globals' configuration."
        ))
        .with_label(span)
}

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NoUndef {
    /// When set to `true`, warns on undefined variables used in a `typeof` expression.
    #[serde(rename = "typeof")]
    // This field can't be called typeof directly, as that's a keyword in Rust.
    type_of: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallow the use of undeclared variables.
    ///
    /// This rule can be disabled for TypeScript code, as the TypeScript compiler
    /// enforces this check.
    ///
    /// ### Why is this bad?
    ///
    /// It is most likely a potential ReferenceError caused by a misspelling
    /// of a variable or parameter name.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```javascript
    /// var foo = someFunction();
    /// var bar = a + 1;
    /// ```
    NoUndef,
    eslint,
    nursery,
    config = NoUndef,
    version = "0.0.8",
    short_description = "Disallow the use of undeclared variables.",
);

impl Rule for NoUndef {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        let symbol_table = ctx.scoping();
        let framework = FrameworkGlobals::of(ctx);

        for reference_id_list in ctx.scoping().root_unresolved_references_ids() {
            for reference_id in reference_id_list {
                let reference = symbol_table.get_reference(reference_id);

                if reference.is_type() {
                    continue;
                }

                let name = ctx.semantic().reference_name(reference);

                if ctx.is_global_defined(name) {
                    continue;
                }

                if framework.is_defined(name, ctx) {
                    continue;
                }

                // Skip reporting error for 'arguments' if it's in a function scope
                if name == "arguments"
                    && ctx
                        .scoping()
                        .scope_ancestors(reference.scope_id())
                        .map(|id| ctx.scoping().scope_flags(id))
                        .any(|scope_flags| scope_flags.is_function() && !scope_flags.is_arrow())
                {
                    continue;
                }

                let node = ctx.nodes().get_node(reference.node_id());
                if !self.type_of && has_typeof_operator(node, ctx) {
                    continue;
                }

                ctx.diagnostic(no_undef_diagnostic(name, node.kind().span()));
            }
        }
    }
}

/// Names a single-file-component compiler makes available to a `<script>`
/// block without a declaration.
///
/// oxlint lints only the script of a `.svelte` / `.vue` file, as plain
/// JavaScript. The framework parsers ESLint uses (`svelte-eslint-parser`,
/// `vue-eslint-parser`) declare these for the rule, so ESLint reports none of
/// them; without this, `no-undef` fires on every runes component and every
/// `<script setup>`.
enum FrameworkGlobals {
    None,
    /// Runes, `$$props` & friends, `$store` auto-subscriptions, and bindings
    /// that a top-level `$:` reactive statement declares by assigning to them.
    Svelte {
        /// Spans of the top-level `$:` statements, used to find the latter.
        reactive_spans: Vec<Span>,
    },
    /// `<script setup>` compiler macros.
    Vue,
}

impl FrameworkGlobals {
    fn of(ctx: &LintContext<'_>) -> Self {
        let path = ctx.file_path();
        if is_svelte_path(path) {
            let reactive_spans = ctx
                .nodes()
                .iter()
                .filter_map(|node| {
                    let AstKind::LabeledStatement(labeled) = node.kind() else {
                        return None;
                    };
                    // Only a *top-level* `$:` is a reactive statement; a `$`
                    // label inside a function is an ordinary label.
                    (labeled.label.name == "$"
                        && matches!(ctx.nodes().parent_kind(node.id()), AstKind::Program(_)))
                    .then_some(labeled.span)
                })
                .collect();
            Self::Svelte { reactive_spans }
        } else if is_vue_path(path) {
            Self::Vue
        } else {
            Self::None
        }
    }

    fn is_defined(&self, name: &str, ctx: &LintContext<'_>) -> bool {
        match self {
            Self::None => false,
            Self::Vue => VUE_SETUP_MACROS.binary_search(&name).is_ok(),
            Self::Svelte { reactive_spans } => {
                if SVELTE_RUNES.binary_search(&name).is_ok()
                    || SVELTE_PSEUDO_GLOBALS.binary_search(&name).is_ok()
                {
                    return true;
                }
                // `$count` reads the store held by `count`, so it is defined
                // exactly when `count` is.
                if let Some(store) = name.strip_prefix('$')
                    && ctx.scoping().get_root_binding(store.into()).is_some()
                {
                    return true;
                }
                // `$: doubled = a * 2` declares `doubled`. Assigning to an
                // undeclared name inside a top-level `$:` is how Svelte 3/4
                // introduces a reactive binding, so every such write declares
                // the name for the whole component.
                !reactive_spans.is_empty() && self.is_reactive_declaration(name, ctx)
            }
        }
    }

    fn is_reactive_declaration(&self, name: &str, ctx: &LintContext<'_>) -> bool {
        let Self::Svelte { reactive_spans } = self else {
            return false;
        };
        ctx.scoping().root_unresolved_references_ids().flatten().any(|reference_id| {
            let reference = ctx.scoping().get_reference(reference_id);
            if !reference.is_write() || ctx.semantic().reference_name(reference) != name {
                return false;
            }
            let span = ctx.nodes().get_node(reference.node_id()).kind().span();
            reactive_spans.iter().any(|reactive| reactive.contains_inclusive(span))
        })
    }
}

fn has_typeof_operator(node: &AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    let parent = ctx.nodes().parent_node(node.id());
    match parent.kind() {
        AstKind::UnaryExpression(expr) => expr.operator == UnaryOperator::Typeof,
        AstKind::ParenthesizedExpression(_) => has_typeof_operator(parent, ctx),
        _ => false,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        ("var a = 1, b = 2; a;", None, None),
        // "/*global b*/ function f() { b; }",
        ("function f() { b; }", None, Some(serde_json::json!({"globals": { "b": false }}))),
        // "/*global b a:false*/  a;  function f() { b; a; }",
        ("function a(){}  a();", None, None),
        ("function f(b) { b; }", None, None),
        ("var a; a = 1; a++;", None, None),
        ("var a; function f() { a = 1; }", None, None),
        // "/*global b:true*/ b++;",
        // "/*eslint-env browser*/ window;",
        // "/*eslint-env node*/ require(\"a\");",
        ("Object; isNaN();", None, None),
        (
            "function evilEval(stuffToEval) { var ultimateAnswer; ultimateAnswer = 42; eval(stuffToEval); }",
            None,
            None,
        ),
        ("typeof a", None, None),
        ("typeof (a)", None, None),
        ("var b = typeof a", None, None),
        ("typeof a === 'undefined'", None, None),
        ("if (typeof a === 'undefined') {}", None, None),
        ("function foo() { var [a, b=4] = [1, 2]; return {a, b}; }", None, None),
        ("var toString = 1;", None, None),
        ("function myFunc(...foo) {  return foo;}", None, None),
        ("var React, App, a=1; React.render(<App attr={a} />);", None, None),
        ("var console; [1,2,3].forEach(obj => {\n  console.log(obj);\n});", None, None),
        ("var Foo; class Bar extends Foo { constructor() { super();  }}", None, None),
        ("import Warning from '../lib/warning'; var warn = new Warning('text');", None, None),
        ("import * as Warning from '../lib/warning'; var warn = new Warning('text');", None, None),
        ("var a; [a] = [0];", None, None),
        ("var a; ({a} = {});", None, None),
        ("var a; ({b: a} = {});", None, None),
        ("var obj; [obj.a, obj.b] = [0, 1];", None, None),
        ("URLSearchParams;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        (
            "URLSearchParams;",
            None,
            Some(
                serde_json::json!({"env": { "browser": false }, "globals": { "URLSearchParams": "readonly" }}),
            ),
        ),
        ("Intl;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        ("IntersectionObserver;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        ("Credential;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        ("requestIdleCallback;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        ("customElements;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        ("PromiseRejectionEvent;", None, Some(serde_json::json!({"env": { "browser": true }}))),
        ("(foo, bar) => { foo ||= WeakRef; bar ??= FinalizationRegistry; }", None, None),
        ("class C extends C {}", None, None),
        // "/*global b:false*/ function f() { b = 1; }",
        ("function f() { b = 1; }", None, Some(serde_json::json!({"globals": { "b": false } }))),
        // "/*global b:false*/ function f() { b++; }",
        // "/*global b*/ b = 1;",
        // "/*global b:false*/ var b = 1;",
        ("Array = 1;", None, None),
        ("class A { constructor() { new.target; } }", None, None),
        (
            "var {bacon, ...others} = stuff; foo(others)",
            None,
            // parserOptions: {
            //     ecmaVersion: 2018
            // },
            Some(serde_json::json!({"globals": { "stuff": false, "foo": false }})),
        ),
        ("export * as ns from \"source\"", None, None),
        ("import.meta", None, None),
        ("let a; class C { static {} } a;", None, None),
        ("var a; class C { static {} } a;", None, None),
        ("a; class C { static {} } var a;", None, None),
        ("class C { static { C; } }", None, None),
        ("const C = class { static { C; } }", None, None),
        ("class C { static { a; } } var a;", None, None),
        ("class C { static { a; } } let a;", None, None),
        ("class C { static { var a; a; } }", None, None),
        ("class C { static { a; var a; } }", None, None),
        ("class C { static { a; { var a; } } }", None, None),
        ("class C { static { let a; a; } }", None, None),
        ("class C { static { a; let a; } }", None, None),
        ("class C { static { function a() {} a; } }", None, None),
        ("class C { static { a; function a() {} } }", None, None),
        ("String;Array;Boolean;", None, None),
        ("[Float16Array, Iterator]", None, None), // es2025
        // arguments should not be reported in regular functions
        ("function test() { return arguments; }", None, None),
        ("var fn = function() { return arguments[0]; };", None, None),
        ("const obj = { method() { return arguments.length; } };", None, None),
        // arguments in nested block scope within function should not be reported
        ("function correct(a) { { return arguments; } }", None, None),
        ("function test() { if (true) { return arguments[0]; } }", None, None),
        ("function test() { for (let i = 0; i < 1; i++) { return arguments; } }", None, None),
        // ("AsyncDisposableStack; DisposableStack; SuppressedError", None, None), / es2026
        ("function resolve<T>(path: string): T { return { path } as T; }", None, None),
        ("let xyz: NodeListOf<HTMLElement>", None, None),
        ("type Foo = Record<string, unknown>;", None, None),
        (
            "export interface StoreImpl { onOutputBlobs: (callback: (blobs: MediaSetBlobs) => void) => import('rxjs').Subscription; }",
            None,
            None,
        ),
    ];

    let fail = vec![
        ("a = 1;", None, None),
        // ("if (typeof anUndefinedVar === 'string') {}", Some(serde_json::json!({"typeof": true})), None), // should fail on `anUndefinedVar`
        ("var a = b;", None, None),
        ("function f() { b; }", None, None),
        ("window;", None, None),
        // ("Intl;", None, None), builtin
        ("require(\"a\");", None, None),
        ("var React; React.render(<img attr={a} />);", None, None),
        ("var React, App; React.render(<App attr={a} />);", None, None),
        ("[a] = [0];", None, None),
        ("({a} = {});", None, None),
        ("({b: a} = {});", None, None),
        ("[obj.a, obj.b] = [0, 1];", None, None),
        ("const c = 0; const a = {...b, c};", None, None),
        ("class C { static { a; } }", None, None),
        ("class C { static { { let a; } a; } }", None, None),
        ("class C { static { { function a() {} } a; } }", None, None),
        ("class C { static { function foo() { var a; }  a; } }", None, None),
        ("class C { static { var a; } static { a; } }", None, None),
        ("class C { static { let a; } static { a; } }", None, None),
        ("class C { static { function a(){} } static { a; } }", None, None),
        ("class C { static { var a; } foo() { a; } }", None, None),
        ("class C { static { let a; } foo() { a; } }", None, None),
        ("class C { static { var a; } [a]; }", None, None),
        ("class C { static { let a; } [a]; }", None, None),
        ("class C { static { function a() {} } [a]; }", None, None),
        ("class C { static { var a; } } a;", None, None),
        ("toString()", None, None),
        ("hasOwnProperty()", None, None),
        ("export class Foo{ bar: notDefined; }; const t = r + 1;", None, None),
        // arguments should be reported in arrow functions (they don't have their own arguments)
        ("const arrow = () => arguments;", None, None),
        // arguments outside functions should be reported
        ("var a = arguments;", None, None),
    ];

    Tester::new(NoUndef::NAME, NoUndef::PLUGIN, pass, fail).test_and_snapshot();

    let pass = vec![(
        "if (typeof anUndefinedVar === 'string') {}",
        Some(serde_json::json!([{ "typeof": false }])),
    )];
    let fail = vec![(
        "if (typeof anUndefinedVar === 'string') {}",
        Some(serde_json::json!([{ "typeof": true }])),
    )];

    Tester::new(NoUndef::NAME, NoUndef::PLUGIN, pass, fail).test();

    let pass = vec![("foo", None, Some(serde_json::json!({ "globals": { "foo": "readonly" } })))];
    let fail = vec![("foo", None, Some(serde_json::json!({ "globals": { "foo": "off" } })))];

    Tester::new(NoUndef::NAME, NoUndef::PLUGIN, pass, fail).test();
}

#[test]
fn test_svelte() {
    use crate::tester::Tester;

    let pass = vec![
        // Svelte 5 runes, including their members.
        "<script>
            function log() {}
            let { label } = $props();
            let count = $state(0);
            let frozen = $state.raw({});
            let doubled = $derived(count * 2);
            let tripled = $derived.by(() => count * 3);
            $effect(() => log(doubled, tripled, frozen, label));
            $effect.pre(() => {});
            $inspect(count);
         </script>",
        "<script>
            function log() {}
            let done = $bindable(false);
            const id = $props.id();
            log(done, id, $host());
         </script>",
        // Svelte 3/4 component pseudo-globals.
        "<script>
            function log() {}
            log($$props, $$restProps, $$slots);
         </script>",
        // `$store` auto-subscription: defined because `count` is.
        "<script>
            function log() {}
            import { writable } from 'svelte/store';
            const count = writable(0);
            log($count);
         </script>",
        "<script>
            function log() {}
            import { page } from '$app/stores';
            log($page.url);
         </script>",
        // A top-level `$:` assignment declares the binding.
        "<script>
            function log() {}
            let a = 1;
            $: doubled = a * 2;
            $: log(doubled);
         </script>",
    ];

    let fail = vec![
        // A plain typo is still a typo.
        "<script>
            function log() {}
            log(notDefinedAnywhere);
         </script>",
        // `$foo` is only defined when `foo` is a binding in the script.
        "<script>
            function log() {}
            log($notAStore);
         </script>",
        // `$$` pseudo-globals are an exact list.
        "<script>
            function log() {}
            log($$madeUp);
         </script>",
        // A `$` label inside a function is not a reactive statement, so it
        // declares nothing.
        "<script>
            function log() {}
            function f() {
                $: nested = 1;
            }
            log(nested);
         </script>",
    ];

    Tester::new(NoUndef::NAME, NoUndef::PLUGIN, pass, fail).change_rule_path("test.svelte").test();
}

#[test]
fn test_svelte_module() {
    use crate::tester::Tester;

    // Runes are also available in `.svelte.js` / `.svelte.ts` modules.
    let pass = vec!["export const counter = $state({ n: 0 });"];

    Tester::new(NoUndef::NAME, NoUndef::PLUGIN, pass, vec![])
        .change_rule_path("counter.svelte.ts")
        .test();
}

#[test]
fn test_vue() {
    use crate::tester::Tester;

    let pass = vec![
        "<script setup>
            function log() {}
            const props = defineProps({ label: String });
            const emit = defineEmits(['go']);
            const model = defineModel();
            defineExpose({ emit });
            defineOptions({ name: 'X' });
            defineSlots();
            log(props, model, withDefaults);
         </script>",
    ];

    let fail = vec![
        "<script setup>
            function log() {}
            log(defineWhatever);
         </script>",
    ];

    Tester::new(NoUndef::NAME, NoUndef::PLUGIN, pass, fail).change_rule_path("test.vue").test();
}
