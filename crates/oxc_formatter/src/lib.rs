// NOTE: `inline_always`: Intentional on `FormatWith::fmt` / `FormatOnce::fmt` hot-path dispatch
#![allow(clippy::inline_always)]

mod ast_nodes;
#[cfg(feature = "detect_code_removal")]
mod detect_code_removal;
mod embed_context;
mod formatter;
mod ir_transform;
mod options;
mod parentheses;
mod print;
mod source_text;
mod utils;

use oxc_allocator::Allocator;
use oxc_ast::Comment;
use oxc_ast::ast::*;
use oxc_diagnostics::OxcDiagnostic;
use oxc_formatter_core::{EmbeddedIr, FormatSession, Formatted, InputKind};
use oxc_parser::{ParseOptions, Parser, ParserReturn};
use oxc_span::SourceType;

// Internal only AST-wrapping IR primitives.
// External call-sites use the text-in `format`, `format_fragment`,
// or the special-purpose AST-in `format_program`.
pub(crate) use crate::ast_nodes::{AstNode, AstNodes};
pub use crate::embed_context::{CssInJsTemplate, HtmlEmbedMeta};
// `JsFormatContext` is public solely as the type parameter of the `Formatted`
// returned by `format` / `format_fragment`.
// Its methods are not part of the public contract.
pub use crate::formatter::JsFormatContext;
pub use crate::ir_transform::options::*;
pub use crate::options::*;
#[cfg(feature = "detect_code_removal")]
pub use detect_code_removal::detect_code_removal;
// Re-export the language-agnostic formatting macros from `oxc_formatter_core` so existing
// `crate::write!` / `crate::format_args!` / `crate::best_fitting!`
// call-sites in `oxc_formatter` continue to work without changes.
pub(crate) use oxc_formatter_core::{best_fitting, format_args, write};
// Internal-only re-exports so crate-local `use crate::{Buffer, Format};` continues to work
// without leaking these IR primitives in the public API.
pub(crate) use oxc_formatter_core::{Buffer, Format};

use self::formatter::prelude::tag::Label;
use crate::print::{FormatFunctionParams, FormatTypeParameters};

/// Usage context a JS/TS fragment is placed in js-in-xxx.
/// Drives context-dependent formatting decisions (e.g. forced parentheses, quote style).
///
/// Currently `format_fragment()` callers pass wrapped source.
/// (Prettier's multiparser wraps the fragment before `textToDoc()`);
/// Each variant documents the expected wrap as an input contract.
/// The JS formatter knows nothing about Prettier/Vue vocabulary.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum FragmentContext {
    /// Function params in a binding-LHS position (e.g. Vue `v-for` left).
    /// Parentheses are forced when there are multiple params or a rest element.
    ///
    /// Input wrap: `function _(PARAMS) {}`
    FunctionParamsAsBindingLhs,
    /// Function params in a plain binding position (e.g. Vue `v-slot`).
    ///
    /// Input wrap: `function _(PARAMS) {}`
    FunctionParamsAsBinding,
    /// Type parameters in a standard declaration position (e.g. Vue `<script generic>`).
    ///
    /// Input wrap: `type T<PARAMS> = any`
    TypeParameters,
    /// A bare expression (e.g. Vue `v-if` / `v-bind` values, `{{ ... }}` interpolations).
    ///
    /// Input: the raw expression text, NOT pre-wrapped.
    /// (Prettier's `__js_expression` parser family receives the bare text,
    /// so there is no host-side wrap to mirror;
    /// `format_fragment` wraps internally as `(EXPR\n);` to parse it.)
    ///
    /// `in_html_attribute` selects the preferred quote style:
    /// inside a double-quoted attribute single quotes are preferred,
    /// while outside one (an interpolation) the configured quote style is kept.
    ///
    /// `vue_expression` marks fragments from Prettier's `__vue_expression` /
    /// `__vue_ts_expression` parsers (`v-bind` values and `{{ ... }}`
    /// interpolations, but NOT `v-for` right-hand sides or event handlers).
    /// It enables the Vue 2 filter-sequence layout for top-level `|` chains.
    Expression { in_html_attribute: bool, vue_expression: bool },
    /// Statement(s) from an inline event handler (e.g. Vue `@click` values that
    /// do not parse as a single expression).
    ///
    /// Input: the raw statements text, NOT pre-wrapped.
    ///
    /// A lone expression statement keeps or drops its trailing semicolon
    /// following the same rule as Prettier's `__vue_event_binding` parser
    /// (which mirrors the Vue compiler's inline-handler detection):
    /// the semicolon is printed only when the expression is a function/arrow
    /// expression, a member expression, or an identifier other than `undefined`.
    /// This is semantic, not stylistic: in Vue, a handler value with a trailing
    /// semicolon compiles as an inline statement rather than a method reference.
    EventHandlerStatements,
}

/// Classification of the root node of a [`FragmentContext::Expression`] fragment.
///
/// Embedders need this for Prettier's hug-vs-expand layout decision
/// (`shouldHugJsExpression` in `language-html/embed/utilities.js`):
/// object/array literals hug the attribute quotes, other expressions expand
/// with an indented soft line break. Variant names match the estree node types
/// the decision is keyed on. Template/string literals only hug when the
/// fragment came from Prettier's `__vue_expression` (Vue) or `ng` (Angular)
/// parsers, unlike object/array literals which hug unconditionally; callers
/// outside those embeds should treat `TemplateLiteral`/`StringLiteral` as
/// `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionRootKind {
    ObjectExpression,
    ArrayExpression,
    TemplateLiteral,
    StringLiteral,
    Other,
}

/// Embedded-fragment flags threaded into [`JsFormatContext`] by [`format_node`].
#[derive(Clone, Copy, Debug, Default)]
struct EmbedFlags {
    /// See [`JsFormatContext::embedded_in_html_attribute`].
    in_html_attribute: bool,
    /// See [`JsFormatContext::embedded_vue_expression`].
    vue_expression: bool,
    /// See [`JsFormatContext::embedded_in_html_interpolation`].
    in_html_interpolation: bool,
}

/// Result of [`format_fragment`]: the formatted IR plus fragment metadata.
pub struct FormattedFragment<'a> {
    pub formatted: Formatted<'a, JsFormatContext<'a>>,
    /// `Some` for [`FragmentContext::Expression`] and
    /// [`FragmentContext::EventHandlerStatements`], `None` for the other contexts.
    /// An event-handler fragment always reports [`ExpressionRootKind::Other`]:
    /// its root is the statement list, which never hugs.
    pub expression_root: Option<ExpressionRootKind>,
}

/// Format an entire JS/TS program from source text, text-in entry point.
///
/// # Errors
/// Returns the first parse error as an [`OxcDiagnostic`].
/// For now, any parse diagnostic is an error, even when the parser could recover.
pub fn format<'a>(
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
    options: JsFormatOptions,
) -> Result<Formatted<'a, JsFormatContext<'a>>, OxcDiagnostic> {
    // Compatibility wrapper: a service-less `PhysicalFile` session,
    // so embedded languages stay as-is.
    // Hosts that install services (oxfmt) use [`format_with_session`].
    format_with_session(
        &FormatSession::new(allocator, InputKind::PhysicalFile),
        source_text,
        source_type,
        options,
    )
}

/// Like [`format()`], but on a caller-supplied [`FormatSession`]:
/// the root whose session carries the host's `SessionServices`
/// (embedded dispatch, string embedding, Tailwind sorting).
///
/// # Errors
/// Same as [`format()`].
pub fn format_with_session<'a>(
    session: &FormatSession<'a>,
    source_text: &'a str,
    source_type: SourceType,
    options: JsFormatOptions,
) -> Result<Formatted<'a, JsFormatContext<'a>>, OxcDiagnostic> {
    let program = parse(session.allocator(), source_text, source_type)?;
    Ok(format_program_with_session(session, program, options))
}

/// Parse `source_text` as a JS/TS program and build the formatter IR for
/// embedding into another formatter's document — a `.svelte` component's
/// `<script>`, say.
///
/// Unlike [`format()`], this:
/// - allocates from the session's shared arena and `GroupId` space, so the
///   IR lives as long as the parent's document and its group ids cannot
///   collide with the parent's
/// - emits neither a BOM nor the trailing newline (the parent owns the
///   surrounding layout)
///
/// The returned [`EmbeddedIr`] carries the pre-sort Tailwind classes its
/// `TailwindClass(index)` elements refer to; the parent document owns the
/// batch sort and re-indexes them (`DispatchPayload::into_doc`).
///
/// # Errors
/// Same as [`format()`]: the first parse error.
pub fn format_to_ir<'a>(
    session: &FormatSession<'a>,
    source_text: &str,
    source_type: SourceType,
    options: JsFormatOptions,
) -> Result<EmbeddedIr<'a>, OxcDiagnostic> {
    let allocator = session.allocator();
    let source_text: &'a str = allocator.alloc_str(source_text);
    let program = parse(allocator, source_text, source_type)?;
    let node = AstNode::new(program, AstNodes::Dummy(), allocator);
    let context =
        JsFormatContext::new(program.source_text, program.source_type, &program.comments, options);
    Ok(formatter::format_embedded(
        context,
        session,
        oxc_formatter_core::Arguments::new(&[oxc_formatter_core::Argument::new(&node)]),
    ))
}

/// Format a pre-wrapped JS/TS-in-xxx fragment from source text.
///
/// The caller passes source already wrapped per the [`FragmentContext`] input contract
/// (Prettier wraps it before calling `textToDoc()`);
/// This function parses it, extracts the target node, and formats it with the context-appropriate rules.
///
/// # Errors
/// Returns the first parse error,
/// or an error when the wrapped source doesn't match the shape the context expects.
pub fn format_fragment<'a>(
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
    options: JsFormatOptions,
    context: FragmentContext,
) -> Result<FormattedFragment<'a>, OxcDiagnostic> {
    // A js-in-xxx fragment never owns file envelopes (BOM / front matter).
    let session = FormatSession::new(allocator, InputKind::Fragment);
    let (formatted, expression_root) =
        format_fragment_inner(&session, source_text, source_type, options, context, &ToFormatted)?;
    Ok(FormattedFragment { formatted, expression_root })
}

/// As [`format_fragment`], but on a caller-supplied session and returning IR
/// for a parent document to splice in — the shape a `.svelte` component's
/// `{…}` needs.
///
/// The session must be one derived from the parent's (the dispatcher hands
/// the child exactly that), so the IR shares the parent's arena and
/// `GroupId` space.
///
/// # Errors
/// Same as [`format_fragment`].
pub fn format_fragment_to_ir<'a>(
    session: &FormatSession<'a>,
    source_text: &str,
    source_type: SourceType,
    options: JsFormatOptions,
    context: FragmentContext,
) -> Result<(EmbeddedIr<'a>, Option<ExpressionRootKind>), OxcDiagnostic> {
    let source_text: &'a str = session.allocator().alloc_str(source_text);
    format_fragment_inner(session, source_text, source_type, options, context, &ToEmbeddedIr)
}

/// How a fragment's built content is turned into a result: a printable
/// document, or IR for a parent to splice. The two differ only in the last
/// step, and the branch that builds the content is worth having once.
trait FragmentFinish<'a> {
    type Output;

    fn finish<F: Format<'a, JsFormatContext<'a>>>(
        &self,
        session: &FormatSession<'a>,
        options: JsFormatOptions,
        node: &F,
        source_text: &'a str,
        source_type: SourceType,
        comments: &'a [Comment],
        embed_flags: EmbedFlags,
    ) -> Self::Output;
}

struct ToFormatted;

impl<'a> FragmentFinish<'a> for ToFormatted {
    type Output = Formatted<'a, JsFormatContext<'a>>;

    fn finish<F: Format<'a, JsFormatContext<'a>>>(
        &self,
        session: &FormatSession<'a>,
        options: JsFormatOptions,
        node: &F,
        source_text: &'a str,
        source_type: SourceType,
        comments: &'a [Comment],
        embed_flags: EmbedFlags,
    ) -> Self::Output {
        format_node(session, options, node, source_text, source_type, comments, embed_flags)
    }
}

struct ToEmbeddedIr;

impl<'a> FragmentFinish<'a> for ToEmbeddedIr {
    type Output = EmbeddedIr<'a>;

    fn finish<F: Format<'a, JsFormatContext<'a>>>(
        &self,
        session: &FormatSession<'a>,
        options: JsFormatOptions,
        node: &F,
        source_text: &'a str,
        source_type: SourceType,
        comments: &'a [Comment],
        embed_flags: EmbedFlags,
    ) -> Self::Output {
        let context = JsFormatContext::new(source_text, source_type, comments, options)
            .with_embedded_in_html_attribute(embed_flags.in_html_attribute)
            .with_embedded_vue_expression(embed_flags.vue_expression)
            .with_embedded_in_html_interpolation(embed_flags.in_html_interpolation);
        formatter::format_embedded(
            context,
            session,
            oxc_formatter_core::Arguments::new(&[oxc_formatter_core::Argument::new(node)]),
        )
    }
}

fn format_fragment_inner<'a, Finish: FragmentFinish<'a>>(
    session: &FormatSession<'a>,
    source_text: &'a str,
    source_type: SourceType,
    options: JsFormatOptions,
    context: FragmentContext,
    finish: &Finish,
) -> Result<(Finish::Output, Option<ExpressionRootKind>), OxcDiagnostic> {
    let allocator = session.allocator();
    // `Expression` receives bare text; wrap it so leading `{` parses as an
    // object literal instead of a block, and a trailing `//` comment cannot
    // swallow the closing delimiter. `preserve_parens` is disabled in
    // `parse_for_format`, so the synthetic parens leave no AST node behind.
    let source_text = if matches!(context, FragmentContext::Expression { .. }) {
        allocator.alloc_str(&format!("({source_text}\n);"))
    } else {
        source_text
    };
    let program = parse(allocator, source_text, source_type)?;

    // Fragment contexts sitting inside a double-quoted attribute prefer single
    // quotes to avoid clashing with the surrounding attribute delimiter.
    // Interpolations are not inside an attribute, so they keep the configured style.
    //
    // This is only the *preferred* quote, not a forced one; `is_quote_forced`
    // (utils/string.rs) is what actually prevents the surrounding attribute
    // delimiter from being reintroduced when the string's own content forces
    // the other quote.
    let in_html_attribute =
        !matches!(context, FragmentContext::Expression { in_html_attribute: false, .. });
    let embed_flags = EmbedFlags {
        in_html_attribute,
        vue_expression: matches!(context, FragmentContext::Expression { vue_expression: true, .. }),
        in_html_interpolation: matches!(
            context,
            FragmentContext::Expression { in_html_attribute: false, .. }
        ),
    };
    let options = if in_html_attribute {
        JsFormatOptions { quote_style: QuoteStyle::Single, ..options }
    } else {
        options
    };

    let mut expression_root = None;

    let formatted = match context {
        FragmentContext::FunctionParamsAsBindingLhs | FragmentContext::FunctionParamsAsBinding => {
            let Some(Statement::FunctionDeclaration(func)) = program.body.first() else {
                return Err(OxcDiagnostic::error(
                    "Expected fragment wrapped as `function _(...) {}`",
                ));
            };
            let params = &*func.params;
            // Parens are forced only in the binding-LHS context with multiple params or a rest element.
            let with_parens = matches!(context, FragmentContext::FunctionParamsAsBindingLhs)
                && (1 < params.items.len() || params.rest.is_some());
            let node = AstNode::new(params, AstNodes::Dummy(), allocator);
            let content = FormatFunctionParams::new(&node, with_parens);
            finish.finish(
                session,
                options,
                &content,
                program.source_text,
                source_type,
                &program.comments,
                embed_flags,
            )
        }
        FragmentContext::TypeParameters => {
            let Some(Statement::TSTypeAliasDeclaration(decl)) = program.body.first() else {
                return Err(OxcDiagnostic::error(
                    "Expected fragment wrapped as `type T<...> = any`",
                ));
            };
            let Some(type_params) = decl.type_parameters.as_deref() else {
                return Err(OxcDiagnostic::error("Expected type parameters in wrapped fragment"));
            };
            let node = AstNode::new(type_params, AstNodes::Dummy(), allocator);
            let content = FormatTypeParameters::new(&node);
            finish.finish(
                session,
                options,
                &content,
                program.source_text,
                source_type,
                &program.comments,
                embed_flags,
            )
        }
        FragmentContext::Expression { .. } => {
            // Exactly one statement is required: text like `a); (b` would
            // otherwise wrap into two statements and silently format only the first.
            let [Statement::ExpressionStatement(stmt)] = program.body.as_slice() else {
                return Err(OxcDiagnostic::error("Expected a single expression fragment"));
            };
            let expression = &stmt.expression;
            expression_root = Some(classify_expression_root(expression));
            // Parent the expression under the `Program` node rather than `Dummy`:
            // expression formatting reads the parent (trailing-comment bounds,
            // parenthesization), and a `Program` parent matches no parens rule —
            // the same "root expression" semantics as Prettier's `JsExpressionRoot`.
            let program_node = allocator.alloc(AstNode::new(program, AstNodes::Dummy(), allocator));
            let node = AstNode::new(expression, AstNodes::Program(program_node), allocator);
            finish.finish(
                session,
                options,
                &node,
                program.source_text,
                source_type,
                &program.comments,
                embed_flags,
            )
        }
        FragmentContext::EventHandlerStatements => {
            // The root of an event-handler fragment is the whole statement list
            // (Prettier reports the `Program` node to `__onHtmlBindingRoot`),
            // which is never in the hug list.
            expression_root = Some(ExpressionRootKind::Other);
            // A comments-only handler (e.g. `@click="/* hello */"`) has nothing
            // to format; `Program`'s trailing-comments path would prepend a
            // space, so print the dangling comments directly instead.
            if program.body.is_empty() && program.directives.is_empty() {
                let content = formatter::prelude::format_with(|f| {
                    formatter::trivia::format_dangling_comments(program.span).fmt(f);
                });
                finish.finish(
                    session,
                    options,
                    &content,
                    program.source_text,
                    source_type,
                    &program.comments,
                    embed_flags,
                )
            }
            // A lone expression statement gets the Vue inline-handler semicolon
            // rule; anything else (multiple statements, declarations, directives)
            // is formatted as a plain program under the normal semicolon option.
            else if let ([Statement::ExpressionStatement(stmt)], []) =
                (program.body.as_slice(), program.directives.as_slice())
            {
                let with_semicolon = keeps_event_handler_semicolon(&stmt.expression);
                // See the `Expression` arm for why the parent is the `Program` node.
                let program_node =
                    allocator.alloc(AstNode::new(program, AstNodes::Dummy(), allocator));
                let node =
                    AstNode::new(&stmt.expression, AstNodes::Program(program_node), allocator);
                let content = formatter::prelude::format_with(|f| {
                    write!(f, [node]);
                    if with_semicolon {
                        write!(f, [";"]);
                    }
                });
                finish.finish(
                    session,
                    options,
                    &content,
                    program.source_text,
                    source_type,
                    &program.comments,
                    embed_flags,
                )
            } else {
                let node = AstNode::new(program, AstNodes::Dummy(), allocator);
                finish.finish(
                    session,
                    options,
                    &node,
                    program.source_text,
                    source_type,
                    &program.comments,
                    embed_flags,
                )
            }
        }
    };

    Ok((formatted, expression_root))
}

/// See [`ExpressionRootKind`].
fn classify_expression_root(expression: &Expression) -> ExpressionRootKind {
    match expression {
        Expression::ObjectExpression(_) => ExpressionRootKind::ObjectExpression,
        Expression::ArrayExpression(_) => ExpressionRootKind::ArrayExpression,
        Expression::TemplateLiteral(_) => ExpressionRootKind::TemplateLiteral,
        Expression::StringLiteral(_) => ExpressionRootKind::StringLiteral,
        _ => ExpressionRootKind::Other,
    }
}

/// The Vue inline-handler semicolon rule for [`FragmentContext::EventHandlerStatements`].
///
/// Mirrors Prettier's `shouldPrintSemicolon` for `__vue_event_binding`
/// (`language-js/print/expression-statement.js`), which in turn mirrors the
/// Vue compiler's inline-handler detection: TS assertion wrappers are unwrapped
/// first, then function/arrow expressions, member expressions (including
/// optional chains ending in a member access), and identifiers other than
/// `undefined` keep their semicolon.
fn keeps_event_handler_semicolon(expression: &Expression) -> bool {
    match expression {
        Expression::TSAsExpression(e) => keeps_event_handler_semicolon(&e.expression),
        Expression::TSTypeAssertion(e) => keeps_event_handler_semicolon(&e.expression),
        Expression::TSNonNullExpression(e) => keeps_event_handler_semicolon(&e.expression),
        Expression::TSInstantiationExpression(e) => keeps_event_handler_semicolon(&e.expression),
        Expression::TSSatisfiesExpression(e) => keeps_event_handler_semicolon(&e.expression),
        Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => true,
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(_)
            | ChainElement::ComputedMemberExpression(_)
            | ChainElement::PrivateFieldExpression(_) => true,
            ChainElement::TSNonNullExpression(e) => keeps_event_handler_semicolon(&e.expression),
            ChainElement::CallExpression(_) => false,
        },
        Expression::Identifier(identifier) => identifier.name != "undefined",
        _ => false,
    }
}

/// Format an already-parsed program, special-purpose AST-in entry point.
///
/// Most callers want [`format()`] (text-in).
/// This skips parsing and is meant for cases that already hold a `Program`.
/// e.g. perf/allocation measurement that isolates formatting from parsing, or error-tolerant harnesses.
///
/// The `program` MUST be parsed via [`parse_for_format`] (formatter parse options + JSX enabling),
/// the formatter may panic on the wrong parse options.
/// No parse happens here, so there is no error to return.
pub fn format_program<'a>(
    allocator: &'a Allocator,
    program: &'a Program<'a>,
    options: JsFormatOptions,
) -> Formatted<'a, JsFormatContext<'a>> {
    format_program_with_session(
        &FormatSession::new(allocator, InputKind::PhysicalFile),
        program,
        options,
    )
}

/// Shared AST-in funnel for [`format_with_session`] / [`format_program`].
fn format_program_with_session<'a>(
    session: &FormatSession<'a>,
    program: &'a Program<'a>,
    options: JsFormatOptions,
) -> Formatted<'a, JsFormatContext<'a>> {
    let node = AstNode::new(program, AstNodes::Dummy(), session.allocator());
    format_node(
        session,
        options,
        &node,
        program.source_text,
        program.source_type,
        &program.comments,
        EmbedFlags::default(),
    )
}

/// Parse `source_text` the way the formatter requires, for AST-in callers of [`format_program`].
///
/// Applies the formatter's parse options and JSX enabling, exactly as [`format()`] does internally,
/// so a program fed to [`format_program`] matches the text-in path.
/// Use this when you need to control parse-error handling
/// or isolate parsing from formatting (perf/allocation measurement, error-tolerant harnesses).
/// Inspect the returned `ParserReturn` (`errors` / `panicked`) and pass `&ret.program` to [`format_program`].
pub fn parse_for_format<'a>(
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
) -> ParserReturn<'a> {
    // Always enable JSX for JavaScript source types (no syntax conflict)
    let source_type =
        if source_type.is_javascript() { source_type.with_jsx(true) } else { source_type };

    let options = ParseOptions {
        parse_regular_expression: false, // the formatter doesn't need regexes parsed
        allow_return_outside_function: true, // accept all syntax the formatter may be handed
        allow_v8_intrinsics: true,
        preserve_parens: false, // MUST be false: the formatter panics otherwise
        // The formatter does not use `Ident` hashes, but `detect_code_removal` runs semantic
        // analysis on this AST, and semantic requires hashed `Ident`s.
        enable_ident_hashes: cfg!(feature = "detect_code_removal"),
    };
    Parser::new(allocator, source_text, source_type).with_options(options).parse()
}

/// Parse `source_text` and promote the `Program` to the arena lifetime.
///
/// NOTE: Reject ANY parse diagnostic, not only `panicked`: we format valid code only, by design.
/// A recovered AST may be an unfaithful "fix" of the source
/// (e.g. invalid modifiers are reported but not all of them are representable),
/// so formatting it can silently rewrite what the user wrote.
/// Prettier instead formats invalid inputs through parser recovery, which also hides the error from the user;
fn parse<'a>(
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
) -> Result<&'a Program<'a>, OxcDiagnostic> {
    let ret = parse_for_format(allocator, source_text, source_type);
    if let Some(err) = ret.diagnostics.into_iter().next() {
        return Err(err);
    }
    Ok(allocator.alloc(ret.program))
}

/// Run the IR formatter over a single `Format` value and return its `Formatted` IR.
///
/// The only site that invokes the IR `formatter::format`.
/// Callers ([`format_program`] / [`format_fragment`]) construct the node (a whole-`Program` wrapper or a fragment),
/// and pass the surrounding `source_text` / `comments`.
fn format_node<'a, F: Format<'a, JsFormatContext<'a>>>(
    session: &FormatSession<'a>,
    options: JsFormatOptions,
    node: &F,
    source_text: &'a str,
    source_type: SourceType,
    comments: &'a [Comment],
    embed_flags: EmbedFlags,
) -> Formatted<'a, JsFormatContext<'a>> {
    let context = JsFormatContext::new(source_text, source_type, comments, options)
        .with_embedded_in_html_attribute(embed_flags.in_html_attribute)
        .with_embedded_vue_expression(embed_flags.vue_expression)
        .with_embedded_in_html_interpolation(embed_flags.in_html_interpolation);
    formatter::format(
        context,
        session,
        oxc_formatter_core::Arguments::new(&[oxc_formatter_core::Argument::new(node)]),
    )
}

// ---

#[derive(Copy, Clone, Debug)]
pub(crate) enum JsLabels {
    MemberChain,
    /// For `ir_transform/sort_imports`
    ImportDeclaration,
    /// For `ir_transform/sort_imports`
    /// Wraps a single emitted comment so the transform can identify it without
    /// inspecting element shape (which varies by comment kind / `jsdoc` formatting).
    /// Also suppresses internal line breaks for multi-line block comments.
    Comment,
}

impl Label for JsLabels {
    fn id(&self) -> u64 {
        *self as u64
    }

    fn debug_name(&self) -> &'static str {
        match self {
            Self::MemberChain => "MemberChain",
            Self::ImportDeclaration => "ImportDeclaration",
            Self::Comment => "Comment",
        }
    }
}
