use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPattern, CallExpression, Declaration, Expression, ImportDeclarationSpecifier, Program,
    Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Deserialize;
use svelte_markup_parser::ast::{AttributeKind, AttributeValue, Element, Node, ValuePart};

use crate::{
    rule::{DefaultRuleConfig, Rule},
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::{parse_svelte_expression, svelte_scripts, walk_svelte_elements, walk_svelte_nodes},
};

fn goto_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected goto() call without resolve().")
        .with_help("Wrap the URL with `resolve()` from `$app/paths`.")
        .with_label(span)
}

fn link_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected href link without resolve().")
        .with_help("Wrap the URL with `resolve()` from `$app/paths`, or mark the link with `rel=\"external\"`.")
        .with_label(span)
}

fn push_state_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected pushState() call without resolve().")
        .with_help("Wrap the URL with `resolve()` from `$app/paths` (or pass `''` to keep the current URL).")
        .with_label(span)
}

fn replace_state_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected replaceState() call without resolve().")
        .with_help("Wrap the URL with `resolve()` from `$app/paths` (or pass `''` to keep the current URL).")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoNavigationWithoutResolve {
    /// Whether `goto()` calls are exempt from this rule.
    ignore_goto: bool,
    /// Whether `<a href>` links are exempt from this rule.
    ignore_links: bool,
    /// Whether `pushState()` calls are exempt from this rule.
    ignore_push_state: bool,
    /// Whether `replaceState()` calls are exempt from this rule.
    ignore_replace_state: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires SvelteKit's internal navigation — `<a href>` links and
    /// `goto()` / `pushState()` / `replaceState()` calls — to build their
    /// URLs with `resolve()` (or `asset()`) from `$app/paths`.
    ///
    /// ### Why is this bad?
    ///
    /// A hard-coded internal path like `/foo` breaks as soon as the app is
    /// served under a base path, and it bypasses SvelteKit's route
    /// type-checking. `resolve()` prefixes the base path and validates the
    /// route id. Absolute URLs (`https://…`), pure fragments (`#…`), nullish
    /// values, and links marked `rel="external"` are external navigation and
    /// are allowed as-is; `pushState('')` / `replaceState('')` (shallow
    /// routing on the current URL) are allowed too.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { goto } from '$app/navigation';
    ///   goto('/foo');
    /// </script>
    ///
    /// <a href="/foo">Click me!</a>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { goto } from '$app/navigation';
    ///   import { resolve } from '$app/paths';
    ///   goto(resolve('/foo'));
    /// </script>
    ///
    /// <a href={resolve('/foo')}>Click me!</a>
    /// <a href="https://svelte.dev">External</a>
    /// <a href="#section">Fragment</a>
    /// ```
    ///
    /// ### Options
    ///
    /// This rule takes an object with four boolean properties, each `false`
    /// by default: `ignoreGoto`, `ignoreLinks`, `ignorePushState`, and
    /// `ignoreReplaceState`. Setting one to `true` disables the
    /// corresponding check.
    ///
    /// ```json
    /// {
    ///   "svelte/no-navigation-without-resolve": ["error", { "ignoreLinks": true }]
    /// }
    /// ```
    ///
    /// ### Deviations from `eslint-plugin-svelte`
    ///
    /// - Upstream only enables the rule when `@sveltejs/kit` is installed;
    ///   oxlint checks every `.svelte` file, so only enable this rule in
    ///   SvelteKit projects.
    /// - Upstream can additionally allow values whose TypeScript type is
    ///   `ResolvedPathname` (from `$app/types`); oxlint has no type
    ///   information, so such values are reported.
    /// - Identifiers are resolved through top-level `const`/`let`/`var`
    ///   initializers of the file's `<script>` blocks (upstream uses full
    ///   scope analysis); imports of `goto`/`resolve` etc. are matched by
    ///   local name without shadowing analysis.
    NoNavigationWithoutResolve,
    svelte,
    correctness,
    config = NoNavigationWithoutResolve,
    version = "1.80.0",
    short_description = "Disallow SvelteKit navigation without `resolve()`.",
);

impl Rule for NoNavigationWithoutResolve {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }
}

impl SvelteTemplateRule for NoNavigationWithoutResolve {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let source_text = ctx.source_text();
        let allocator = Allocator::new();

        // Parse the `<script>` blocks: they carry the `$app/paths` /
        // `$app/navigation` imports, the navigation calls, and the top-level
        // variables that link expressions may reference.
        let scripts = svelte_scripts(nodes, source_text);
        let mut programs: Vec<(Program<'_>, u32)> = Vec::new();
        for script in &scripts {
            let source_type = if script.typescript { SourceType::ts() } else { SourceType::mjs() };
            let ret = Parser::new(&allocator, script.content, source_type)
                .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
                .parse();
            if ret.panicked || !ret.diagnostics.is_empty() {
                continue;
            }
            programs.push((ret.program, script.offset));
        }
        let env = build_script_env(&programs);

        let mut diagnostics: Vec<OxcDiagnostic> = Vec::new();

        // goto() / pushState() / replaceState() calls, in the scripts and in
        // template expressions (event handlers etc.).
        if !(self.ignore_goto && self.ignore_push_state && self.ignore_replace_state) {
            let mut visitor = NavCallVisitor {
                env: &env,
                ignore_goto: self.ignore_goto,
                ignore_push_state: self.ignore_push_state,
                ignore_replace_state: self.ignore_replace_state,
                offset: 0,
                diagnostics: &mut diagnostics,
            };
            for (program, offset) in &programs {
                visitor.offset = *offset;
                visitor.visit_program(program);
            }

            let mut expressions: Vec<(&str, u32)> = Vec::new();
            collect_template_expressions(nodes, &mut expressions);
            for (text, offset) in expressions {
                if let Some(expression) = parse_svelte_expression(&allocator, text) {
                    visitor.offset = offset;
                    visitor.visit_expression(&expression);
                }
            }
        }

        // `<a href>` links.
        if !self.ignore_links {
            walk_svelte_elements(nodes, &mut |element| {
                check_link_element(element, &env, &allocator, &mut diagnostics);
            });
        }

        for diagnostic in diagnostics {
            ctx.diagnostic(diagnostic);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavKind {
    Goto,
    PushState,
    ReplaceState,
}

fn nav_kind_from_name(name: &str) -> Option<NavKind> {
    match name {
        "goto" => Some(NavKind::Goto),
        "pushState" => Some(NavKind::PushState),
        "replaceState" => Some(NavKind::ReplaceState),
        _ => None,
    }
}

/// What the file's `<script>` blocks tell us: which local names are the
/// `$app/paths` / `$app/navigation` imports, and top-level variable
/// initializers for identifier resolution.
#[derive(Default)]
struct ScriptEnv<'v, 'a> {
    /// Local names of `resolve` / `asset` imported from `$app/paths`.
    resolve_names: FxHashSet<&'a str>,
    /// Local names of `import * as ns from '$app/paths'`.
    paths_namespaces: FxHashSet<&'a str>,
    /// Local names of `goto` / `pushState` / `replaceState` imported from
    /// `$app/navigation`.
    nav_names: FxHashMap<&'a str, NavKind>,
    /// Local names of `import * as ns from '$app/navigation'`.
    nav_namespaces: FxHashSet<&'a str>,
    /// Top-level `const`/`let`/`var name = init` initializers (upstream uses
    /// full scope analysis; oxlint resolves top-level declarations only).
    vars: FxHashMap<&'a str, &'v Expression<'a>>,
}

fn build_script_env<'v, 'a>(programs: &'v [(Program<'a>, u32)]) -> ScriptEnv<'v, 'a> {
    let mut env = ScriptEnv::default();
    for (program, _) in programs {
        for statement in &program.body {
            match statement {
                Statement::ImportDeclaration(import) => {
                    if import.import_kind.is_type() {
                        continue;
                    }
                    let Some(specifiers) = &import.specifiers else { continue };
                    let source = import.source.value.as_str();
                    if source != "$app/paths" && source != "$app/navigation" {
                        continue;
                    }
                    for specifier in specifiers {
                        match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                                if specifier.import_kind.is_type() {
                                    continue;
                                }
                                let imported = specifier.imported.name();
                                let local = specifier.local.name.as_str();
                                if source == "$app/paths" {
                                    if imported == "resolve" || imported == "asset" {
                                        env.resolve_names.insert(local);
                                    }
                                } else if let Some(kind) = nav_kind_from_name(imported.as_str()) {
                                    env.nav_names.insert(local, kind);
                                }
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                                let local = specifier.local.name.as_str();
                                if source == "$app/paths" {
                                    env.paths_namespaces.insert(local);
                                } else {
                                    env.nav_namespaces.insert(local);
                                }
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                        }
                    }
                }
                Statement::VariableDeclaration(declaration) => {
                    collect_declarators(declaration, &mut env.vars);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::VariableDeclaration(declaration) = &export.declaration {
                        collect_declarators(declaration, &mut env.vars);
                    }
                }
                _ => {}
            }
        }
    }
    env
}

fn collect_declarators<'v, 'a>(
    declaration: &'v oxc_ast::ast::VariableDeclaration<'a>,
    vars: &mut FxHashMap<&'a str, &'v Expression<'a>>,
) {
    for declarator in &declaration.declarations {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(init) = &declarator.init
        {
            vars.entry(id.name.as_str()).or_insert(init);
        }
    }
}

/// Visits every call expression, reporting `goto`/`pushState`/`replaceState`
/// calls whose first argument is not an allowed URL value.
struct NavCallVisitor<'r, 'v, 'a> {
    env: &'r ScriptEnv<'v, 'a>,
    ignore_goto: bool,
    ignore_push_state: bool,
    ignore_replace_state: bool,
    /// File offset of the text the currently visited AST was parsed from.
    offset: u32,
    diagnostics: &'r mut Vec<OxcDiagnostic>,
}

impl<'a> Visit<'a> for NavCallVisitor<'_, '_, 'a> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.check_call(call);
        walk::walk_call_expression(self, call);
    }
}

impl<'a> NavCallVisitor<'_, '_, 'a> {
    fn check_call(&mut self, call: &CallExpression<'a>) {
        let kind = match &call.callee {
            Expression::Identifier(ident) => self.env.nav_names.get(ident.name.as_str()).copied(),
            // `navigation.goto(...)` through a namespace import.
            Expression::StaticMemberExpression(member) => {
                if let Expression::Identifier(object) = &member.object
                    && self.env.nav_namespaces.contains(object.name.as_str())
                {
                    nav_kind_from_name(&member.property.name)
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(kind) = kind else { return };
        let ignored = match kind {
            NavKind::Goto => self.ignore_goto,
            NavKind::PushState => self.ignore_push_state,
            NavKind::ReplaceState => self.ignore_replace_state,
        };
        if ignored {
            return;
        }
        // A call without arguments is not checked, like upstream.
        let Some(first) = call.arguments.first() else { return };
        // `pushState('')` / `replaceState('')` are shallow routing on the
        // current URL; `goto` allows nothing but a resolved path.
        let config = AllowConfig { allow_empty: kind != NavKind::Goto, ..AllowConfig::default() };
        let allowed = first
            .as_expression()
            .is_some_and(|expression| is_value_allowed(expression, self.env, config, &mut vec![]));
        if !allowed {
            let span = first.span();
            let span = Span::new(span.start + self.offset, span.end + self.offset);
            self.diagnostics.push(match kind {
                NavKind::Goto => goto_diagnostic(span),
                NavKind::PushState => push_state_diagnostic(span),
                NavKind::ReplaceState => replace_state_diagnostic(span),
            });
        }
    }
}

/// Every `{expression}` in the template: mustaches, attribute values,
/// directive values, and spreads.
fn collect_template_expressions<'a>(nodes: &[Node<'a>], out: &mut Vec<(&'a str, u32)>) {
    walk_svelte_nodes(nodes, &mut |node| match node {
        Node::Mustache(tag) => out.push((tag.expression, tag.expression_span.start)),
        Node::Element(element) => {
            for attribute in &element.attributes {
                match &attribute.kind {
                    AttributeKind::Plain { value: Some(value), .. } => {
                        push_value_expressions(value, out);
                    }
                    AttributeKind::Directive(directive) => {
                        if let Some(value) = &directive.value {
                            push_value_expressions(value, out);
                        }
                    }
                    AttributeKind::Spread { expression_span, expression } => {
                        out.push((expression, expression_span.start));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    });
}

fn push_value_expressions<'a>(value: &AttributeValue<'a>, out: &mut Vec<(&'a str, u32)>) {
    for part in &value.parts {
        if let ValuePart::Expression(tag) = part {
            out.push((tag.expression, tag.expression_span.start));
        }
    }
}

/// What upstream accepts in an `<a href>` without `resolve()`.
const LINK_CONFIG: AllowConfig = AllowConfig {
    allow_absolute: true,
    allow_empty: false,
    allow_fragment: true,
    allow_nullish: true,
};

/// Checks the `href` of an `<a>` element (components and `svelte:*` elements
/// have different names and are skipped, like upstream).
fn check_link_element(
    element: &Element<'_>,
    env: &ScriptEnv<'_, '_>,
    allocator: &Allocator,
    diagnostics: &mut Vec<OxcDiagnostic>,
) {
    if element.name != "a" || has_rel_external(element, env, allocator) {
        return;
    }
    for attribute in &element.attributes {
        match &attribute.kind {
            AttributeKind::Plain { name, value: Some(value), .. } if *name == "href" => {
                // Upstream checks the first value part; an empty value
                // (`href=""`) has none and is skipped.
                let Some(first) = value.parts.first() else { continue };
                let allowed = match first {
                    ValuePart::Text(text) => {
                        value_is_absolute_url(text.value) || text.value.starts_with('#')
                    }
                    ValuePart::Expression(tag) => {
                        parse_svelte_expression(allocator, tag.expression).is_none_or(
                            |expression| {
                                is_value_allowed(&expression, env, LINK_CONFIG, &mut vec![])
                            },
                        )
                    }
                };
                if !allowed {
                    // Upstream labels the whole `href="…"` attribute.
                    diagnostics.push(link_diagnostic(attribute.span));
                }
            }
            // `<a {href}>` is shorthand for `href={href}`.
            AttributeKind::Shorthand { name, .. } if *name == "href" => {
                let allowed = env.vars.get("href").is_some_and(|init| {
                    is_value_allowed(init, env, LINK_CONFIG, &mut vec!["href"])
                });
                if !allowed {
                    diagnostics.push(link_diagnostic(attribute.span));
                }
            }
            _ => {}
        }
    }
}

fn has_rel_external(element: &Element<'_>, env: &ScriptEnv<'_, '_>, allocator: &Allocator) -> bool {
    for attribute in &element.attributes {
        match &attribute.kind {
            AttributeKind::Plain { name, value: Some(value), .. } if *name == "rel" => {
                match value.parts.first() {
                    Some(ValuePart::Text(text)) => {
                        if text.value.split_whitespace().any(|part| part == "external") {
                            return true;
                        }
                    }
                    Some(ValuePart::Expression(tag)) => {
                        if let Some(expression) = parse_svelte_expression(allocator, tag.expression)
                        {
                            match &expression {
                                Expression::StringLiteral(literal) => {
                                    if literal
                                        .value
                                        .split_whitespace()
                                        .any(|part| part == "external")
                                    {
                                        return true;
                                    }
                                }
                                Expression::Identifier(ident)
                                    if identifier_is_external(ident.name.as_str(), env) =>
                                {
                                    return true;
                                }
                                _ => {}
                            }
                        }
                    }
                    None => {}
                }
            }
            AttributeKind::Shorthand { name, .. }
                if *name == "rel" && identifier_is_external("rel", env) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Upstream requires the variable's initializer to be exactly the string
/// literal `'external'`.
fn identifier_is_external(name: &str, env: &ScriptEnv<'_, '_>) -> bool {
    matches!(env.vars.get(name), Some(Expression::StringLiteral(literal)) if literal.value == "external")
}

#[derive(Debug, Default, Clone, Copy)]
struct AllowConfig {
    allow_absolute: bool,
    allow_empty: bool,
    allow_fragment: bool,
    allow_nullish: bool,
}

/// Port of upstream's `isValueAllowed`: is this expression an acceptable
/// navigation target? `visited` breaks recursive initializer chains (which
/// upstream reports as not allowed).
fn is_value_allowed<'a>(
    expression: &Expression<'a>,
    env: &ScriptEnv<'_, 'a>,
    config: AllowConfig,
    visited: &mut Vec<&'a str>,
) -> bool {
    match expression {
        Expression::Identifier(ident) => {
            let name = ident.name.as_str();
            if let Some(init) = env.vars.get(name) {
                if visited.contains(&name) {
                    return false;
                }
                visited.push(name);
                let allowed = is_value_allowed(init, env, config, visited);
                visited.pop();
                return allowed;
            }
        }
        Expression::ConditionalExpression(conditional) => {
            return is_value_allowed(&conditional.consequent, env, config, visited)
                && is_value_allowed(&conditional.alternate, env, config, visited);
        }
        _ => {}
    }
    (config.allow_absolute && is_absolute_url(expression))
        || (config.allow_empty && is_empty_string(expression))
        || (config.allow_fragment && starts_with_fragment(expression, env, visited))
        || (config.allow_nullish && is_nullish(expression))
        || is_resolve_call(expression, env)
}

/// `resolve(...)` / `asset(...)` (or `ns.resolve(...)`) from `$app/paths`.
fn is_resolve_call(expression: &Expression<'_>, env: &ScriptEnv<'_, '_>) -> bool {
    let Expression::CallExpression(call) = expression else { return false };
    match &call.callee {
        Expression::Identifier(ident) => env.resolve_names.contains(ident.name.as_str()),
        Expression::StaticMemberExpression(member) => {
            member.property.name == "resolve"
                && matches!(&member.object, Expression::Identifier(object) if env.paths_namespaces.contains(object.name.as_str()))
        }
        _ => false,
    }
}

/// Upstream's `/^[+a-z]*:/i`: an optional scheme followed by `:`.
fn value_is_absolute_url(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() && (bytes[index].is_ascii_alphabetic() || bytes[index] == b'+') {
        index += 1;
    }
    index < bytes.len() && bytes[index] == b':'
}

fn is_absolute_url(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::BinaryExpression(binary) => {
            binary.operator == oxc_syntax::operator::BinaryOperator::Addition
                && (is_absolute_url(&binary.left) || is_absolute_url(&binary.right))
        }
        Expression::StringLiteral(literal) => value_is_absolute_url(&literal.value),
        Expression::TemplateLiteral(template) => {
            template.expressions.iter().any(is_absolute_url)
                || template.quasis.iter().any(|quasi| value_is_absolute_url(&quasi.value.raw))
        }
        _ => false,
    }
}

fn is_empty_string(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::StringLiteral(literal) => literal.value.is_empty(),
        Expression::TemplateLiteral(template) => {
            template.expressions.is_empty()
                && template.quasis.len() == 1
                && template.quasis[0].value.raw.is_empty()
        }
        _ => false,
    }
}

fn is_nullish(expression: &Expression<'_>) -> bool {
    match expression {
        // `undefined` is an identifier; `null` is a literal.
        Expression::Identifier(ident) => ident.name == "undefined",
        Expression::NullLiteral(_) => true,
        _ => false,
    }
}

fn starts_with_fragment<'a>(
    expression: &Expression<'a>,
    env: &ScriptEnv<'_, 'a>,
    visited: &mut Vec<&'a str>,
) -> bool {
    match expression {
        Expression::BinaryExpression(binary) => {
            binary.operator == oxc_syntax::operator::BinaryOperator::Addition
                && starts_with_fragment(&binary.left, env, visited)
        }
        Expression::Identifier(ident) => {
            let name = ident.name.as_str();
            if let Some(init) = env.vars.get(name)
                && !visited.contains(&name)
            {
                visited.push(name);
                let starts = starts_with_fragment(init, env, visited);
                visited.pop();
                starts
            } else {
                false
            }
        }
        Expression::StringLiteral(literal) => literal.value.starts_with('#'),
        Expression::TemplateLiteral(template) => {
            template
                .expressions
                .first()
                .is_some_and(|expression| starts_with_fragment(expression, env, visited))
                || template.quasis.first().is_some_and(|quasi| quasi.value.raw.starts_with('#'))
        }
        _ => false,
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        // Resolved navigation and links.
        (
            "<script>
                import { resolve } from '$app/paths';
                import { goto } from '$app/navigation';

                const value = resolve('/foo/');

                goto(resolve('/foo/'));
                goto(value);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
                import { resolve } from '$app/paths';

                const value = resolve('/foo/');
                const href = resolve('/foo/');
            </script>

            <a href={resolve('/foo/')}>Click me!</a>
            <a href={value}>Click me!</a>
            <a {href}>Click me!</a>

            <!-- Testing for attribute without value. -->
            <input type=\"text\" disabled />",
            None,
            None,
            svelte_path(),
        ),
        // Aliased and namespace imports of resolve().
        (
            "<script>
                import { resolve as alias } from '$app/paths';
                import { goto } from '$app/navigation';

                goto(alias('/foo/'));
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
                import * as paths from '$app/paths';
            </script>

            <a href={paths.resolve('/foo/')}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // asset() from $app/paths is also allowed.
        (
            "<script>
                import { asset } from '$app/paths';

                const value = asset('/foo/');
            </script>

            <a href={asset('/foo/')}>Click me!</a>
            <a href={value}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Absolute URLs are external navigation.
        (
            "<script>
                const protocol = 'https';
                const value = \"https://svelte.dev\";
                const href = \"https://svelte.dev\";
            </script>

            <a href=\"http://svelte.dev\">Click me!</a>
            <a href={'https://svelte.dev'}>Click me!</a>
            <a href={'http://svelte' + '.dev'}>Click me!</a>
            <a href={`${protocol}://svelte.dev`}>Click me!</a>
            <a href=\"mailto:user@example.com\">Click me!</a>
            <a href=\"tel:+123456789\">Click me!</a>
            <a href={value}>Click me!</a>
            <a {href}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Fragment-only URLs stay on the page.
        (
            "<script>
                const section = 'sectionName';
                const value = '#section';
                const href = '#section';
            </script>

            <a href=\"#\">Click me!</a>
            <a href=\"#section\">Click me!</a>
            <a href={'#' + section}>Click me!</a>
            <a href={`#${section}`}>Click me!</a>
            <a href={'#user:42'}>Click me!</a>
            <a href={value}>Click me!</a>
            <a {href}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Nullish href renders an inert link.
        (
            "<script>
                const one = undefined;
                const two = null;
                const href = null;
            </script>

            <a href={undefined}>Click me!</a>
            <a href={null}>Click me!</a>
            <a href={one}>Click me!</a>
            <a href={two}>Click me!</a>
            <a {href}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // rel="external" opts a link out, in all its forms.
        (
            "<script>
                const value = 'whatever';
                const href = 'whatever';
                const external = 'external';
                const rel = 'external';
            </script>

            <a href=\"whatever\" rel=\"external\">Click me!</a>
            <a href={value} rel=\"external\">Click me!</a>
            <a {href} rel=\"external\">Click me!</a>
            <a href=\"whatever\" rel={'external'}>Click me!</a>
            <a href=\"whatever\" rel={external}>Click me!</a>
            <a href=\"whatever\" {rel}>Click me!</a>
            <a href=\"whatever\" rel=\"noopener external noreferrer\">Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Ternaries where both branches are allowed.
        (
            "<script>
                const condition = true;
                const url = condition ? 'https://example.com' : '#section';
            </script>

            <a href={condition ? 'https://example.com' : '#section'}>Click me!</a>
            <a href={condition ? '#section' : null}>Click me!</a>
            <a href={url}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Shallow routing with an empty URL.
        (
            "<script>
                import { pushState, replaceState } from '$app/navigation';

                pushState('');
                pushState(``);
                replaceState('', {});
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Calls to something that is not the $app/navigation import.
        (
            "<script>
                function goto(url) {}
                goto('/foo');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // goto() without arguments is not checked.
        (
            "<script>
                import { goto } from '$app/navigation';
                goto();
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Components named like links are not `<a>` elements.
        ("<Link href=\"/foo\">Click me!</Link>", None, None, svelte_path()),
        // Option-dependent cases (enable once options dispatch lands):
        // ("<a href=\"/foo\">Click me!</a>", Some(serde_json::json!([{ "ignoreLinks": true }])), None, svelte_path()),
        // ("<script>import { goto } from '$app/navigation'; goto('/foo');</script>", Some(serde_json::json!([{ "ignoreGoto": true }])), None, svelte_path()),
    ];

    let fail = vec![
        // goto() with unresolved URLs.
        (
            "<script>
                import { goto } from '$app/navigation';

                const value = \"/foo\";

                goto('/foo');
                goto(value);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Links with unresolved URLs, in every value form.
        (
            "<script>
                const value = \"/foo\";
                const href = \"/foo\";
            </script>

            <a href=\"/foo\">Click me!</a>
            <a href={'/foo'}>Click me!</a>
            <a href={'/' + 'foo'}>Click me!</a>
            <a href={value}>Click me!</a>
            <a {href}>Click me!</a>
            <a href={'/user:42'}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Concatenating onto a resolve() result is not a resolve() call.
        (
            "<script>
                import { resolve } from '$app/paths';

                const value = resolve('/foo') + '/bar';
            </script>

            <a href={resolve('/foo') + '/bar'}>Click me!</a>
            <a href={'/foo' + resolve('/bar')}>Click me!</a>
            <a href={value}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // A path that merely contains a fragment is still internal.
        (
            "<a href=\"/foo#section\">Click me!</a>\n<a href={'/foo#section'}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Strings that merely look nullish.
        (
            "<a href=\"undefined\">Click me!</a>\n<a href=\"null\">Click me!</a>\n<a href={`${undefined}`}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Only `+` concatenation can make a URL absolute or a fragment.
        ("<a href={'https://example.com' - '/foo'}>Click me!</a>", None, None, svelte_path()),
        // Ternary with one disallowed branch.
        (
            "<script>
                import { resolve } from '$app/paths';
                import { goto } from '$app/navigation';

                const condition = true;

                goto(condition ? resolve('/foo') : '/bar');
                goto(condition ? '/foo' : '/bar');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // pushState()/replaceState() with a plain path.
        (
            "<script>
                import { pushState, replaceState } from '$app/navigation';

                pushState('/foo', {});
                replaceState('/foo', {});
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Namespace imports of $app/navigation.
        (
            "<script>
                import * as navigation from '$app/navigation';

                navigation.goto('/foo');
                navigation.pushState('/foo', {});
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Aliased goto.
        (
            "<script>
                import { goto as navigate } from '$app/navigation';

                navigate('/foo');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // goto() does not allow absolute or empty URLs (use window.location).
        (
            "<script>
                import { goto } from '$app/navigation';

                goto('https://example.com');
                goto('');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Calls inside template expressions are checked too.
        (
            "<script>
                import { goto } from '$app/navigation';
            </script>

            <button onclick={() => goto('/foo')}>Go</button>",
            None,
            None,
            svelte_path(),
        ),
        // Recursive initializer chains cannot be proven resolved.
        (
            "<script>
                const a = value;
                const value = a;
            </script>

            <a href={value}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Destructured props are not resolvable initializers.
        (
            "<script lang=\"ts\">
                const { href } = $props();
            </script>

            <a {href}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
        // Namespace `paths.asset(...)` is not tracked by upstream either.
        (
            "<script>
                import * as paths from '$app/paths';
            </script>

            <a href={paths.asset('/foo/')}>Click me!</a>",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(NoNavigationWithoutResolve::NAME, NoNavigationWithoutResolve::PLUGIN, pass, fail)
        .test_and_snapshot();
}
