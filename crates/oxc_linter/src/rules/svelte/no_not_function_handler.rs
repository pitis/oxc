use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType, Span};
use svelte_markup_parser::ast::{AttributeKind, DirectiveKind, ExpressionTag, Node, ValuePart};

use crate::{
    rule::Rule,
    svelte_template::{SvelteTemplateContext, SvelteTemplateRule},
    utils::walk_svelte_elements,
};

fn no_not_function_handler_diagnostic(phrase: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Unexpected {phrase} in event handler."))
        .with_help(
            "Event handlers must be functions; pass a function reference or an arrow function like `on:click={() => …}`.",
        )
        .with_label(span)
}

/// The base event names of eslint-plugin-svelte's `EVENT_NAMES` list (from
/// `src/utils/events.ts`, itself derived from Svelte's `elements.d.ts`).
/// Each occurs upstream as `on:<name>` and `on<name>`, and — except for the
/// four in [`EVENTS_WITHOUT_CAPTURE_ATTRIBUTE`] — as `on<name>capture`.
/// Sorted for binary search.
const EVENT_NAMES: &[&str] = &[
    "abort",
    "animationend",
    "animationiteration",
    "animationstart",
    "auxclick",
    "beforeinput",
    "beforematch",
    "beforetoggle",
    "blur",
    "cancel",
    "canplay",
    "canplaythrough",
    "change",
    "click",
    "close",
    "compositionend",
    "compositionstart",
    "compositionupdate",
    "contentvisibilityautostatechange",
    "contextmenu",
    "copy",
    "cuechange",
    "cut",
    "dblclick",
    "drag",
    "dragend",
    "dragenter",
    "dragexit",
    "dragleave",
    "dragover",
    "dragstart",
    "drop",
    "durationchange",
    "emptied",
    "encrypted",
    "ended",
    "error",
    "focus",
    "focusin",
    "focusout",
    "formdata",
    "fullscreenchange",
    "fullscreenerror",
    "gamepadconnected",
    "gamepaddisconnected",
    "gotpointercapture",
    "input",
    "introend",
    "introstart",
    "invalid",
    "keydown",
    "keypress",
    "keyup",
    "load",
    "loadeddata",
    "loadedmetadata",
    "loadstart",
    "lostpointercapture",
    "message",
    "messageerror",
    "mousedown",
    "mouseenter",
    "mouseleave",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "outroend",
    "outrostart",
    "paste",
    "pause",
    "play",
    "playing",
    "pointercancel",
    "pointerdown",
    "pointerenter",
    "pointerleave",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerup",
    "progress",
    "ratechange",
    "reset",
    "resize",
    "scroll",
    "scrollend",
    "seeked",
    "seeking",
    "select",
    "selectionchange",
    "selectstart",
    "stalled",
    "submit",
    "suspend",
    "timeupdate",
    "toggle",
    "touchcancel",
    "touchend",
    "touchmove",
    "touchstart",
    "transitioncancel",
    "transitionend",
    "transitionrun",
    "transitionstart",
    "visibilitychange",
    "volumechange",
    "waiting",
    "wheel",
];

/// Event names upstream's list has no `on<name>capture` attribute for.
const EVENTS_WITHOUT_CAPTURE_ATTRIBUTE: &[&str] =
    &["gamepadconnected", "gamepaddisconnected", "mouseenter", "mouseleave"];

#[derive(Debug, Default, Clone)]
pub struct NoNotFunctionHandler;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows values that are definitely not functions — object, array,
    /// class, and literal expressions — in event handlers, both `on:`
    /// directives and Svelte 5 `on*` event attributes.
    ///
    /// ### Why is this bad?
    ///
    /// An event handler is called when the event fires; a value that is not
    /// a function cannot be called. Writing `on:click={handler()}` style
    /// mistakes aside, a literal like `on:click={{ handler }}` (an object)
    /// or `on:click={'handler'}` (a string) silently does nothing useful
    /// and is almost always a mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```svelte
    /// <button on:click={{ handler }} />
    /// <button on:click={'handler'} />
    /// <button onclick={[handler]} />
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```svelte
    /// <button on:click={handler} />
    /// <button on:click={() => handler()} />
    /// <button on:click />
    /// ```
    NoNotFunctionHandler,
    svelte,
    correctness,
    version = "1.80.0",
    short_description = "Disallow use of not function in event handler.",
);

impl Rule for NoNotFunctionHandler {}

impl SvelteTemplateRule for NoNotFunctionHandler {
    fn run_on_markup<'a>(&self, nodes: &[Node<'a>], ctx: &mut SvelteTemplateContext<'a>) {
        let mut reports: Vec<(&'static str, Span)> = Vec::new();
        walk_svelte_elements(nodes, &mut |element| {
            for attribute in &element.attributes {
                match &attribute.kind {
                    // `on:event={handler}` — every `on:` directive is
                    // checked, whatever the event name (upstream's
                    // `SvelteDirective` handler doesn't consult the event
                    // list). A bare `on:event` (event forwarding) has no
                    // value and is fine.
                    AttributeKind::Directive(directive) if directive.kind == DirectiveKind::On => {
                        if let Some(tag) =
                            directive.value.as_ref().and_then(|value| value.as_single_expression())
                        {
                            check_handler_expression(tag, &mut reports);
                        }
                    }
                    // Svelte 5 `onclick={handler}` — plain attributes whose
                    // name is one of the known event-handler attributes.
                    AttributeKind::Plain { name, value: Some(value), .. }
                        if is_event_attribute(name) =>
                    {
                        for part in &value.parts {
                            if let ValuePart::Expression(tag) = part {
                                check_handler_expression(tag, &mut reports);
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        for (phrase, span) in reports {
            ctx.diagnostic(no_not_function_handler_diagnostic(phrase, span));
        }
    }
}

/// Whether `name` is one of upstream `EVENT_NAMES`' attribute forms:
/// `on<event>` or `on<event>capture`.
fn is_event_attribute(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("on") else { return false };
    // Direct match first: `gotpointercapture` / `lostpointercapture` are
    // themselves event names ending in "capture".
    if EVENT_NAMES.binary_search(&rest).is_ok() {
        return true;
    }
    rest.strip_suffix("capture").is_some_and(|base| {
        EVENT_NAMES.binary_search(&base).is_ok()
            && !EVENTS_WITHOUT_CAPTURE_ATTRIBUTE.contains(&base)
    })
}

/// Check one `{expr}` handler value, reporting on the expression's own span
/// (upstream reports on the expression node, not the whole directive).
fn check_handler_expression(tag: &ExpressionTag<'_>, reports: &mut Vec<(&'static str, Span)>) {
    let (text, trimmed_span) = tag.trimmed();
    if let Some((phrase, span)) = not_function_phrase(text) {
        reports.push((
            phrase,
            Span::new(trimmed_span.start + span.start, trimmed_span.start + span.end),
        ));
    }
}

/// Parse the handler expression and map its top-level node to upstream's
/// `PHRASES` table when it is one of the definitely-not-a-function kinds;
/// the returned span is relative to `text`.
///
/// Upstream also resolves identifiers through `const` initializers in the
/// `<script>` block (`findRootExpression`); this markup-only pass has no
/// script analysis, so identifiers are never reported — a deliberate
/// narrowing that only produces false negatives, never false positives.
fn not_function_phrase(text: &str) -> Option<(&'static str, Span)> {
    let allocator = Allocator::new();
    let expression = Parser::new(&allocator, text, SourceType::ts())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse_expression()
        .ok()?;
    let phrase = match &expression {
        Expression::ObjectExpression(_) => "object",
        Expression::ArrayExpression(_) => "array",
        Expression::ClassExpression(_) => "class",
        // Upstream's `Literal` phrase function: regex and bigint literals
        // get their own phrases, `null` is deliberately not reported
        // (Svelte accepts a nullish handler), and other primitives report
        // as `` `${typeof value}` value ``. Template literals count as
        // string values.
        Expression::RegExpLiteral(_) => "regex value",
        Expression::BigIntLiteral(_) => "bigint value",
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => "string value",
        Expression::NumericLiteral(_) => "number value",
        Expression::BooleanLiteral(_) => "boolean value",
        _ => return None,
    };
    Some((phrase, expression.span()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::NoNotFunctionHandler;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            ("<button on:click={fn} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={() => a} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Event forwarding has no value.
            ("<button on:click />", None, None, Some(PathBuf::from("test.svelte"))),
            // A nullish handler is accepted (upstream skips `null`).
            ("<button on:click={null} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={a ?? b} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Other directive kinds are not event handlers.
            ("<input bind:value={''} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Svelte 5 event attribute with a function reference.
            ("<button onclick={fn} />", None, None, Some(PathBuf::from("test.svelte"))),
            // A plain attribute that isn't a known event name is a prop.
            ("<Widget once={'nope'} />", None, None, Some(PathBuf::from("test.svelte"))),
        ];

        let fail = vec![
            ("<button on:click={{ fn }} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={[a]} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={class B {}} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={'str'} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={`str`} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={100} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={true} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={42n} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button on:click={/reg/} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Modifiers don't change the check.
            ("<button on:click|once={{ a }} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Svelte 5 event attributes, including the capture variants.
            ("<button onclick={{ fn }} />", None, None, Some(PathBuf::from("test.svelte"))),
            ("<button onclickcapture={'s'} />", None, None, Some(PathBuf::from("test.svelte"))),
            // Nested inside blocks.
            (
                "{#if x}<button on:click={{ a }} />{/if}",
                None,
                None,
                Some(PathBuf::from("test.svelte")),
            ),
        ];

        Tester::new(NoNotFunctionHandler::NAME, NoNotFunctionHandler::PLUGIN, pass, fail)
            .test_and_snapshot();
    }
}
