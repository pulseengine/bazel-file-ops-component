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
pub use explain::{RULE_IDS, RUN_ERROR_IDS, explain};

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

/// How a control character is shown: `\x1b` for C0 controls and DEL,
/// `\u{9b}` for C1 controls. Newline and tab stay (they lay out the
/// excerpt); everything else that a terminal would act on is made visible,
/// so a file name or script holding ESC cannot drive the terminal.
fn escape_char(c: char, out: &mut String) {
    match c {
        '\n' | '\t' => out.push(c),
        c if (c as u32) < 0x20 || c as u32 == 0x7f => {
            out.push_str(&format!("\\x{:02x}", c as u32));
        }
        c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
        c => out.push(c),
    }
}

/// `s` with its control characters escaped (see [`escape_char`]).
pub fn escape_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        escape_char(c, &mut out);
    }
    out
}

/// A source with its control characters escaped, and where each original
/// byte offset lands in the escaped text (for the spans).
struct Escaped {
    name: String,
    text: String,
    map: Vec<u32>,
}

impl Escaped {
    fn new(src: Source<'_>) -> Self {
        let mut text = String::with_capacity(src.text.len());
        let mut map = vec![0u32; src.text.len() + 1];
        for (i, c) in src.text.char_indices() {
            for k in 0..c.len_utf8() {
                map[i + k] = text.len() as u32;
            }
            escape_char(c, &mut text);
        }
        map[src.text.len()] = text.len() as u32;
        Self {
            name: escape_controls(src.name),
            text,
            map,
        }
    }

    fn source(&self) -> Source<'_> {
        Source::new(&self.name, &self.text)
    }

    fn span(&self, s: Span) -> Span {
        let at = |o: u32| self.map[(o as usize).min(self.map.len() - 1)];
        let start = at(s.start);
        Span::new(start, at(s.end).max(start))
    }

    /// The finding as it is shown: spans moved into the escaped text, and
    /// every text field escaped.
    fn finding(&self, f: &Finding) -> Finding {
        let mut f = f.clone();
        f.span = self.span(f.span);
        f.message = escape_controls(&f.message);
        f.excerpt = escape_controls(&f.excerpt);
        f.notes = f.notes.iter().map(|n| escape_controls(n)).collect();
        f.help = f.help.as_deref().map(escape_controls);
        for r in &mut f.related {
            r.span = self.span(r.span);
            r.label = escape_controls(&r.label);
        }
        f
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
    let es = Escaped::new(src);
    let f = es.finding(f);
    renderer(opts).render(&[finding_group(es.source(), &f)])
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
    let es = Escaped::new(src);
    let shown: Vec<Finding> = sorted.iter().map(|f| es.finding(f)).collect();
    let r = renderer(opts);
    let mut out = String::new();
    for f in &shown {
        out.push_str(&r.render(&[finding_group(es.source(), f)]));
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

/// A run error (REQ-KS-060): a built-in or a grant check that failed while
/// the plan ran, located at the command that ran it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDiag {
    /// One of [`RUN_ERROR_IDS`], e.g. `run:grants`.
    pub id: String,
    /// The line the built-in printed, e.g. `cat: a.txt: No such file or directory`.
    pub message: String,
    /// The failing command in the source, when the site is known.
    pub span: Option<Span>,
    pub help: String,
}

/// Renders one run error, rustc-style: `error[run:…]: message`, the source
/// line with the failing command underlined, then `= help: …`. Without a
/// span (an unknown site) it renders the title and the help only.
pub fn render_run_error(src: Source<'_>, d: &RunDiag, opts: Options) -> String {
    let r = renderer(opts);
    let es = Escaped::new(src);
    let src = es.source();
    let (message, help) = (escape_controls(&d.message), escape_controls(&d.help));
    let mut group = Group::with_title(
        Level::ERROR
            .primary_title(message.as_str())
            .id(d.id.as_str()),
    );
    if let Some(span) = d.span {
        group = group.element(
            Snippet::source(src.text)
                .path(src.name)
                .fold(true)
                .annotation(AnnotationKind::Primary.span(clamp(src.text, es.span(span)))),
        );
    }
    group = group.element(Level::HELP.message(help.as_str()));
    r.render(&[group])
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

    /// Second review, non-blocking 3: ESC in a file name, a run error's
    /// message and a parse error's excerpt is shown as `\x1b`, never raw.
    #[test]
    fn control_characters_are_escaped_in_run_errors() {
        let text = "cat \"$(printf '\\033[2J')\"\n";
        let d = RunDiag {
            id: "run:not-found".into(),
            message: "cat: \u{1b}[2J: No such file or directory".into(),
            span: Some(Span::new(0, 25)),
            help: "check the name \u{9b}31m".into(),
        };
        let out = render_run_error(
            Source::new("evil\u{1b}]0;x\u{7}.sh", text),
            &d,
            Options::default(),
        );
        assert!(
            !out.contains('\u{1b}') && !out.contains('\u{7}') && !out.contains('\u{9b}'),
            "{out:?}"
        );
        assert!(out.contains("cat: \\x1b[2J: No such file"), "{out}");
        assert!(out.contains("evil\\x1b]0;x\\x07.sh"), "{out}");
        assert!(out.contains("\\u{9b}31m"), "{out}");
    }

    #[test]
    fn control_characters_are_escaped_in_parse_error_excerpts() {
        // A parse error whose span covers an ESC in the source.
        let text = "echo \u{1b}[31m red (x)\n";
        let start = text.find('(').unwrap() as u32;
        let e = ParseError {
            construct: "subshell".into(),
            message: "subshells are refused near \u{1b}[31m".into(),
            span: Span::new(5, start + 3),
            expected: vec!["a word".into()],
            help: None,
        };
        let out = render_parse_error(Source::new("a\u{1b}b.sh", text), &e, Options::default());
        assert!(!out.contains('\u{1b}'), "{out:?}");
        assert!(out.contains("echo \\x1b[31m red (x)"), "{out}");
        assert!(out.contains("found `\\x1b[31m red (x)`"), "{out}");
        assert!(out.contains("a\\x1bb.sh"), "{out}");
        // The underline still starts under the escaped character.
        let f = parse_error_finding(Source::new("s", text), &e);
        let es = Escaped::new(Source::new("s", text));
        assert_eq!(es.span(f.span).start, 5);
        assert_eq!(es.finding(&f).excerpt, "\\x1b[31m red (x)");
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
