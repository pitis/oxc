use oxc_macros::declare_oxc_lint;

use crate::{context::LintContext, rule::Rule};

#[derive(Debug, Default, Clone)]
pub struct JsxUsesVars;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Nothing, here. Upstream this rule reports no problems either: its whole
    /// job is to call ESLint's `markVariableAsUsed` for every identifier in a
    /// JSX opening element, so that `no-unused-vars` stops reporting a
    /// component that is only ever referenced from JSX.
    ///
    /// ### Why is this bad?
    ///
    /// It is not — it is a workaround for ESLint's scope analysis not treating
    /// a JSX element name as a reference to the variable it names. `oxc`'s
    /// semantic model resolves JSX identifiers as ordinary references, so
    /// `eslint/no-unused-vars` already counts them and there is nothing left
    /// for this rule to do.
    ///
    /// ### Examples
    ///
    /// This is *correct* code, and reports nothing here or upstream — the
    /// point is that `no-unused-vars` must not report `Foo` either:
    /// ```jsx
    /// import Foo from './Foo'
    /// export default () => <Foo />
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Implemented as a deliberate no-op. It exists so that a config naming
    /// `vue/jsx-uses-vars` — `plugin:vue/base` does, so effectively every Vue
    /// config — resolves instead of failing, since oxlint treats an unknown
    /// rule name as a configuration error. Enabling or disabling it changes
    /// nothing.
    JsxUsesVars,
    vue,
    correctness,
    version = "1.80.0",
    short_description = "Prevent variables used in JSX from being marked as unused.",
);

impl Rule for JsxUsesVars {
    // Intentionally empty: see the rule documentation. A run function has to
    // exist so the rule is dispatched at all — a rule that implements none is
    // rejected as unreachable by `run_less_rules_are_dispatched_elsewhere`.
    fn run_once(&self, _ctx: &LintContext) {}
}

#[test]
fn test() {
    use crate::tester::Tester;

    // `no-unused-vars` counting JSX references is what this rule exists to
    // guarantee, and is covered by that rule's own tests; this one only has to
    // stay silent.
    let pass = vec![
        "import Foo from './Foo'; export default () => <Foo />",
        "const a = 1; export default () => <div>{a}</div>",
        "const unused = 1;",
    ];

    Tester::new(JsxUsesVars::NAME, JsxUsesVars::PLUGIN, pass, vec![]).test();
}
