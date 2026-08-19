//! The literal prefix and suffix of a markup expression.
//!
//! Port of `eslint-plugin-svelte`'s `expression-affixes.js`. Given the
//! expression in `class={…}` or `id={…}`, it works out the literal text the
//! value is known to begin and end with, so a stylesheet selector can still
//! be matched against a name the component assembles at runtime.

use oxc_ast::ast::Expression;

/// The literal text an expression's value must begin and end with, as
/// `(prefix, suffix)`. A half is `None` when nothing can be determined, and
/// both being `None` means the expression could produce any string at all.
///
/// `resolve` maps a bare identifier to the expression it was initialised
/// with, and returns `None` for anything it cannot resolve.
pub fn expression_affixes<'a, R>(
    expression: &'a Expression<'a>,
    resolve: &R,
) -> (Option<&'a str>, Option<&'a str>)
where
    R: Fn(&str) -> Option<&'a Expression<'a>>,
{
    let mut resolving = Vec::new();
    let prefix = prefix_literal(expression, resolve, &mut resolving);
    let suffix = suffix_literal(expression, resolve, &mut resolving);
    (prefix, suffix)
}

fn prefix_literal<'a, R>(
    expression: &'a Expression<'a>,
    resolve: &R,
    resolving: &mut Vec<&'a str>,
) -> Option<&'a str>
where
    R: Fn(&str) -> Option<&'a Expression<'a>>,
{
    match expression {
        // `a + b` begins with whatever `a` begins with.
        Expression::BinaryExpression(binary) => prefix_literal(&binary.left, resolve, resolving),
        Expression::Identifier(identifier) => {
            let initializer = enter_variable(identifier.name.as_str(), resolve, resolving)?;
            let prefix = prefix_literal(initializer, resolve, resolving);
            resolving.pop();
            prefix
        }
        Expression::StringLiteral(literal) => Some(literal.value.as_str()),
        Expression::TemplateLiteral(template) => {
            // Walk the template in source order — quasi, expression, quasi,
            // … — and take the first part that contributes any text.
            for (index, quasi) in template.quasis.iter().enumerate() {
                if !quasi.value.raw.is_empty() {
                    return Some(quasi.value.raw.as_str());
                }
                if let Some(expression) = template.expressions.get(index) {
                    return prefix_literal(expression, resolve, resolving);
                }
            }
            None
        }
        _ => None,
    }
}

fn suffix_literal<'a, R>(
    expression: &'a Expression<'a>,
    resolve: &R,
    resolving: &mut Vec<&'a str>,
) -> Option<&'a str>
where
    R: Fn(&str) -> Option<&'a Expression<'a>>,
{
    match expression {
        // `a + b` ends with whatever `b` ends with.
        Expression::BinaryExpression(binary) => suffix_literal(&binary.right, resolve, resolving),
        Expression::Identifier(identifier) => {
            let initializer = enter_variable(identifier.name.as_str(), resolve, resolving)?;
            let suffix = suffix_literal(initializer, resolve, resolving);
            resolving.pop();
            suffix
        }
        Expression::StringLiteral(literal) => Some(literal.value.as_str()),
        Expression::TemplateLiteral(template) => {
            // The mirror of the prefix walk, from the end backwards.
            for index in (0..template.quasis.len()).rev() {
                let quasi = &template.quasis[index];
                if !quasi.value.raw.is_empty() {
                    return Some(quasi.value.raw.as_str());
                }
                if let Some(expression) =
                    index.checked_sub(1).and_then(|i| template.expressions.get(i))
                {
                    return suffix_literal(expression, resolve, resolving);
                }
            }
            None
        }
        _ => None,
    }
}

/// Resolve an identifier and push it onto the in-progress stack, or return
/// `None` when it does not resolve — including when it is already being
/// resolved, which is how a cyclic chain (`const a = b; const b = a;`)
/// terminates instead of recursing forever.
///
/// The caller pops the stack once it is done with the initialiser.
fn enter_variable<'a, R>(
    name: &'a str,
    resolve: &R,
    resolving: &mut Vec<&'a str>,
) -> Option<&'a Expression<'a>>
where
    R: Fn(&str) -> Option<&'a Expression<'a>>,
{
    if resolving.contains(&name) {
        return None;
    }
    let initializer = resolve(name)?;
    resolving.push(name);
    Some(initializer)
}
