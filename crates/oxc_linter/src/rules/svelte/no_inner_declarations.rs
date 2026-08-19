use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::Rule,
};

fn no_inner_declarations_diagnostic(decl_type: &str, body: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Variable or `function` declarations are not allowed in nested blocks")
        .with_help(format!("Move {decl_type} declaration to {body} root"))
        .with_label(span)
}

/// Determines what type of declarations to check.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum NoInnerDeclarationsConfig {
    /// Disallows function declarations in nested blocks.
    #[default]
    Functions,
    /// Disallows function and var declarations in nested blocks.
    Both,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct NoInnerDeclarationsOptions {
    /// Controls whether function declarations in nested blocks are allowed in strict mode (ES6+ behavior).
    #[schemars(with = "BlockScopedFunctions")]
    block_scoped_functions: Option<BlockScopedFunctions>,
    /// Controls whether declarations directly inside TypeScript namespace or module bodies are allowed.
    #[schemars(with = "Namespaces")]
    namespaces: Option<Namespaces>,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum BlockScopedFunctions {
    /// Allow function declarations in nested blocks in strict mode (ES6+ behavior).
    #[default]
    Allow,
    /// Disallow function declarations in nested blocks regardless of strict mode.
    Disallow,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Namespaces {
    /// Allow declarations directly inside TypeScript namespace or module bodies.
    Allow,
    /// Disallow declarations directly inside TypeScript namespace or module bodies.
    #[default]
    Disallow,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(default)]
pub struct NoInnerDeclarations(NoInnerDeclarationsConfig, NoInnerDeclarationsOptions);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows variable or function declarations in nested blocks inside
    /// the `<script>` blocks of Svelte components.
    ///
    /// ### Why is this bad?
    ///
    /// A `var` or function declaration is permitted anywhere a statement can
    /// go, even nested deeply inside other blocks — including Svelte's `$:`
    /// reactive blocks, which are ordinary nested blocks as far as hoisting
    /// is concerned. Hoisting makes such declarations act as if they were
    /// written at the root, which is confusing; moving them to the root of
    /// the script or of the enclosing function makes the actual scope clear.
    /// Block bindings (`let`, `const`) are not hoisted and are not affected
    /// by this rule.
    ///
    /// This is `eslint/no-inner-declarations` applied to Svelte files;
    /// eslint-plugin-svelte ships it as `svelte/no-inner-declarations`
    /// because the upstream Svelte parser wraps scripts in a non-`Program`
    /// root that breaks the core rule (oxlint parses each script block as
    /// its own program, so the logic is identical to the core rule's).
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <script>
    ///   if (test) {
    ///     function doSomething() {}
    ///   }
    ///   $: {
    ///     function doSomethingElse() {}
    ///   }
    /// </script>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <script>
    ///   function doSomething() {}
    ///   function doSomethingElse() {
    ///     function doAnotherThing() {}
    ///   }
    /// </script>
    /// ```
    ///
    /// ### Options
    ///
    /// Same as `eslint/no-inner-declarations`:
    /// `["functions" | "both", { "blockScopedFunctions": "allow" | "disallow" }]`.
    /// `"both"` also checks `var` declarations; passing an options array with
    /// `blockScopedFunctions` left as `"allow"` skips function declarations
    /// in strict-mode code (Svelte scripts are modules, hence always strict).
    NoInnerDeclarations,
    svelte,
    correctness,
    config = NoInnerDeclarations,
    version = "1.80.0",
    short_description = "Disallow variable/function declarations in nested blocks in Svelte scripts.",
);

impl Rule for NoInnerDeclarations {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let config = value.get(0).and_then(serde_json::Value::as_str).map_or_else(
            NoInnerDeclarationsConfig::default,
            |value| match value {
                "functions" => NoInnerDeclarationsConfig::Functions,
                _ => NoInnerDeclarationsConfig::Both,
            },
        );

        // Options follow the mode string, matching ESLint's positional schema
        // `[("functions" | "both"), { … }]`.
        let block_scoped_functions = if value.is_array() && !value.is_null() {
            value
                .get(1)
                .and_then(|v| v.get("blockScopedFunctions"))
                .and_then(serde_json::Value::as_str)
                .map(|value| match value {
                    "disallow" => BlockScopedFunctions::Disallow,
                    _ => BlockScopedFunctions::Allow,
                })
                .or(Some(BlockScopedFunctions::Allow))
        } else {
            None
        };

        let namespaces =
            value.get(1).and_then(|v| v.get("namespaces")).and_then(serde_json::Value::as_str).map(
                |value| match value {
                    "allow" => Namespaces::Allow,
                    _ => Namespaces::Disallow,
                },
            );

        Ok(Self(config, NoInnerDeclarationsOptions { block_scoped_functions, namespaces }))
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::VariableDeclaration(decl) => {
                if self.0 == NoInnerDeclarationsConfig::Functions || !decl.kind.is_var() {
                    return;
                }

                check_rule(node, ctx, self.1.namespaces == Some(Namespaces::Allow));
            }
            AstKind::Function(func) => {
                if !func.is_function_declaration() {
                    return;
                }

                if self.0 == NoInnerDeclarationsConfig::Functions
                    && self.1.block_scoped_functions == Some(BlockScopedFunctions::Allow)
                {
                    // Modules are always strict mode.
                    // This check is redundant, because in modules, the scope will have strict mode flag set,
                    // but checking source type is cheaper than scope flags lookup, so do the quick check first.
                    if ctx.source_type().is_module() {
                        return;
                    }

                    let scope_id = node.scope_id();
                    let is_strict = ctx.scoping().scope_flags(scope_id).is_strict_mode();
                    if is_strict {
                        return;
                    }
                }

                check_rule(node, ctx, self.1.namespaces == Some(Namespaces::Allow));
            }
            _ => {}
        }
    }

    fn should_run(&self, ctx: &ContextHost) -> bool {
        ctx.file_extension().is_some_and(|ext| ext == "svelte")
    }
}

fn check_rule<'a>(node: &AstNode<'a>, ctx: &LintContext<'a>, allow_namespaces: bool) {
    let parent_node = ctx.nodes().parent_node(node.id());
    let parent_kind = parent_node.kind();

    // A declaration may be wrapped in `export`; look through it to find the real enclosing scope.
    let enclosing = if matches!(
        parent_kind,
        AstKind::ExportDeclaration(_) | AstKind::ExportDefaultDeclaration(_)
    ) {
        ctx.nodes().parent_node(parent_node.id()).kind()
    } else {
        parent_kind
    };

    // Declarations directly inside a TS namespace/module body (exported or not) are governed
    // by the `namespaces` option.
    if matches!(enclosing, AstKind::TSModuleBlock(_)) {
        if allow_namespaces {
            return;
        }
    } else if matches!(
        parent_kind,
        AstKind::Program(_)
            | AstKind::FunctionBody(_)
            | AstKind::StaticBlock(_)
            | AstKind::ExportDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_)
    ) {
        return;
    }

    let mut body = "program";
    let mut parent = ctx.nodes().parent_node(parent_node.id());
    loop {
        match parent.kind() {
            AstKind::Program(_) => break,
            AstKind::StaticBlock(_) => {
                body = "class static block body";
                break;
            }
            AstKind::Function(_) => {
                body = "function body";
                break;
            }
            _ => parent = ctx.nodes().parent_node(parent.id()),
        }
    }

    let (decl_type, span) = match node.kind() {
        AstKind::VariableDeclaration(decl) => {
            let span = Span::sized(decl.span.start, 3); // 3 for "var".len()
            ("variable", span)
        }
        AstKind::Function(func) => {
            let span = Span::sized(func.span.start, 8); // 8 for "function".len()
            ("function", span)
        }
        _ => unreachable!(),
    };

    ctx.diagnostic(no_inner_declarations_diagnostic(decl_type, body, span));
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let svelte_path = || Some(PathBuf::from("test.svelte"));

    // NOTE: with the default configuration only function declarations are
    // checked; `var` cases need `["both"]` and the strict-mode exemption
    // needs `["functions", { "blockScopedFunctions": "allow" }]`. Those
    // option-dependent cases are listed commented out below until options
    // dispatch for this rule lands (regenerated `from_configuration`
    // dispatch via `cargo lintgen`).
    let pass = vec![
        ("<script>\n\tfunction doSomething() {}\n</script>", None, None, svelte_path()),
        // A function body is a valid root.
        (
            "<script>\n\tfunction doSomething() {\n\t\tfunction somethingElse() {}\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        (
            "<script>\n\t(function () {\n\t\tfunction doSomething() {}\n\t})();\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Function expressions are not declarations.
        (
            "<script>\n\tif (test) {\n\t\tvar fn = function () {};\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        ("<script>\n\t$: fn = function () {};\n</script>", None, None, svelte_path()),
        // `var` is only checked with the "both" option.
        ("<script>\n\tif (test) {\n\t\tvar foo;\n\t}\n</script>", None, None, svelte_path()),
        // `let`/`const` are block-scoped and never reported.
        (
            "<script>\n\t$: {\n\t\tlet a = 1;\n\t\tconst b = 2;\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Arrow function bodies are function roots.
        (
            "<script>\n\tfoo(() => {\n\t\tfunction bar() {}\n\t});\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Top-level declarations in both script blocks.
        (
            "<script context=\"module\">\n\tfunction shared() {}\n</script>\n<script>\n\tfunction local() {}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        ("<script>\n\texport function bar() {}\n</script>", None, None, svelte_path()),
        // No script at all.
        ("<div>{content}</div>", None, None, svelte_path()),
        // Option-dependent cases (enable once options dispatch lands):
        // ("<script>\n\tif (test) { let x = 1; }\n</script>", Some(serde_json::json!(["both"])), None, svelte_path()),
        // ("<script>\n\tif (test) { function doSomething() {} }\n</script>", Some(serde_json::json!(["functions", { "blockScopedFunctions": "allow" }])), None, svelte_path()),
    ];

    let fail = vec![
        (
            "<script>\n\tif (test) {\n\t\tfunction doSomething() {}\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Svelte reactive statements create ordinary nested blocks.
        (
            "<script>\n\t$: {\n\t\tfunction doSomething() {}\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Nested block inside a function body reports "function body root".
        (
            "<script>\n\tfunction doSomething() {\n\t\tdo {\n\t\t\tfunction somethingElse() {}\n\t\t} while (test);\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Module scripts are checked the same way.
        (
            "<script context=\"module\">\n\tif (test) {\n\t\tfunction doSomething() {}\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // TypeScript scripts too.
        (
            "<script lang=\"ts\">\n\tif (test) {\n\t\tfunction doSomething() {}\n\t}\n</script>",
            None,
            None,
            svelte_path(),
        ),
        // Option-dependent cases (enable once options dispatch lands):
        // ("<script>\n\tif (foo) var a;\n</script>", Some(serde_json::json!(["both"])), None, svelte_path()),
        // ("<script>\n\twhile (test) {\n\t\tvar foo;\n\t}\n</script>", Some(serde_json::json!(["both"])), None, svelte_path()),
        // ("<script>\n\tif (test) { function doSomething() {} }\n</script>", Some(serde_json::json!(["both", { "blockScopedFunctions": "disallow" }])), None, svelte_path()),
    ];

    Tester::new(NoInnerDeclarations::NAME, NoInnerDeclarations::PLUGIN, pass, fail)
        .test_and_snapshot();
}
