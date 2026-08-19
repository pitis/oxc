use oxc_ast::{
    AstKind,
    ast::{AssignmentOperator, AssignmentTarget, Expression, StaticMemberExpression},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::SymbolId;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    ast_util::outermost_paren_parent,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn prefer_svelte_reactivity_diagnostic(span: Span, class: BuiltinClass) -> OxcDiagnostic {
    let builtin = class.builtin_name();
    let replacement = class.svelte_name();
    OxcDiagnostic::warn(format!(
        "Found a mutable instance of the built-in {builtin} class. Use {replacement} instead."
    ))
    .with_help(format!(
        "Mutations of a plain `{builtin}` are not tracked by Svelte's reactivity; import `{replacement}` from `svelte/reactivity` instead."
    ))
    .with_label(span)
}

/// Built-in classes that have a reactive counterpart in `svelte/reactivity`.
#[derive(Debug, Clone, Copy)]
enum BuiltinClass {
    Date,
    Map,
    Set,
    Url,
    UrlSearchParams,
}

impl BuiltinClass {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Date" => Some(Self::Date),
            "Map" => Some(Self::Map),
            "Set" => Some(Self::Set),
            "URL" => Some(Self::Url),
            "URLSearchParams" => Some(Self::UrlSearchParams),
            _ => None,
        }
    }

    fn builtin_name(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::Map => "Map",
            Self::Set => "Set",
            Self::Url => "URL",
            Self::UrlSearchParams => "URLSearchParams",
        }
    }

    fn svelte_name(self) -> &'static str {
        match self {
            Self::Date => "SvelteDate",
            Self::Map => "SvelteMap",
            Self::Set => "SvelteSet",
            Self::Url => "SvelteURL",
            Self::UrlSearchParams => "SvelteURLSearchParams",
        }
    }

    /// Methods that mutate an instance of this class. For `URL`, mutation
    /// happens through property assignments instead; see
    /// [`URL_MUTABLE_PROPERTIES`].
    fn mutating_methods(self) -> &'static [&'static str] {
        match self {
            Self::Date => &[
                "setDate",
                "setFullYear",
                "setHours",
                "setMilliseconds",
                "setMinutes",
                "setMonth",
                "setSeconds",
                "setTime",
                "setUTCDate",
                "setUTCFullYear",
                "setUTCHours",
                "setUTCMilliseconds",
                "setUTCMinutes",
                "setUTCMonth",
                "setUTCSeconds",
                "setYear",
            ],
            Self::Map => &["clear", "delete", "set"],
            Self::Set => &["add", "clear", "delete"],
            Self::Url => &[],
            Self::UrlSearchParams => &["append", "delete", "set", "sort"],
        }
    }
}

/// Writable `URL` properties; assigning to one of them mutates the instance.
const URL_MUTABLE_PROPERTIES: [&str; 10] = [
    "hash", "host", "hostname", "href", "password", "pathname", "port", "protocol", "search",
    "username",
];

#[derive(Debug, Default, Clone)]
pub struct PreferSvelteReactivity;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows mutable instances of the built-in `Date`, `Map`, `Set`,
    /// `URL` and `URLSearchParams` classes where a reactive alternative is
    /// provided by `svelte/reactivity`. An instance counts as mutable when it
    /// is actually mutated: a mutating method such as `map.set(...)` is
    /// called, or a writable `URL` property is assigned.
    ///
    /// ### Why is this bad?
    ///
    /// Svelte's reactivity does not track mutations of plain built-in
    /// objects, so the UI silently goes stale when they change. The
    /// `SvelteDate`, `SvelteMap`, `SvelteSet`, `SvelteURL` and
    /// `SvelteURLSearchParams` classes from `svelte/reactivity` are drop-in
    /// reactive replacements.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   const entries = new Map();
    ///
    ///   entries.set('key', 'value');
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   import { SvelteMap } from 'svelte/reactivity';
    ///
    ///   const entries = new SvelteMap();
    ///
    ///   entries.set('key', 'value');
    /// </script>
    /// ```
    PreferSvelteReactivity,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Prefer `svelte/reactivity` built-ins inside `$state`.",
);

impl Rule for PreferSvelteReactivity {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::NewExpression(new_expr) = node.kind() else {
            return;
        };
        let Expression::Identifier(ident) = new_expr.callee.get_inner_expression() else {
            return;
        };
        let Some(class) = BuiltinClass::from_name(&ident.name) else {
            return;
        };
        // Shadowed constructors (e.g. `import { Date } from 'package'`) are
        // unrelated to the global built-ins.
        if !ctx.is_reference_to_global_variable(ident) {
            return;
        }

        if is_mutated(class, node, ctx) {
            ctx.diagnostic(prefer_svelte_reactivity_diagnostic(new_expr.span, class));
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

/// Is the instance created by this `new` expression mutated, either directly
/// (`new Map().set(...)`) or through the variable it is stored in?
fn is_mutated<'a>(class: BuiltinClass, new_node: &AstNode<'a>, ctx: &LintContext<'a>) -> bool {
    let Some(parent) = outermost_paren_parent(new_node, ctx.semantic()) else {
        return false;
    };
    match parent.kind() {
        // `new Map().set(...)` / `new URL(...).hash = ...`
        AstKind::StaticMemberExpression(member)
            if member.object.get_inner_expression().span() == new_node.span() =>
        {
            is_mutating_member(class, parent, member, ctx)
        }
        // `const variable = new Map(...);`
        AstKind::VariableDeclarator(declarator) => declarator
            .id
            .get_binding_identifier()
            .is_some_and(|binding| is_symbol_mutated(class, binding.symbol_id(), ctx)),
        // `variable = new Map(...);`
        AstKind::AssignmentExpression(assignment)
            if assignment.operator == AssignmentOperator::Assign =>
        {
            let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left else {
                return false;
            };
            ctx.scoping()
                .get_reference(target.reference_id())
                .symbol_id()
                .is_some_and(|symbol_id| is_symbol_mutated(class, symbol_id, ctx))
        }
        _ => false,
    }
}

/// Does any reference to this variable mutate the instance it holds?
fn is_symbol_mutated(class: BuiltinClass, symbol_id: SymbolId, ctx: &LintContext<'_>) -> bool {
    ctx.scoping().get_resolved_references(symbol_id).any(|reference| {
        if !reference.is_read() {
            return false;
        }
        let ident_node = ctx.nodes().get_node(reference.node_id());
        let Some(member_node) = outermost_paren_parent(ident_node, ctx.semantic()) else {
            return false;
        };
        let AstKind::StaticMemberExpression(member) = member_node.kind() else {
            return false;
        };
        if member.object.get_inner_expression().span() != ident_node.span() {
            return false;
        }
        is_mutating_member(class, member_node, member, ctx)
    })
}

/// Is this (non-computed) member access a mutation: a call of a mutating
/// method, or — for `URL` — an assignment to a writable property?
fn is_mutating_member<'a>(
    class: BuiltinClass,
    member_node: &AstNode<'a>,
    member: &StaticMemberExpression<'a>,
    ctx: &LintContext<'a>,
) -> bool {
    let property = member.property.name.as_str();
    let Some(parent) = outermost_paren_parent(member_node, ctx.semantic()) else {
        return false;
    };
    if matches!(class, BuiltinClass::Url) {
        return URL_MUTABLE_PROPERTIES.contains(&property)
            && matches!(
                parent.kind(),
                AstKind::AssignmentExpression(assignment)
                    if assignment.left.span() == member.span
            );
    }
    class.mutating_methods().contains(&property)
        && matches!(
            parent.kind(),
            AstKind::CallExpression(call)
                if call.callee.get_inner_expression().span() == member.span
        )
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    let pass = vec![
        // Read-only Date usage.
        (
            "<script>
              const variable = new Date(8.64e15);

                console.log(Date.now());
                console.log(Date.parse('1970-01-01T00:00:00Z'));
                console.log(Date.UTC(96, 1, 2, 3, 4, 5));
                console.log(variable.getDate());
                console.log(variable.getDay());
                console.log(variable.getFullYear());
                console.log(variable.getHours());
                console.log(variable.getMilliseconds());
                console.log(variable.getMinutes());
                console.log(variable.getMonth());
                console.log(variable.getSeconds());
                console.log(variable.getTime());
                console.log(variable.getTimezoneOffset());
                console.log(variable.getUTCDate());
                console.log(variable.getUTCDay());
                console.log(variable.getUTCFullYear());
                console.log(variable.getUTCHours());
                console.log(variable.getUTCMilliseconds());
                console.log(variable.getUTCMinutes());
                console.log(variable.getUTCMonth());
                console.log(variable.getUTCSeconds());
                console.log(variable.getYear());
                console.log(variable.toDateString());
                console.log(variable.toISOString());
                console.log(variable.toJSON());
                console.log(variable.toLocaleDateString());
                console.log(variable.toLocaleString());
                console.log(variable.toLocaleTimeString());
                console.log(variable.toString());
                console.log(variable.toTemporalInstant());
                console.log(variable.toTimeString());
                console.log(variable.toUTCString());
                console.log(variable.valueOf());
                console.log(variable[Symbol.toPrimitive]('string'));
            </script>

            {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Read-only Map usage.
        (
            "<script>
                const variable = new Map([[1, 'one'], [2, 'two']]);

                console.log(Map.groupBy(variable, (element) => 'group'));
                console.log(Map[Symbol.species]);
                console.log(variable.entries());
                variable.forEach((value) => {
                    console.log(value);
                });
                console.log(variable.get(1));
                console.log(variable.has(1));
                console.log(variable.keys());
                console.log(variable.values());
                console.log(variable[Symbol.iterator]());
                console.log(variable.size);
            </script>

            {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Read-only Set usage.
        (
            "<script>
                const variable = new Set([1, 2, 1, 3, 3]);
                const other = new Set([1, 2]);

                console.log(Set[Symbol.species]);
                console.log(variable.difference(other));
                console.log(variable.entries());
                variable.forEach((value) => {
                    console.log(value);
                });
                console.log(variable.has(1));
                console.log(variable.intersection(other));
                console.log(variable.isDisjointFrom(other));
                console.log(variable.isSubsetOf(other));
                console.log(variable.isSupersetOf(other));
                console.log(variable.keys());
                console.log(variable.symmetricDifference(other));
                console.log(variable.union(other));
                console.log(variable.values());
                console.log(variable[Symbol.iterator]());
                console.log(variable.size);
            </script>

            {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Read-only URL usage; assignments of URL properties to other
        // variables are reads, not mutations.
        (
            "<script>
                const variable = new URL('https://svelte.dev/');

                console.log(variable.hash);
                console.log(variable.host);
                console.log(variable.hostname);
                console.log(variable.href);
                console.log(variable.origin);
                console.log(variable.password);
                console.log(variable.pathname);
                console.log(variable.port);
                console.log(variable.protocol);
                console.log(variable.search);
                console.log(variable.searchParams);
                console.log(variable.username);
                let unused = 30;
                unused = variable.port;
                console.log(URL.canParse('https://svelte.dev/'));
                objectURL = URL.createObjectURL(new MediaSource());
                console.log(URL.parse('https://svelte.dev/'));
                URL.revokeObjectURL(objectURL);
                console.log(variable.toJSON());
                console.log(variable.toString());
            </script>

            {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Read-only URLSearchParams usage.
        (
            "<script>
                const variable = new URLSearchParams('foo=1&bar=2');

                console.log(variable.size);
                console.log(variable.entries());
                variable.forEach((value, key) => {
                    console.log(key);
                    console.log(value);
                })
                console.log(variable.get('foo'));
                console.log(variable.getAll('foo'));
                console.log(variable.has('foo'));
                console.log(variable.has('foo', '1'));
                console.log(variable.keys());
                console.log(variable.toString());
                console.log(variable.values());
            </script>

            {variable}",
            None,
            None,
            svelte_path(),
        ),
        // svelte/reactivity classes aliased to the built-in names.
        (
            "<script>
              import { SvelteDate as Date } from 'svelte/reactivity';

              const variable = new Date(8.64e15);

              variable.setDate(24);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { SvelteMap as Map } from 'svelte/reactivity';

              const variable = new Map([[1, 'one'], [2, 'two']]);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { SvelteSet as Set } from 'svelte/reactivity';

              const variable = new Set([1, 2, 1, 3, 3]);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { SvelteURL as URL } from 'svelte/reactivity';

              const variable = new URL('https://svelte.dev/');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { SvelteURLSearchParams as URLSearchParams } from 'svelte/reactivity';

              const variable = new URLSearchParams('foo=1&bar=2');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // The svelte/reactivity classes themselves are fine, mutations
        // included.
        (
            "<script>
              import { SvelteMap } from 'svelte/reactivity';

              const variable = new SvelteMap([[1, 'one'], [2, 'two']]);

              variable.set(3, 'three');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // Unrelated classes with built-in names from other packages.
        (
            "<script>
              import { Date } from 'package';

              const variable = new Date(8.64e15);

              variable.setDate(24);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { Map } from 'package';

              const variable = new Map([[1, 'one'], [2, 'two']]);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { Set } from 'package';

              const variable = new Set([1, 2, 1, 3, 3]);
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { URL } from 'package';

              const variable = new URL('https://svelte.dev/');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>
              import { URLSearchParams } from 'package';

              const variable = new URLSearchParams('foo=1&bar=2');
            </script>",
            None,
            None,
            svelte_path(),
        ),
        // A mutating method name on a different, non-tracked object.
        (
            "<script>
              const variable = new Map([[1, 'one']]);
              const other = { set() {} };
              other.set(1, 'two');
            </script>",
            None,
            None,
            svelte_path(),
        ),
    ];

    let fail = vec![
        // Date: every mutating method marks the instance as mutable.
        (
            "<script> const variable = new Date(8.64e15); variable.setDate(24); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setFullYear(1968); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setFullYear(1968, 10, 3); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setHours(23); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setHours(23, 59, 59, 999); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setMilliseconds(999); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setMinutes(59); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setMonth(11); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setSeconds(59); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setTime(123456); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCDate(23); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCFullYear(1968); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCHours(23); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCMilliseconds(420); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCMinutes(59); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCMonth(10); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setUTCSeconds(59); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Date(8.64e15); variable.setYear(1968); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Map.
        (
            "<script> const variable = new Map([[1, 'one'], [2, 'two']]); variable.clear(); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Map([[1, 'one'], [2, 'two']]); variable.delete(1); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Map([[1, 'one'], [2, 'two']]); variable.set(1, 'two'); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Set.
        (
            "<script> const variable = new Set([1, 2, 1, 3, 3]); variable.add(42); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Set([1, 2, 1, 3, 3]); variable.clear(); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new Set([1, 2, 1, 3, 3]); variable.delete(42); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        // URL: assigning any writable property.
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.hash = 'anchor'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.host = 'example.test'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.hostname = 'example.test'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.href = 'https://svelte.dev/'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.password = 'passwd'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.pathname = 'tutorial'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.port = '80'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.protocol = 'https'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.search = 'foo=bar'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URL('https://svelte.dev/'); variable.username = 'usr'; </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        // URLSearchParams.
        (
            "<script> const variable = new URLSearchParams('foo=1&bar=2'); variable.append('baz', '3'); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URLSearchParams('foo=1&bar=2'); variable.delete('foo'); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URLSearchParams('foo=1&bar=2'); variable.set('foo', '-1') </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script> const variable = new URLSearchParams('foo=1&bar=2'); variable.sort(); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
        // Mutating directly on the fresh instance.
        ("<script> new Map([[1, 'one']]).set(2, 'two'); </script>", None, None, svelte_path()),
        (
            "<script> new URL('https://svelte.dev/').hash = 'anchor'; </script>",
            None,
            None,
            svelte_path(),
        ),
        // Instance stored via an assignment instead of a declarator.
        (
            "<script> let variable; variable = new Set([1]); variable.add(2); </script> {variable}",
            None,
            None,
            svelte_path(),
        ),
    ];

    Tester::new(PreferSvelteReactivity::NAME, PreferSvelteReactivity::PLUGIN, pass, fail)
        .test_and_snapshot();
}
