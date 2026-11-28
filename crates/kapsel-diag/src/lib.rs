//! Error reporting for kapsel, rustc-style (a first-class goal).
//!
//! - [`render_finding`], [`render_parse_error`] and [`render_report`] print
//!   findings with `annotate-snippets` (the renderer rustc uses):
//!
//!   ```text
//!   error[KS-R001]: `|| true` throws away the exit status of `false`
//!    --> -c:1:7
//!     |
//!   1 | false || true
//!     | -----  ^^^^^^^ the failure is discarded here
//!     | |
//!     | this command's failure
//!     |
//!     = help: handle the failure explicitly: `false || echo "false failed" >&2`
//!   ```
//!
//! - [`render_json`] gives the same information as stable JSON
//!   (`schema: "kapsel.diag.v0"`) for editors and other tools.
//! - [`explain`] returns the long-form text for a rule id, like
//!   `rustc --explain`.
//!
//! Spans are byte ranges into the source. Out-of-range spans are clamped, and
//! spans that split a UTF-8 character are widened to whole characters, so a
//! bad span never panics the renderer.

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use kapsel_ast::{ParseError, Span};
use kapsel_plan::{CheckOutcome, Finding, Severity};
use serde::Serialize;

mod explain;
pub use explain::{RULE_IDS, explain};

/// The text a finding's spans index into, and the name shown after `-->`.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    /// `-c`, a script path, or `history` for session findings.
    pub name: &'a str,
    pub text: &'a str,
}

impl<'a> Source<'a> {
    pub fn new(name: &'a str, text: &'a str) -> Self {
        Self { name, text }
    }
}

/// Rendering options.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// ANSI colours, for a terminal.
    pub color: bool,
}

fn renderer(opts: Options) -> Renderer {
    if opts.color {
        Renderer::styled()
    } else {
        Renderer::plain()
    }
}

/// Clamps `span` into `text` and widens it to character boundaries.
fn clamp(text: &str, span: Span) -> std::ops::Range<usize> {
    let len = text.len();
    let mut start = (span.start as usize).min(len);
    let mut end = (span.end as usize).clamp(start, len);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    while !text.is_char_boundary(end) {
        end += 1;
    }
    start..end
}

/// The id shown in brackets: the rule of a finding, `KS-R000` for a parse
/// error.
pub const PARSE_ERROR_RULE: &str = "KS-R000";

fn level(sev: Severity) -> Level<'static> {
    match sev {
        Severity::Refuse => Level::ERROR,
        Severity::Warn => Level::WARNING,
    }
}

/// The primary underline's label. The title already says what is wrong, so
/// only a parse error's "expected ..." note is repeated at the span.
fn primary_label(f: &Finding) -> Option<String> {
    if f.rule != PARSE_ERROR_RULE {
        return None;
    }
    f.notes.iter().find_map(|n| {
        n.starts_with("expected ")
            .then(|| n.split(", found").next().unwrap_or(n).to_string())
    })
}

fn finding_group<'a>(src: Source<'a>, f: &'a Finding) -> Group<'a> {
    let mut snippet = Snippet::source(src.text).path(src.name).fold(true);
    let mut primary = AnnotationKind::Primary.span(clamp(src.text, f.span));
    if let Some(label) = primary_label(f) {
        primary = primary.label(label);
    }
    snippet = snippet.annotation(primary);
    for r in &f.related {
        snippet = snippet.annotation(
            AnnotationKind::Context
                .span(clamp(src.text, r.span))
                .label(r.label.as_str()),
        );
    }
    let mut group = level(f.severity)
        .primary_title(f.message.as_str())
        .id(f.rule.as_str())
        .element(snippet);
    for n in &f.notes {
        group = group.element(Level::NOTE.message(n.as_str()));
    }
    if let Some(h) = &f.help {
        group = group.element(Level::HELP.message(h.as_str()));
    }
    group
}

/// Renders one finding.
pub fn render_finding(src: Source<'_>, f: &Finding, opts: Options) -> String {
    renderer(opts).render(&[finding_group(src, f)])
}

/// Turns a parse error into the finding the check stage reports for it
/// (`KS-R000`), so both render the same way.
pub fn parse_error_finding(src: Source<'_>, e: &ParseError) -> Finding {
    let range = clamp(src.text, e.span);
    let mut notes = Vec::new();
    if !e.expected.is_empty() {
        let found = match src.text[range.clone()].chars().next() {
            None => "end of input".to_string(),
            Some(_) => format!("`{}`", &src.text[range.clone()]),
        };
        notes.push(format!("expected {}, found {found}", join_or(&e.expected)));
    }
    if e.construct != "syntax" {
        notes.push(format!("construct: {}", e.construct));
    }
    Finding {
        rule: PARSE_ERROR_RULE.into(),
        severity: Severity::Refuse,
        message: e.message.clone(),
        span: e.span,
        excerpt: src.text[range].to_string(),
        help: e.help.clone(),
        notes,
        related: Vec::new(),
    }
}

fn join_or(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// Renders one parse error as `error[KS-R000]`.
pub fn render_parse_error(src: Source<'_>, e: &ParseError, opts: Options) -> String {
    render_finding(src, &parse_error_finding(src, e), opts)
}

/// Renders findings in span order, then a summary line:
/// `error: refused: 2 errors, 1 warning` when any finding refuses, otherwise
/// `warning: 1 warning emitted`. Empty input renders as an empty string.
pub fn render_report(src: Source<'_>, findings: &[Finding], opts: Options) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by_key(|f| (f.span.start, f.span.end, f.rule.clone()));
    let r = renderer(opts);
    let mut out = String::new();
    for f in &sorted {
        out.push_str(&r.render(&[finding_group(src, f)]));
        out.push_str("\n\n");
    }
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Refuse)
        .count();
    let warnings = findings.len() - errors;
    let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    let summary = if errors > 0 {
        let mut s = format!("refused: {}", plural(errors, "error"));
        if warnings > 0 {
            s.push_str(&format!(", {}", plural(warnings, "warning")));
        }
        r.render(&[Group::with_title(Level::ERROR.primary_title(s))])
    } else {
        r.render(&[Group::with_title(
            Level::WARNING.primary_title(format!("{} emitted", plural(warnings, "warning"))),
        )])
    };
    out.push_str(&summary);
    out.push('\n');
    out
}

/// Renders a check outcome: the refusal's findings, or the plan's warnings.
pub fn render_outcome(src: Source<'_>, outcome: &CheckOutcome, opts: Options) -> String {
    match outcome {
        CheckOutcome::Refused(r) => render_report(src, &r.findings, opts),
        CheckOutcome::Plan(p) => render_report(src, &p.warnings, opts),
    }
}

/// Schema id of [`render_json`]'s output.
pub const DIAG_SCHEMA: &str = "kapsel.diag.v0";

/// 1-based line and column (column in characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// Line and column of byte `offset` (clamped) in `text`.
pub fn line_col(text: &str, offset: u32) -> LineCol {
    let r = clamp(text, Span::new(offset, offset));
    let prefix = &text[..r.start];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() as u32 + 1;
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    LineCol {
        line,
        col: text[line_start..r.start].chars().count() as u32 + 1,
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    schema: &'static str,
    file: &'a str,
    errors: usize,
    warnings: usize,
    diagnostics: Vec<JsonDiag<'a>>,
}

#[derive(Serialize)]
struct JsonDiag<'a> {
    rule: &'a str,
    /// `error` or `warning`, as rendered.
    level: &'static str,
    severity: Severity,
    message: &'a str,
    span: Span,
    start: LineCol,
    end: LineCol,
    excerpt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<&'a str>,
    notes: &'a [String],
    related: Vec<JsonRelated<'a>>,
}

#[derive(Serialize)]
struct JsonRelated<'a> {
    label: &'a str,
    span: Span,
    start: LineCol,
    end: LineCol,
}

/// Stable JSON for tools: one object with `schema`, `file`, counts and a
/// `diagnostics` array in span order. Field order is fixed.
pub fn render_json(src: Source<'_>, findings: &[Finding]) -> String {
    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by_key(|f| (f.span.start, f.span.end, f.rule.clone()));
    let diagnostics = sorted
        .iter()
        .map(|f| JsonDiag {
            rule: &f.rule,
            level: match f.severity {
                Severity::Refuse => "error",
                Severity::Warn => "warning",
            },
            severity: f.severity,
            message: &f.message,
            span: f.span,
            start: line_col(src.text, f.span.start),
            end: line_col(src.text, f.span.end),
            excerpt: &f.excerpt,
            help: f.help.as_deref(),
            notes: &f.notes,
            related: f
                .related
                .iter()
                .map(|r| JsonRelated {
                    label: &r.label,
                    span: r.span,
                    start: line_col(src.text, r.span.start),
                    end: line_col(src.text, r.span.end),
                })
                .collect(),
        })
        .collect();
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Refuse)
        .count();
    let report = JsonReport {
        schema: DIAG_SCHEMA,
        file: src.name,
        errors,
        warnings: findings.len() - errors,
        diagnostics,
    };
    serde_json::to_string_pretty(&report).expect("diagnostics serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_handles_out_of_range_and_utf8() {
        let t = "héllo";
        assert_eq!(clamp(t, Span::new(2, 3)), 1..3);
        assert_eq!(clamp(t, Span::new(9, 12)), 6..6);
        assert_eq!(clamp(t, Span::new(4, 2)), 4..4);
    }

    #[test]
    fn line_col_counts_chars() {
        let t = "héllo\nwörld";
        assert_eq!(line_col(t, 0), LineCol { line: 1, col: 1 });
        assert_eq!(line_col(t, 3), LineCol { line: 1, col: 3 });
        assert_eq!(line_col(t, 7), LineCol { line: 2, col: 1 });
        assert_eq!(line_col(t, 99), LineCol { line: 2, col: 6 });
    }
}
