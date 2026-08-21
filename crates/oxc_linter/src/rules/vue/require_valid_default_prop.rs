use cow_utils::CowUtils;
use oxc_ast::{
    AstKind,
    ast::{
        Argument, BindingPattern, Expression, FunctionBody, ObjectExpression, ObjectPropertyKind,
        TSLiteral, TSSignature, TSType,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::Semantic;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    frameworks::FrameworkOptions,
    rule::Rule,
    utils::{find_property, is_vue_component_options_object, static_key_name},
};

/// The constructors Vue accepts as a runtime prop `type`.
const NATIVE_TYPES: [&str; 8] =
    ["String", "Number", "Boolean", "Function", "Object", "Array", "Symbol", "BigInt"];

/// The types whose default must be produced by a factory function, because a
/// literal would be shared between every instance of the component.
const FUNCTION_VALUE_TYPES: [&str; 3] = ["Function", "Object", "Array"];

fn invalid_type_diagnostic(span: Span, name: &str, types: &[&str]) -> OxcDiagnostic {
    let types = types.join(" or ").cow_to_lowercase().into_owned();
    OxcDiagnostic::warn(format!(
        "Type of the default value for '{name}' prop must be a {types}."
    ))
    .with_help("Make the default match the declared type; `Object` and `Array` defaults need a factory function.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireValidDefaultProp;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a prop's `default` to match the prop's declared `type`, and
    /// requires `Object` and `Array` defaults to be produced by a factory
    /// function.
    ///
    /// ### Why is this bad?
    ///
    /// A mismatched default is a runtime warning at best and a wrong value at
    /// worst, and the type says one thing while the code does another.
    ///
    /// The factory requirement is the sharper trap: a default written as an
    /// object or array literal is evaluated once, so *every* instance of the
    /// component shares that one object. Mutating it in one place changes it
    /// everywhere, which looks like spooky action at a distance.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// export default {
    ///   props: {
    ///     name: { type: String, default: 0 },
    ///     tags: { type: Array, default: [] },
    ///   },
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// export default {
    ///   props: {
    ///     name: { type: String, default: 'x' },
    ///     tags: { type: Array, default: () => [] },
    ///   },
    /// }
    /// ```
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// A prop typed through TypeScript is resolved from the type *syntax*
    /// only — keywords, literal types, array and tuple types, function types,
    /// type literals, unions, and a local `interface`, `type` alias or `enum`.
    /// Upstream falls back to the TypeScript checker when the syntax alone is
    /// ambiguous (an imported type, a mapped or conditional type, a generic
    /// parameter); this linter has no checker in this pass, so such a prop
    /// contributes no native type and is skipped rather than guessed at.
    RequireValidDefaultProp,
    vue,
    correctness,
    version = "1.80.0",
    short_description = "Require default value for props to match the declared type.",
);

impl Rule for RequireValidDefaultProp {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectExpression(object) if is_vue_component_options_object(node, ctx) => {
                let Some(props) = find_property(object, "props") else { return };
                let Expression::ObjectExpression(props) = &props.value else { return };
                for (name, definition) in runtime_prop_definitions(props) {
                    let Expression::ObjectExpression(definition) = definition else { continue };
                    let Some(types) = declared_types(definition) else { continue };
                    let Some(default) = find_property(definition, "default") else { continue };
                    check_default(&default.value, &name, &types, false, ctx);
                }
            }
            AstKind::CallExpression(call)
                if ctx.frameworks_options() == FrameworkOptions::VueSetup
                    && matches!(call.callee, Expression::Identifier(_))
                    && call.callee_name() == Some("defineProps") =>
            {
                check_define_props(node, call.arguments.first(), ctx);
            }
            _ => {}
        }
    }
}

fn check_define_props<'a>(
    node: &AstNode<'a>,
    argument: Option<&'a Argument<'a>>,
    ctx: &LintContext<'a>,
) {
    // Runtime form: `defineProps({ x: { type: String, default: 1 } })`.
    if let Some(Expression::ObjectExpression(props)) = argument.and_then(Argument::as_expression) {
        for (name, definition) in runtime_prop_definitions(props) {
            let Expression::ObjectExpression(definition) = definition else { continue };
            let Some(types) = declared_types(definition) else { continue };
            let Some(default) = find_property(definition, "default") else { continue };
            check_default(&default.value, &name, &types, false, ctx);
        }
        return;
    }

    // Type form: `withDefaults(defineProps<Props>(), { … })`, or a
    // destructuring default, both of which name the prop separately from
    // where its type is declared.
    let AstKind::CallExpression(call) = node.kind() else { return };
    let Some(type_arguments) = &call.type_arguments else { return };
    let Some(first_type) = type_arguments.params.first() else { return };

    let mut declared: Vec<(String, Vec<&'static str>)> = Vec::new();
    crate::utils::for_each_define_props_type_signature(
        first_type,
        ctx.semantic(),
        &mut |signature| {
            let TSSignature::TSPropertySignature(signature) = signature else { return };
            let Some(name) = static_key_name(&signature.key) else { return };
            let Some(annotation) = &signature.type_annotation else { return };
            let types = infer_runtime_type(&annotation.type_annotation, ctx.semantic(), 0);
            if !types.is_empty() {
                declared.push((name.into_owned(), types));
            }
        },
    );
    if declared.is_empty() {
        return;
    }

    for (name, defaults) in default_expressions(node, ctx) {
        let Some((_, types)) = declared.iter().find(|(candidate, _)| *candidate == name) else {
            continue;
        };
        for (expression, is_assignment) in defaults {
            check_default(expression, &name, types, is_assignment, ctx);
        }
    }
}

/// The `name -> definition` pairs of a runtime `props` object.
fn runtime_prop_definitions<'e, 'a>(
    props: &'e ObjectExpression<'a>,
) -> Vec<(String, &'e Expression<'a>)> {
    props
        .properties
        .iter()
        .filter_map(|property| {
            let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
            let name = static_key_name(&property.key)?;
            Some((name.into_owned(), &property.value))
        })
        .collect()
}

/// The native type names a runtime `type:` option declares, or `None` when the
/// option is absent or names nothing native — upstream skips both.
fn declared_types(definition: &ObjectExpression<'_>) -> Option<Vec<&'static str>> {
    let type_property = find_property(definition, "type")?;
    let mut types = Vec::new();
    let mut push = |expression: &Expression<'_>| {
        if let Expression::Identifier(identifier) = expression.get_inner_expression()
            && let Some(native) =
                NATIVE_TYPES.into_iter().find(|native| *native == identifier.name.as_str())
            && !types.contains(&native)
        {
            types.push(native);
        }
    };
    match type_property.value.get_inner_expression() {
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                if let Some(expression) = element.as_expression() {
                    push(expression);
                }
            }
        }
        expression => push(expression),
    }
    (!types.is_empty()).then_some(types)
}

/// Every default a `<script setup>` block gives a prop, paired with whether it
/// came from a destructuring assignment (upstream's `src: 'assignment'`) rather
/// than from `withDefaults`.
fn default_expressions<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) -> Vec<(String, Vec<(&'a Expression<'a>, bool)>)> {
    let mut out: Vec<(String, Vec<(&Expression<'_>, bool)>)> = Vec::new();
    let mut add = |name: String, expression: &'a Expression<'a>, is_assignment: bool| {
        if let Some(entry) = out.iter_mut().find(|(candidate, _)| *candidate == name) {
            entry.1.push((expression, is_assignment));
        } else {
            out.push((name, vec![(expression, is_assignment)]));
        }
    };

    let parent = ctx.nodes().parent_node(node.id());
    if let AstKind::CallExpression(outer) = parent.kind()
        && outer.callee_name() == Some("withDefaults")
        && let Some(Expression::ObjectExpression(defaults)) =
            outer.arguments.get(1).and_then(Argument::as_expression)
    {
        for property in &defaults.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else { continue };
            let Some(name) = static_key_name(&property.key) else { continue };
            add(name.into_owned(), &property.value, false);
        }
    }

    // `const { x = 1 } = defineProps<Props>()`.
    let declarator_parent =
        if matches!(parent.kind(), AstKind::CallExpression(_)) { parent } else { node };
    if let AstKind::VariableDeclarator(declarator) = ctx.nodes().parent_kind(declarator_parent.id())
        && let BindingPattern::ObjectPattern(pattern) = &declarator.id
    {
        for property in &pattern.properties {
            let Some(name) = static_key_name(&property.key) else { continue };
            if let BindingPattern::AssignmentPattern(assignment) = &property.value {
                add(name.into_owned(), &assignment.right, true);
            }
        }
    }
    out
}

/// Upstream's `getValueType`, reduced to what the checks below consume.
struct ValueType<'a> {
    /// `Function` for every function form, otherwise the native type.
    type_name: &'static str,
    is_function: bool,
    /// An arrow function with an expression body, whose result is the default.
    expression_return: ExpressionReturn,
    body: Option<&'a FunctionBody<'a>>,
    body_span: Span,
}

/// What an arrow function's expression body evaluates to, when the default is
/// written in that form.
enum ExpressionReturn {
    /// Not an arrow function with an expression body.
    NotApplicable,
    /// An expression body whose type could not be determined.
    Unknown,
    Known(&'static str),
}

fn value_type<'a>(expression: &'a Expression<'a>) -> Option<ValueType<'a>> {
    let plain = |type_name: &'static str| {
        Some(ValueType {
            type_name,
            is_function: false,
            expression_return: ExpressionReturn::NotApplicable,
            body: None,
            body_span: expression.span(),
        })
    };
    match expression.without_parentheses() {
        // `Symbol()`, `Number()`, …
        Expression::CallExpression(call) => {
            let Expression::Identifier(callee) = call.callee.get_inner_expression() else {
                return None;
            };
            NATIVE_TYPES.into_iter().find(|native| *native == callee.name.as_str()).and_then(plain)
        }
        Expression::TemplateLiteral(_) | Expression::StringLiteral(_) => plain("String"),
        Expression::NumericLiteral(_) => plain("Number"),
        Expression::BigIntLiteral(_) => plain("BigInt"),
        Expression::BooleanLiteral(_) => plain("Boolean"),
        Expression::ArrayExpression(_) => plain("Array"),
        Expression::ObjectExpression(_) => plain("Object"),
        Expression::FunctionExpression(function) => Some(ValueType {
            type_name: "Function",
            is_function: true,
            expression_return: ExpressionReturn::NotApplicable,
            body: function.body.as_deref(),
            body_span: function.body.as_ref().map_or(function.span, |body| body.span),
        }),
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.is_expression() {
                let returned = arrow.get_expression().and_then(value_type);
                return Some(ValueType {
                    type_name: "Function",
                    is_function: true,
                    expression_return: returned.map_or(ExpressionReturn::Unknown, |value| {
                        ExpressionReturn::Known(value.type_name)
                    }),
                    body: None,
                    body_span: arrow.get_expression().map_or(arrow.span, GetSpan::span),
                });
            }
            Some(ValueType {
                type_name: "Function",
                is_function: true,
                expression_return: ExpressionReturn::NotApplicable,
                body: arrow.get_function_body(),
                body_span: arrow.get_function_body().map_or(arrow.span, |body| body.span),
            })
        }
        _ => None,
    }
}

/// Upstream's per-default branch of `processPropDefs`, plus the deferred
/// return-type check it does on function exit.
fn check_default<'a>(
    expression: &'a Expression<'a>,
    name: &str,
    types: &[&'static str],
    is_assignment: bool,
    ctx: &LintContext<'a>,
) {
    let Some(value) = value_type(expression) else { return };

    if value.is_function {
        if types.contains(&"Function") {
            return;
        }
        if is_assignment {
            // A destructuring default is the value itself, so a factory
            // function there is simply the wrong type.
            ctx.diagnostic(invalid_type_diagnostic(expression.span(), name, types));
            return;
        }
        match value.expression_return {
            ExpressionReturn::Unknown => return,
            ExpressionReturn::Known(returned) => {
                if types.contains(&returned) {
                    return;
                }
                ctx.diagnostic(invalid_type_diagnostic(value.body_span, name, types));
            }
            ExpressionReturn::NotApplicable => {
                let Some(body) = value.body else { return };
                for returned in returned_expressions(body) {
                    let Some(returned_type) = value_type(returned) else { continue };
                    if types.contains(&returned_type.type_name) {
                        continue;
                    }
                    ctx.diagnostic(invalid_type_diagnostic(returned.span(), name, types));
                }
            }
        }
        return;
    }

    if types.contains(&value.type_name) {
        if is_assignment {
            return;
        }
        if !FUNCTION_VALUE_TYPES.contains(&value.type_name) {
            return;
        }
        // `Object`/`Array` matched, but a literal default is shared between
        // instances, so the factory form is still required.
    }

    // Upstream renames the types it reports for a non-assignment default, so
    // the message asks for the factory function rather than the bare type.
    let expected: Vec<&'static str> = if is_assignment {
        types.to_vec()
    } else {
        types
            .iter()
            .map(
                |type_name| {
                    if FUNCTION_VALUE_TYPES.contains(type_name) { "Function" } else { type_name }
                },
            )
            .collect()
    };
    ctx.diagnostic(invalid_type_diagnostic(expression.span(), name, &expected));
}

/// Every `return <expression>` directly inside `body`, including inside nested
/// blocks but not inside a nested function, which returns for itself.
fn returned_expressions<'e, 'a>(body: &'e FunctionBody<'a>) -> Vec<&'e Expression<'a>> {
    let mut out = Vec::new();
    collect_returns(&body.statements, &mut out);
    out
}

fn collect_returns<'e, 'a>(
    statements: &'e [oxc_ast::ast::Statement<'a>],
    out: &mut Vec<&'e Expression<'a>>,
) {
    use oxc_ast::ast::Statement;
    for statement in statements {
        match statement {
            Statement::ReturnStatement(statement) => {
                if let Some(argument) = &statement.argument {
                    out.push(argument);
                }
            }
            Statement::BlockStatement(block) => collect_returns(&block.body, out),
            Statement::IfStatement(statement) => {
                collect_returns(std::slice::from_ref(&statement.consequent), out);
                if let Some(alternate) = &statement.alternate {
                    collect_returns(std::slice::from_ref(alternate), out);
                }
            }
            Statement::TryStatement(statement) => {
                collect_returns(&statement.block.body, out);
                if let Some(handler) = &statement.handler {
                    collect_returns(&handler.body.body, out);
                }
                if let Some(finalizer) = &statement.finalizer {
                    collect_returns(&finalizer.body, out);
                }
            }
            Statement::SwitchStatement(statement) => {
                for case in &statement.cases {
                    collect_returns(&case.consequent, out);
                }
            }
            _ => {}
        }
    }
}

/// Upstream's `inferRuntimeType`, over the type syntax alone.
fn infer_runtime_type<'a>(
    ts_type: &TSType<'a>,
    semantic: &Semantic<'a>,
    depth: usize,
) -> Vec<&'static str> {
    if depth > 8 {
        return Vec::new();
    }
    match ts_type {
        TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => vec!["String"],
        TSType::TSNumberKeyword(_) => vec!["Number"],
        TSType::TSBooleanKeyword(_) => vec!["Boolean"],
        TSType::TSObjectKeyword(_) | TSType::TSTypeLiteral(_) => vec!["Object"],
        TSType::TSFunctionType(_) => vec!["Function"],
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => vec!["Array"],
        TSType::TSSymbolKeyword(_) => vec!["Symbol"],
        TSType::TSBigIntKeyword(_) => vec!["BigInt"],
        TSType::TSLiteralType(literal) => match &literal.literal {
            TSLiteral::StringLiteral(_) | TSLiteral::TemplateLiteral(_) => vec!["String"],
            // A negated numeric literal (`-1`) arrives as a unary expression.
            TSLiteral::NumericLiteral(_) | TSLiteral::UnaryExpression(_) => vec!["Number"],
            TSLiteral::BigIntLiteral(_) => vec!["BigInt"],
            TSLiteral::BooleanLiteral(_) => vec!["Boolean"],
        },
        TSType::TSUnionType(union) => {
            let mut out: Vec<&'static str> = Vec::new();
            for member in &union.types {
                for inferred in infer_runtime_type(member, semantic, depth + 1) {
                    if !out.contains(&inferred) {
                        out.push(inferred);
                    }
                }
            }
            out
        }
        TSType::TSTypeReference(reference) => {
            let oxc_ast::ast::TSTypeName::IdentifierReference(name) = &reference.type_name else {
                return Vec::new();
            };
            resolve_type_reference(name.name.as_str(), semantic, depth)
        }
        _ => Vec::new(),
    }
}

/// A local `interface` is an object, a local `type` alias is whatever it
/// aliases, and a local `enum` is the kind of its members.
fn resolve_type_reference(name: &str, semantic: &Semantic<'_>, depth: usize) -> Vec<&'static str> {
    for node in semantic.nodes() {
        match node.kind() {
            AstKind::TSInterfaceDeclaration(declaration) if declaration.id.name == name => {
                return vec!["Object"];
            }
            AstKind::TSTypeAliasDeclaration(declaration) if declaration.id.name == name => {
                return infer_runtime_type(&declaration.type_annotation, semantic, depth + 1);
            }
            AstKind::TSEnumDeclaration(declaration) if declaration.id.name == name => {
                // Vue treats a numeric enum as Number and a string enum as
                // String; a mixed one is both.
                let mut out: Vec<&'static str> = Vec::new();
                for member in &declaration.body.members {
                    let inferred =
                        match member.initializer.as_ref().map(Expression::get_inner_expression) {
                            Some(Expression::StringLiteral(_)) => "String",
                            _ => "Number",
                        };
                    if !out.contains(&inferred) {
                        out.push(inferred);
                    }
                }
                return out;
            }
            _ => {}
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RequireValidDefaultProp;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let vue = || Some(PathBuf::from("test.vue"));

        let pass = vec![
            (
                "<script>export default { props: { a: { type: String, default: 'x' }, b: { type: Number, default: 1 }, c: { type: Boolean, default: false } } }</script>",
                None,
                None,
                vue(),
            ),
            // Object/Array defaults through a factory.
            (
                "<script>export default { props: { a: { type: Object, default: () => ({}) }, b: { type: Array, default: () => [] } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>export default { props: { a: { type: Array, default() { return [] } } } }</script>",
                None,
                None,
                vue(),
            ),
            // A Function prop may be a function.
            (
                "<script>export default { props: { a: { type: Function, default: () => 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            // No `type`, or no `default`, is not this rule's business.
            ("<script>export default { props: { a: { default: 1 } } }</script>", None, None, vue()),
            (
                "<script>export default { props: { a: { type: String } } }</script>",
                None,
                None,
                vue(),
            ),
            // A union type accepts either.
            (
                "<script>export default { props: { a: { type: [String, Number], default: 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            // `null` says nothing about the type.
            (
                "<script>export default { props: { a: { type: String, default: null } } }</script>",
                None,
                None,
                vue(),
            ),
            // withDefaults with matching types.
            (
                "<script setup lang=\"ts\">interface Props { msg: string; count?: number }\nconst props = withDefaults(defineProps<Props>(), { msg: 'hi', count: 2 })</script>",
                None,
                None,
                vue(),
            ),
        ];

        let fail = vec![
            (
                "<script>export default { props: { a: { type: String, default: 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            // Object/Array literal defaults are shared between instances.
            (
                "<script>export default { props: { a: { type: Object, default: {} } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>export default { props: { a: { type: Array, default: [] } } }</script>",
                None,
                None,
                vue(),
            ),
            // A factory returning the wrong type.
            (
                "<script>export default { props: { a: { type: String, default: () => 1 } } }</script>",
                None,
                None,
                vue(),
            ),
            (
                "<script>export default { props: { a: { type: Number, default() { return 'x' } } } }</script>",
                None,
                None,
                vue(),
            ),
            // The real-world shape: a TS-typed prop defaulted to the wrong type.
            (
                "<script setup lang=\"ts\">interface Props { borderWidth?: string }\nconst props = withDefaults(defineProps<Props>(), { borderWidth: 1 })</script>",
                None,
                None,
                vue(),
            ),
            // A union that still does not include the default's type.
            (
                "<script>export default { props: { a: { type: [String, Number], default: false } } }</script>",
                None,
                None,
                vue(),
            ),
        ];

        Tester::new(RequireValidDefaultProp::NAME, RequireValidDefaultProp::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
