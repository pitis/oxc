use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode, context::LintContext, rule::Rule,
    utils::is_vue_component_options_object_excluding_instance,
};

fn too_many_components_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("There is more than one component in this file.")
        .with_help("Move each component into a file of its own.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct OneComponentPerFile;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a file to define at most one component.
    ///
    /// ### Why is this bad?
    ///
    /// A file that defines several components cannot be found by the name of
    /// any of them, and tooling that maps a component to a file — the devtools
    /// inspector, hot reload, `no-undef-components`, an editor's go-to-
    /// definition — has nothing to point at.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// Vue.component('foo', { /* … */ })
    /// Vue.component('bar', { /* … */ })
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// Vue.component('foo', { /* … */ })
    /// Vue.mixin({ /* mixins are not components */ })
    /// ```
    OneComponentPerFile,
    vue,
    style,
    version = "1.80.0",
    short_description = "Enforce that each component should be in its own file.",
);

impl Rule for OneComponentPerFile {
    // Whole-file question, so it cannot be answered from a single node.
    fn run_once(&self, ctx: &LintContext) {
        let mut components: Vec<Span> = Vec::new();
        for node in ctx.nodes() {
            let AstKind::ObjectExpression(object) = node.kind() else { continue };
            if !is_vue_component_options_object_excluding_instance(node, ctx) {
                continue;
            }
            // `Vue.mixin({…})` / `app.mixin({…})` declares a mixin, not a
            // component, and upstream leaves it out of the count.
            if is_mixin_argument(node, ctx) {
                continue;
            }
            components.push(object.span);
        }
        if components.len() > 1 {
            for span in components {
                ctx.diagnostic(too_many_components_diagnostic(span));
            }
        }
    }
}

/// Whether this object is the argument of a `*.mixin(…)` call — upstream's
/// `getVueComponentDefinitionType` returning `'mixin'`, which only ever
/// happens for a member-expression callee.
fn is_mixin_argument<'a>(node: &AstNode<'a>, ctx: &LintContext<'a>) -> bool {
    let AstKind::CallExpression(call) = ctx.nodes().parent_kind(node.id()) else { return false };
    let Expression::StaticMemberExpression(callee) = call.callee.get_inner_expression() else {
        return false;
    };
    callee.property.name == "mixin"
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::OneComponentPerFile;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let js = || Some(PathBuf::from("test.js"));
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            ("Vue.component('foo', {})", None, None, js()),
            ("<script>export default {}</script>", None, None, vue()),
            // A mixin is not a component.
            ("Vue.component('foo', {})\nVue.mixin({})", None, None, js()),
            ("app.component('foo', {})\napp.mixin({})", None, None, js()),
            // A plain object is not a component.
            (
                "Vue.component('foo', {})\nconst options = { data() { return {} } }",
                None,
                None,
                js(),
            ),
        ];

        let fail = vec![
            ("Vue.component('foo', {})\nVue.component('bar', {})", None, None, js()),
            ("app.component('foo', {})\napp.component('bar', {})", None, None, js()),
            ("<script>export default {}\nVue.component('bar', {})</script>", None, None, vue()),
        ];

        Tester::new(OneComponentPerFile::NAME, OneComponentPerFile::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
