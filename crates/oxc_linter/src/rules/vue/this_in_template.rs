use oxc_allocator::Allocator;
use oxc_ast::ast::{ComputedMemberExpression, Expression, StaticMemberExpression};
use oxc_ast_visit::{
    Visit,
    walk::{walk_computed_member_expression, walk_static_member_expression},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use oxc_vue_parser::ast::Node;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    utils::{directive_expression, walk_nodes_with_scope},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

fn unexpected_this_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected usage of 'this'.")
        .with_help("Template expressions already evaluate against the component instance; drop the `this.` prefix, e.g. use `foo` instead of `this.foo`.")
        .with_label(span)
}

fn expected_this_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Expected 'this'.")
        .with_help("Prefix the property reference with `this.`, e.g. `this.foo`.")
        .with_label(span)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum ThisInTemplateOption {
    /// Disallow `this.<prop>` in template expressions (the default).
    #[default]
    #[serde(rename = "never")]
    Never,
    /// Require every component-property reference in template expressions
    /// to be prefixed with `this.`.
    #[serde(rename = "always")]
    Always,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ThisInTemplate(ThisInTemplateOption);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces (or disallows, depending on the `"always"`/`"never"` option)
    /// explicit `this.` usage inside Vue `<template>` expressions —
    /// interpolations and directive values.
    ///
    /// ### Why is this bad?
    ///
    /// Template expressions are always evaluated against the component
    /// instance, so `this.` is redundant there (the default, `"never"`,
    /// disallows it); some codebases prefer the opposite for explicitness
    /// (`"always"`).
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Both `"never"` and `"always"` are implemented. The task brief this
    /// rule was built from anticipated `"always"` might need component
    /// script analysis (the component's own data/computed/methods property
    /// list) and, if so, directed implementing only `"never"`. Reading
    /// upstream's source (`this-in-template.js`) shows `"always"` needs no
    /// such analysis: its handler reports *every* identifier reference in a
    /// `VExpressionContainer` that isn't resolved by the expression's own
    /// local scope stack, with **no** whitelist of actual component
    /// properties, global builtins (`Math`, `Date`, …), or anything else —
    /// so e.g. `{{ Math.max(a, b) }}` is reported as needing
    /// `this.Math.max(this.a, this.b)` by real eslint-plugin-vue too. This
    /// rule reproduces that exact (arguably surprising, but verified from
    /// source) behavior rather than narrowing it with an ad-hoc whitelist,
    /// since that would be a fidelity regression, not an improvement.
    ///
    /// The scope stack tracks both `v-for` alias variables (`item`/`index` in
    /// `v-for="(item, index) in items"`) and `v-slot`/its shorthand `#`/the
    /// deprecated `slot-scope`/`scope` attributes' destructured parameters
    /// (e.g. `<template v-slot="{ item }">`) — see
    /// `crate::utils::walk_nodes_with_scope`'s doc comment for the mechanism.
    /// A directive's own dynamic argument (`:[expr]`) is never checked,
    /// for both options (upstream itself excludes it
    /// only for `"always"`, via a `VDirectiveKey` parent check; this rule
    /// doesn't inspect dynamic-argument expressions at all, which is a
    /// strict subset of what upstream reports and therefore never a false
    /// positive, only a — documented — narrower true-negative for `"never"`
    /// mode's dynamic-argument case specifically).
    ///
    /// This fork's Vue template pass doesn't support autofixing yet (see
    /// `crate::vue_template`'s module doc), so upstream's fixer isn't
    /// reproduced either; the reserved-word/non-identifier property name
    /// skip in `"never"` mode is kept anyway (see `looks_like_bare_identifier`)
    /// because upstream uses it to decide *whether to report at all*, not
    /// just whether the fix is safe — skipping it would over-report relative
    /// to real eslint-plugin-vue.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with the default
    /// `"never"` option:
    /// ```vue
    /// <template>
    ///   <div>{{ this.foo }}</div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule with the default
    /// `"never"` option:
    /// ```vue
    /// <template>
    ///   <div>{{ foo }}</div>
    /// </template>
    /// ```
    ///
    /// Examples of **incorrect** code for this rule with the `"always"`
    /// option:
    /// ```vue
    /// <template>
    ///   <div>{{ foo }}</div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule with the `"always"`
    /// option:
    /// ```vue
    /// <template>
    ///   <div>{{ this.foo }}</div>
    /// </template>
    /// ```
    ThisInTemplate,
    vue,
    style,
    config = ThisInTemplateOption,
    version = "1.77.0",
    short_description = "Disallow (or require) usage of `this` in template.",
);

impl Rule for ThisInTemplate {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value::<DefaultRuleConfig<Self>>(value).map(DefaultRuleConfig::into_inner)
    }
}

impl VueTemplateRule for ThisInTemplate {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let option = self.0;
        walk_nodes_with_scope(nodes, &FxHashSet::default(), &mut |node, scope| match node {
            Node::Element(element) => {
                for attribute in &element.attributes {
                    let Some((text, span)) = directive_expression(attribute) else { continue };
                    check_expression(option, text, span, scope, ctx);
                }
            }
            Node::Interpolation(interpolation) => {
                check_expression(
                    option,
                    interpolation.expression,
                    interpolation.expression_span,
                    scope,
                    ctx,
                );
            }
            _ => {}
        });
    }
}

fn check_expression(
    option: ThisInTemplateOption,
    text: &str,
    container_span: Span,
    scope: &FxHashSet<String>,
    ctx: &mut VueTemplateContext<'_>,
) {
    match option {
        ThisInTemplateOption::Never => check_never(text, container_span, scope, ctx),
        ThisInTemplateOption::Always => check_always(text, container_span, scope, ctx),
    }
}

/// eslint-plugin-vue `this-in-template`'s `"never"` handler: `VExpressionContainer
/// MemberExpression > ThisExpression`, i.e. every `this.<name>` / `this['<name>']`
/// found anywhere in the expression (including inside nested, even non-arrow,
/// functions — upstream's selector has no scope-boundary awareness, so
/// neither does this), reported unless `<name>` is a `v-for` alias currently
/// in scope, a JS reserved word, or not convertible back to a bare
/// identifier reference (see `looks_like_bare_identifier`).
fn check_never(
    text: &str,
    container_span: Span,
    scope: &FxHashSet<String>,
    ctx: &mut VueTemplateContext<'_>,
) {
    let allocator = Allocator::new();
    let Ok(expression) = Parser::new(&allocator, text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
    else {
        return;
    };

    let mut visitor = ThisMemberVisitor { scope, reports: Vec::new() };
    visitor.visit_expression(&expression);
    for this_span in visitor.reports {
        ctx.diagnostic(unexpected_this_diagnostic(Span::new(
            container_span.start + this_span.start,
            container_span.start + this_span.end,
        )));
    }
}

/// eslint-plugin-vue `this-in-template`'s `"always"` handler: every reference
/// in the expression's own free-variable set (`node.references`, here
/// [`free_identifier_references`]) that isn't a `v-for` alias currently in
/// scope is reported on its own span — see this rule's doc comment for why
/// that includes references that aren't actually component properties.
fn check_always(
    text: &str,
    container_span: Span,
    scope: &FxHashSet<String>,
    ctx: &mut VueTemplateContext<'_>,
) {
    for (name, relative_span) in free_identifier_references(text) {
        if scope.contains(&name) {
            continue;
        }
        ctx.diagnostic(expected_this_diagnostic(Span::new(
            container_span.start + relative_span.start,
            container_span.start + relative_span.end,
        )));
    }
}

struct ThisMemberVisitor<'s> {
    scope: &'s FxHashSet<String>,
    reports: Vec<Span>,
}

impl<'a> Visit<'a> for ThisMemberVisitor<'_> {
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Expression::ThisExpression(this_expression) = &it.object {
            self.check(this_expression.span, it.property.name.as_str());
        }
        walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        if let Expression::ThisExpression(this_expression) = &it.object
            && let Some(name) = static_computed_property_name(&it.expression)
        {
            self.check(this_expression.span, &name);
        }
        walk_computed_member_expression(self, it);
    }
}

impl ThisMemberVisitor<'_> {
    /// eslint-plugin-vue's guard on its `unexpected` report: `if
    /// (!propertyName || scopeStack.nodes.some(el => el.name === propertyName)
    /// || RESERVED_NAMES.has(propertyName) || /^\d.*$|[^\w$]/.test(propertyName))
    /// return;` — skip when the property name is empty (`getStaticPropertyName`
    /// can return `""`, e.g. for `this['']`, which is JS-falsy), is in scope
    /// as a `v-for` alias, is a JS reserved word, or doesn't look like a name
    /// that could stand alone as a bare identifier reference.
    fn check(&mut self, this_span: Span, property_name: &str) {
        // Verified against real eslint-plugin-vue 10.9.1: `{{ this[''] }}`
        // reports nothing — `looks_like_bare_identifier("")` is vacuously
        // `true` (an empty string starts with no digit and every character
        // of an empty iterator trivially satisfies `.all(..)`), so this
        // explicit empty check is load-bearing, not redundant with it.
        if property_name.is_empty() {
            return;
        }
        if self.scope.contains(property_name) {
            return;
        }
        if RESERVED_NAMES.contains(&property_name) {
            return;
        }
        if !looks_like_bare_identifier(property_name) {
            return;
        }
        self.reports.push(this_span);
    }
}

/// eslint-plugin-vue's `getStaticPropertyName` for a `ComputedMemberExpression`'s
/// key: a string literal's value, a template literal with no substitutions'
/// cooked text, or (best-effort; no test in this rule's suite exercises a
/// numeric computed key) a numeric literal formatted as an integer when it
/// has no fractional part. Anything else (a non-literal computed key, e.g.
/// `this[x]`) isn't statically known and returns `None`.
fn static_computed_property_name(expression: &Expression) -> Option<String> {
    match expression {
        Expression::StringLiteral(literal) => Some(literal.value.as_str().to_string()),
        Expression::NumericLiteral(literal) => Some(format_numeric_property_name(literal.value)),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            template.quasis.first()?.value.cooked.as_ref().map(|cooked| cooked.as_str().to_string())
        }
        _ => None,
    }
}

/// Best-effort integer formatting for the common `this[0]` case; an
/// out-of-range or non-finite value falls back to `f64`'s own `to_string`.
#[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn format_numeric_property_name(value: f64) -> String {
    if value.is_finite()
        && value.fract() == 0.0
        && (i64::MIN as f64..=i64::MAX as f64).contains(&value)
    {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

/// Mirrors the JS regex `/^\d.*$|[^\w$]/` (a name is *not* convertible to a
/// bare identifier reference if it starts with a digit, or contains any
/// character outside `[A-Za-z0-9_$]`): `true` here means the regex does
/// *not* match, i.e. the name is safe to treat as a bare reference.
fn looks_like_bare_identifier(name: &str) -> bool {
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// eslint-plugin-vue's `RESERVED_NAMES` (`lib/utils/js-reserved.json`): JS
/// reserved words, which can never be a bare identifier reference.
const RESERVED_NAMES: &[&str] = &[
    "abstract",
    "arguments",
    "await",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "function",
    "goto",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "let",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "volatile",
    "while",
    "with",
    "yield",
];

/// eslint-plugin-vue `"always"` mode's `node.references`: every *free*
/// (unresolved — not locally bound within `text` itself, e.g. by a nested
/// arrow function's own parameter) identifier reference in `text`, parsed as
/// a JS expression wrapped in `(<text>);` to dodge the object-literal/
/// block-statement ambiguity at statement position (same trick as
/// `no-use-v-if-with-v-for`'s `expression_reference_names`), with full scope
/// resolution via `oxc_semantic` rather than a plain identifier walk —
/// necessary here because "is this name resolved by *something* in the
/// expression's own scope" can't be answered by a name-only walk (e.g.
/// `() => { function f(x) { return x } }` must not report `x`). Spans are
/// relative to `text` itself (the `(`/`);` wrapper's own length is already
/// subtracted). Silently empty on any parse failure, matching this fork's
/// established silent-on-parse-failure discipline.
fn free_identifier_references(text: &str) -> Vec<(String, Span)> {
    let snippet = format!("({text});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let program = allocator.alloc(parser_ret.program);
    let semantic = SemanticBuilder::new_linter().build(program).semantic;

    let mut out = Vec::new();
    for (name, reference_ids) in semantic.scoping().root_unresolved_references() {
        for reference_id in reference_ids.iter().copied() {
            let reference = semantic.scoping().get_reference(reference_id);
            let span = semantic.reference_span(reference);
            out.push((
                name.as_str().to_string(),
                Span::new(span.start.saturating_sub(1), span.end.saturating_sub(1)),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::ThisInTemplate;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            // Default ("never"): no `this.` usage at all.
            (
                r"<template><div>{{ foo.bar }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-if="foo">{{ foo }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :class="{this: foo}">{{ foo }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Bare `this` (not a member expression) is never this rule's concern.
            (
                r#"<template><div :parent="this"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :parent="this"></div></template>"#,
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Reserved word / non-identifier property names: `this.<name>`
            // is never reported for these under "never", regardless of scope.
            (
                r"<template><div>{{ this.class }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ this['0'] }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ this['this'] }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ this['foo bar'] }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty static property name (`getStaticPropertyName` returns
            // `""`, which is JS-falsy): never reported, regardless of the
            // reserved-word/bare-identifier-shape checks. Verified against
            // real eslint-plugin-vue 10.9.1.
            (
                r"<template><div>{{ this[''] }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Empty interpolation: nothing to parse.
            (r"<template><div>{{ }}</div></template>", None, None, Some(PathBuf::from("test.vue"))),
            // `v-for` alias in scope: `this.x`'s property name coincides
            // with the alias, so it's skipped (upstream's own scope check
            // is purely name-based, verified against real eslint-plugin-vue
            // 10.9.1 — see `this_in_template.rs`'s `ThisMemberVisitor::check`).
            (
                r#"<template><div v-for="x of xs">{{this.x}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="x of xs">{{this.x()}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="x of xs">{{this.x.y()}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="x of xs">{{this.x['foo']}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="x of xs">{{this['x']}}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested `v-for` scopes stack correctly.
            (
                r#"<template><div v-for="ssa in sss"><div v-for="ssf in ssa">{{ ssf }}</div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `v-slot` destructured parameter in scope: `this.foo`'s property
            // name coincides with the slot-scope variable, so — same
            // purely-name-based quirk as the `v-for` alias case above — it's
            // skipped. Reviewer-reported false positive, verified against
            // real eslint-plugin-vue 10.9.1: reports nothing.
            (
                r#"<template><template v-slot:default="{ foo }">{{ this.foo }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Shorthand `#` form; slot-scope variable used several elements
            // deep (scope persists across intermediate elements with no
            // scope of their own).
            (
                r#"<template><template #default="{ foo }"><div><span>{{ this.foo }}</span></div></template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Deprecated `slot-scope` attribute (any element), and `scope`
            // attribute (only recognized on `<template>`).
            (
                r#"<template><div slot-scope="{ foo }">{{ this.foo }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><template scope="{ foo }">{{ this.foo }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Rest element binds its own name into scope.
            (
                r#"<template><template v-slot="{ ...rest }">{{ this.rest }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "always": every reference already prefixed with `this.` is fine.
            (
                r"<template><div>{{ this.foo.bar }}</div></template>",
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-for="foo in this.bar">{{ foo }}</div></template>"#,
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "always": a name bound locally within the expression itself
            // (an arrow function's own nested function parameter) is not a
            // free reference and needs no `this.` prefix — matches upstream
            // scope resolution, not a plain identifier-name walk.
            (
                r#"<template><div v-on="() => { function click(foo) { return foo } }"></div></template>"#,
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            (
                r"<template><div>{{ this.foo }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ this.foo() }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ this.foo.bar() }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div :class="this.foo"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Optional chaining still matches upstream's selector.
            (
                r"<template><div>{{ this?.foo }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `this['xs']` is a normal-looking identifier, not reserved:
            // still reported (unlike `this['0']`/`this['this']` above).
            (
                r"<template><div>{{ this['xs'] }}</div></template>",
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Explicit "never" behaves like the default.
            (
                r"<template><div>{{ this.foo }}</div></template>",
                Some(json!(["never"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // "always": a bare component-property reference needs `this.`.
            (
                r"<template><div>{{ foo }}</div></template>",
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r"<template><div>{{ foo() }}</div></template>",
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div v-if="foo"></div></template>"#,
                Some(json!(["always"])),
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Aliased destructuring (`{ foo: renamed }`) binds only the
            // local name `renamed` into scope, not the source key `foo` — so
            // `this.foo` still needs reporting even inside that slot scope.
            (
                r#"<template><template v-slot="{ foo: renamed }">{{ this.foo }}</template></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // The deprecated `scope` attribute only establishes slot scope
            // on `<template>` — on any other element it's just inert markup
            // (matches `no_deprecated_scope_attribute`'s own restriction),
            // so `this.foo` here is still a genuine, reportable reference.
            (
                r#"<template><div scope="{ foo }">{{ this.foo }}</div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ThisInTemplate::NAME, ThisInTemplate::PLUGIN, pass, fail).test_and_snapshot();
    }
}
