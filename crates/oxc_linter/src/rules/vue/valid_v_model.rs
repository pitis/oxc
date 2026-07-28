use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, ForStatementLeft, Statement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{SourceType, Span};
use oxc_vue_parser::ast::{AttributeValue, Element, Node};

use crate::{
    rule::Rule,
    utils::{
        directive_modifier_span, directive_value_missing, get_attribute, get_directive,
        is_custom_component,
    },
    vue_template::{VueTemplateContext, VueTemplateRule},
};

/// eslint-plugin-vue `valid-v-model`'s `VALID_MODIFIERS`.
const VALID_MODIFIERS: &[&str] = &["lazy", "number", "trim"];

fn unexpected_invalid_element_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'v-model' directives aren't supported on <{name}> elements."))
        .with_help(
            "Use `v-model` only on form elements (`input`/`select`/`textarea`) or components.",
        )
        .with_label(span)
}

fn unexpected_input_file_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-model' directives don't support 'file' input type.")
        .with_help("File inputs can't be programmatically set; use a `ref` and a `change` listener instead.")
        .with_label(span)
}

fn unexpected_argument_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-model' directives require no argument.")
        .with_help("Remove the argument; native elements don't support `v-model:foo`.")
        .with_label(span)
}

fn unexpected_modifier_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'v-model' directives don't support the modifier '{name}'."))
        .with_help("Native elements only support `.lazy`, `.number`, and `.trim`.")
        .with_label(span)
}

fn missing_value_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-model' directives require that attribute value.")
        .with_help("Give `v-model` a binding target, e.g. `v-model=\"value\"`.")
        .with_label(span)
}

fn unexpected_optional_chaining_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Optional chaining cannot appear in 'v-model' directives.")
        .with_help("`v-model` needs a plain assignable target; remove the `?.`.")
        .with_label(span)
}

fn unexpected_non_lhs_expression_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-model' directives require the attribute value which is valid as LHS.")
        .with_help("Use an identifier or member expression, e.g. `foo` or `foo.bar`.")
        .with_label(span)
}

fn unexpected_null_object_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("'v-model' directive has potential null object property access.")
        .with_help("Ensure the object being accessed can't be `null`/optionally-chained before assigning to it.")
        .with_label(span)
}

fn unexpected_update_iteration_variable_diagnostic(var_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "'v-model' directives cannot update the iteration variable '{var_name}' itself."
    ))
    .with_help("Bind to a property of the iterated item instead, e.g. `item.value`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidVModel;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces valid `v-model` directives in Vue `<template>` blocks:
    /// `v-model` is only allowed on form elements (`input`/`select`/
    /// `textarea`) or components, an `input[type=file]` never supports it,
    /// an argument (`v-model:foo`) is only allowed on components, native
    /// elements only accept the `lazy`/`number`/`trim` modifiers, a value
    /// is required, that value must be a valid assignment target, and it
    /// must not reassign a `v-for` iteration variable directly.
    ///
    /// ### Why is this bad?
    ///
    /// `v-model` compiles down to a value binding plus an update listener;
    /// each of the above shapes either fails to compile, silently does
    /// nothing, or (for a bare `v-for` alias) reassigns a local loop
    /// variable instead of the underlying data, which is thrown away on
    /// the next render.
    ///
    /// ### Deviations from eslint-plugin-vue
    ///
    /// Directive values are parsed with `oxc_parser` (TypeScript syntax
    /// enabled unconditionally, matching this fork's other `valid-v-*`
    /// rules) rather than through vue-eslint-parser's actual per-file
    /// parser selection; a value that fails to parse this way is treated
    /// exactly like upstream's `if (!expression) return` — silently
    /// skipping the parse-dependent checks (optional chaining, LHS
    /// validity, null-object access, iteration-variable reuse) while still
    /// running the non-parse-dependent checks (element validity, `type`,
    /// argument, modifiers, missing value) above it, mirroring upstream's
    /// control flow exactly. Not reproduced: reporting on the modifier
    /// node itself (this AST has no per-modifier span, so
    /// `unexpectedModifier` reports on the whole directive key, like this
    /// fork's other `valid-v-*` rules).
    ///
    /// The iteration-variable check only tracks `v-for` aliases (the
    /// `<alias> in/of <expr>` variables declared by `v-for` on the same
    /// element or an ancestor), not `v-slot` scope variables — matching
    /// upstream's `getVariable`, which walks the same ancestor chain but
    /// is scoped here to what this fork's template AST exposes without
    /// full scope analysis (see `valid-v-for`'s `check_for_value` for the
    /// same alias-parsing mechanism this reuses).
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <div v-model="foo" />
    ///   <input v-model:aaa="foo" />
    ///   <input v-model.aaa="foo" />
    ///   <input type="file" v-model="foo" />
    ///   <input v-model="a + b" />
    ///   <div v-for="item in items">
    ///     <input v-model="item" />
    ///   </div>
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <input v-model="foo" />
    ///   <input type="text" v-model.lazy="foo" />
    ///   <MyComponent v-model:aaa="foo" />
    ///   <div v-for="item in items">
    ///     <input v-model="item.value" />
    ///   </div>
    /// </template>
    /// ```
    ValidVModel,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce valid `v-model` directives.",
);

impl Rule for ValidVModel {}

impl VueTemplateRule for ValidVModel {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        let mut aliases = Vec::new();
        walk_with_for_aliases(nodes, &mut aliases, ctx);
    }
}

/// Depth-first walk that, unlike [`crate::utils::walk_elements`], threads a
/// stack of every ancestor (and self) `v-for` alias name down through
/// `children` — needed by the iteration-variable check, which must know
/// about `v-for` aliases declared anywhere above the `v-model`'d element,
/// not just on it.
fn walk_with_for_aliases<'a>(
    nodes: &[Node<'a>],
    aliases: &mut Vec<String>,
    ctx: &mut VueTemplateContext<'a>,
) {
    for node in nodes {
        let Node::Element(element) = node else { continue };
        check_element(element, aliases, ctx);

        let pushed = get_directive(element, "for", None)
            .and_then(|attribute| attribute.value.as_ref())
            .map_or(0, |value| {
                let names = for_alias_names(value.text);
                let count = names.len();
                aliases.extend(names);
                count
            });

        walk_with_for_aliases(&element.children, aliases, ctx);
        aliases.truncate(aliases.len() - pushed);
    }
}

/// eslint-plugin-vue `isValidElement`.
fn is_valid_element(element: &Element<'_>) -> bool {
    let name = element.name;
    name == "input"
        || name == "select"
        || name == "textarea"
        || (name != "keep-alive"
            && name != "slot"
            && name != "transition"
            && name != "transition-group"
            && is_custom_component(element))
}

fn has_file_type(element: &Element<'_>) -> bool {
    get_attribute(element, "type")
        .is_some_and(|attribute| attribute.value.as_ref().is_some_and(|value| value.text == "file"))
}

fn check_element<'a>(element: &Element<'a>, aliases: &[String], ctx: &mut VueTemplateContext<'a>) {
    for attribute in &element.attributes {
        let Some(directive) = &attribute.directive else { continue };
        if directive.name != "model" {
            continue;
        }

        if !is_valid_element(element) {
            ctx.diagnostic(unexpected_invalid_element_diagnostic(element.name, attribute.span));
        }
        if element.name == "input" && has_file_type(element) {
            ctx.diagnostic(unexpected_input_file_diagnostic(attribute.span));
        }
        if !is_custom_component(element) {
            if let Some(argument) = &directive.argument {
                ctx.diagnostic(unexpected_argument_diagnostic(argument.span));
            }
            for (index, modifier) in directive.modifiers.iter().enumerate() {
                if !VALID_MODIFIERS.contains(modifier) {
                    let span = directive_modifier_span(attribute, ctx.source_text(), index);
                    ctx.diagnostic(unexpected_modifier_diagnostic(modifier, span));
                }
            }
        }

        if directive_value_missing(attribute) {
            ctx.diagnostic(missing_value_diagnostic(attribute.span));
            continue;
        }
        let value = attribute.value.as_ref().expect("checked by directive_value_missing");
        check_expression(value, aliases, ctx);
    }
}

/// eslint-plugin-vue's `isOptionalChainingMemberExpression` / `isLhs` /
/// `maybeNullObjectMemberExpression` chain, plus the
/// `unexpectedUpdateIterationVariable` reference loop — run against the
/// directive value parsed as a standalone expression. A parse failure
/// mirrors upstream's `if (!expression) return`: none of these checks fire.
fn check_expression<'a>(
    value: &AttributeValue<'a>,
    aliases: &[String],
    ctx: &mut VueTemplateContext<'a>,
) {
    let allocator = Allocator::new();
    let snippet = format!("({});", value.text);
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return;
    }
    let Some(Statement::ExpressionStatement(statement)) = parser_ret.program.body.first() else {
        return;
    };
    let expression = &statement.expression;

    if is_optional_chaining_member(expression) {
        ctx.diagnostic(unexpected_optional_chaining_diagnostic(value.span));
    } else if !is_lhs(expression) {
        ctx.diagnostic(unexpected_non_lhs_expression_diagnostic(value.span));
    } else if maybe_null_object_member(expression) {
        ctx.diagnostic(unexpected_null_object_diagnostic(value.span));
    }

    // eslint-plugin-vue only flags a reference whose `.parent` is the
    // `VExpressionContainer` directly — i.e. the *entire* directive value is
    // that one identifier, not just a sub-expression using it (`item.foo`,
    // `foo[item]`, … don't trigger this, only a bare `item` does).
    if let Expression::Identifier(identifier) = expression
        && aliases.iter().any(|alias| alias == identifier.name.as_str())
    {
        ctx.diagnostic(unexpected_update_iteration_variable_diagnostic(
            identifier.name.as_str(),
            value.span,
        ));
    }
}

fn is_optional_chaining_member(expr: &Expression<'_>) -> bool {
    let Expression::ChainExpression(chain) = expr else { return false };
    chain.expression.as_member_expression().is_some()
}

fn is_lhs(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::TSAsExpression(inner) => is_lhs(&inner.expression),
        Expression::TSNonNullExpression(inner) => is_lhs(&inner.expression),
        Expression::Identifier(_) => true,
        _ => expr.as_member_expression().is_some(),
    }
}

fn maybe_null_object_member(expr: &Expression<'_>) -> bool {
    let Some(member) = expr.as_member_expression() else { return false };
    match member.object() {
        Expression::ChainExpression(_) | Expression::NullLiteral(_) => true,
        object if object.as_member_expression().is_some() => maybe_null_object_member(object),
        _ => false,
    }
}

/// Mirrors vue-eslint-parser's `ALIAS_ITERATOR` regex — the first (leftmost)
/// whole-word `in`/`of` immediately preceded by whitespace or `)`. Copied
/// from `valid-v-for`'s `find_for_separator` (kept local rather than
/// promoted to a shared helper, to keep this task's diff scoped to its two
/// rules).
fn find_for_separator(text: &str) -> Option<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (index, &(byte_pos, _)) in chars.iter().enumerate() {
        let preceded_ok = index > 0 && {
            let previous = chars[index - 1].1;
            previous.is_whitespace() || previous == ')'
        };
        if !preceded_ok {
            continue;
        }
        for keyword in ["in", "of"] {
            if !text[byte_pos..].starts_with(keyword) {
                continue;
            }
            let after = byte_pos + keyword.len();
            let word_boundary_ok = match text[after..].chars().next() {
                None => true,
                Some(next) => !(next.is_alphanumeric() || next == '_' || next == '$'),
            };
            if word_boundary_ok {
                return Some((byte_pos, after));
            }
        }
    }
    None
}

/// The `v-for` alias *names* declared by a `v-for="<aliases> in/of <expr>"`
/// value — the same parse-as-a-real-`for`-statement mechanism as
/// `valid-v-for`'s `check_for_value` (see its doc comment for the full
/// rationale), but only extracting identifier names (as owned `String`s,
/// since the synthetic snippet's own lifetime can't outlive this call) for
/// name comparison, not spans for diagnostics. Silently returns nothing on
/// any parse failure, matching `valid-v-for`'s silent-on-parse-failure
/// discipline (there is nothing to report here, so this rule simply doesn't
/// learn about that `v-for`'s aliases).
fn for_alias_names(raw: &str) -> Vec<String> {
    let Some((sep_start, sep_end)) = find_for_separator(raw) else { return Vec::new() };
    let aliases_raw = &raw[..sep_start];
    if aliases_raw.trim().is_empty() {
        return Vec::new();
    }
    let delimiter = &raw[sep_start..sep_end];
    let iterator_raw = &raw[sep_end..];

    let trimmed = aliases_raw.trim();
    let inner = if trimmed.len() >= 2 && trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        aliases_raw
    };

    let snippet = format!("for(let [{inner}]{delimiter}{iterator_raw});");
    let allocator = Allocator::new();
    let parser_ret = Parser::new(&allocator, &snippet, SourceType::ts()).parse();
    if parser_ret.panicked || !parser_ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let left = match parser_ret.program.body.first() {
        Some(Statement::ForInStatement(statement)) => &statement.left,
        Some(Statement::ForOfStatement(statement)) => &statement.left,
        _ => return Vec::new(),
    };
    let ForStatementLeft::VariableDeclaration(declaration) = left else { return Vec::new() };
    let Some(declarator) = declaration.declarations.first() else { return Vec::new() };
    let BindingPattern::ArrayPattern(array_pattern) = &declarator.id else { return Vec::new() };

    let mut names = Vec::new();
    for pattern in array_pattern.elements.iter().flatten() {
        collect_binding_names(pattern, &mut names);
    }
    names
}

fn collect_binding_names(pattern: &BindingPattern<'_>, out: &mut Vec<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => out.push(ident.name.as_str().to_string()),
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_binding_names(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for pattern in array.elements.iter().flatten() {
                collect_binding_names(pattern, out);
            }
            if let Some(rest) = &array.rest {
                collect_binding_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_binding_names(&assignment.left, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ValidVModel;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (r"<template></template>", None, None, Some(PathBuf::from("test.vue"))),
            (
                r#"<template><input v-model="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input type="text" v-model.lazy="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input type="number" v-model.number="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input type="password" v-model.trim="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input type="checkbox" v-model="foo.bar"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input type="radio" v-model="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><textarea v-model="foo"></textarea></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><select v-model="foo"></select></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><your-component v-model="foo"></your-component></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Nested member/computed access through a v-for alias is fine
            // (only a *bare* reassignment of the alias itself is not).
            (
                r#"<template><div><div v-for="x in list"><input v-model="x.foo"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><div v-for="x in list"><input v-model="foo[x]"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><div v-for="x in list"><input v-model="foo[x - 1]"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input :type="a" v-model="b"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-bind:type="a" v-model="b"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model:aaa="a"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model:aaa.modifier="a"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model.modifier="a"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // svg: custom component nested in a namespaced element.
            (
                r#"<template><svg><MyComponent v-model="slotProps"></MyComponent></svg></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Parse failure: no report at all, not even for the (would-be
            // fine) element/argument/modifier checks.
            (
                r#"<template><MyComponent v-model="." /></template>"#,
                None,
                None,
                Some(PathBuf::from("parsing-error.vue")),
            ),
            (
                r#"<template><MyComponent v-model="/**/" /></template>"#,
                None,
                None,
                Some(PathBuf::from("comment-value.vue")),
            ),
            // TS `as`/`!` casts: still valid LHS once unwrapped.
            (
                r#"<template><MyComponent v-model="a as string"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model="a!"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model="a as unknown as string"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        let fail = vec![
            (
                r#"<template><div v-model="foo"></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model:aaa="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model.aaa="foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><input v-model></template>", None, None, Some(PathBuf::from("test.vue"))),
            (
                r#"<template><input v-model="a + b"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input type="file" v-model="b"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Bare iteration-variable reassignment, with and without
            // redundant wrapping parens (parens are transparent).
            (
                r#"<template><div><div v-for="x in list"><input v-model="x"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><div v-for="x in list"><input v-model="(x)"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><div v-for="x in list"><input v-model="(((x)))"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><div><div v-for="e in list"><input v-model="e"></div></div></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model="" /></template>"#,
                None,
                None,
                Some(PathBuf::from("empty-value.vue")),
            ),
            (
                r#"<template><input v-model="foo?.bar"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model="foo?.bar.baz"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model="(a?.b)?.c"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model="(a?.b).c"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model="(null).foo"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><input v-model="(a?.b).c.d"></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (
                r#"<template><MyComponent v-model="a() as string"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Chained non-null assertions: still a `CallExpression` base,
            // still not LHS. Empirically verified against a live
            // eslint-plugin-vue + `@typescript-eslint/parser` run: reports
            // over the *whole* value span, matching this rule.
            (
                r#"<template><MyComponent v-model="a()!!!!"></MyComponent></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(ValidVModel::NAME, ValidVModel::PLUGIN, pass, fail).test_and_snapshot();
    }
}
