use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, rule::Rule};

fn no_octal_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Octal literals should not be used.")
        .with_help("Write the number with the `0o` prefix, or in decimal.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOctal;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows legacy octal literals — a leading `0` followed by digits,
    /// as in `071`.
    ///
    /// ### Why is this bad?
    ///
    /// The leading zero is easy to read as decorative padding, but it changes
    /// the value: `071` is 57, not 71. The form is a syntax error in strict
    /// mode and in modules, so it only survives in sloppy scripts, where it is
    /// almost always a mistake rather than a choice. ES6's explicit `0o71`
    /// says the same thing unambiguously.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// var num = 071;
    /// var result = someObject.get(071);
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// var num = 0o71;
    /// var num = 71;
    /// var num = 0.71;
    /// var num = 0;
    /// ```
    NoOctal,
    eslint,
    correctness,
    version = "1.80.0",
    short_description = "Disallow octal literals.",
);

impl Rule for NoOctal {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::NumericLiteral(literal) = node.kind() else { return };
        // Upstream's test is `/^0[0-9]/` on the raw text, which admits `08`
        // and `09` — legal decimals that ESLint still reports, because the
        // leading zero misleads in exactly the same way.
        let Some(raw) = literal.raw else { return };
        let mut bytes = raw.as_str().bytes();
        if bytes.next() == Some(b'0') && bytes.next().is_some_and(|byte| byte.is_ascii_digit()) {
            ctx.diagnostic(no_octal_diagnostic(literal.span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "var a = 'hello world';",
        "0x1234",
        "0X5;",
        "a = 0;",
        "0.1",
        "0.5e0",
        "0.5e-100",
        ".5e0",
        "0b111",
        "0o71",
        "0.000000000000000001",
        // A separator cannot follow the leading zero.
        "0",
        "-0",
    ];

    let fail =
        vec!["var a = 01234;", "a = 1 + 01234;", "00", "08", "09.1", "09e1", "09.1e1", "018"];

    Tester::new(NoOctal::NAME, NoOctal::PLUGIN, pass, fail).test_and_snapshot();
}
