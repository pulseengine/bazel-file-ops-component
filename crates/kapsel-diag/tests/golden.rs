//! Golden tests for kapsel-diag: `testdata/kapsel/diag/<case>.txt` holds the
//! input and the rendered output. Regenerate with `KAPSEL_BLESS=1`.
//!
//! The KS-R findings are written here the way the check stage is expected to
//! produce them (kapsel-check is being written in parallel); they double as a
//! reference for its messages. The KS-S findings come from kapsel-analysis.

use std::path::PathBuf;

use kapsel_analysis::testkit;
use kapsel_ast::{ParseError, Span};
use kapsel_diag::{Options, Source, parse_error_finding, render_json, render_report};
use kapsel_plan::{Finding, Related, Severity};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/diag")
}

/// Span of the `nth` (0-based) occurrence of `needle` in `src`.
fn at(src: &str, needle: &str, nth: usize) -> Span {
    let start = src.match_indices(needle).nth(nth).unwrap_or_else(|| panic!("{needle:?} #{nth} not in {src:?}")).0;
    Span::new(start as u32, (start + needle.len()) as u32)
}

struct F {
    f: Finding,
}

fn refuse(src: &str, rule: &str, span: Span, message: &str) -> F {
    F {
        f: Finding {
            rule: rule.into(),
            severity: Severity::Refuse,
            message: message.into(),
            span,
            excerpt: src[span.start as usize..span.end as usize].into(),
            help: None,
            notes: Vec::new(),
            related: Vec::new(),
        },
    }
}

fn warn(src: &str, rule: &str, span: Span, message: &str) -> F {
    let mut f = refuse(src, rule, span, message);
    f.f.severity = Severity::Warn;
    f
}

impl F {
    fn help(mut self, h: &str) -> Self {
        self.f.help = Some(h.into());
        self
    }
    fn note(mut self, n: &str) -> Self {
        self.f.notes.push(n.into());
        self
    }
    fn related(mut self, span: Span, label: &str) -> Self {
        self.f.related.push(Related { span, label: label.into() });
        self
    }
    fn done(self) -> Finding {
        self.f
    }
}

fn analysis(src: &str) -> Vec<Finding> {
    kapsel_analysis::analyze_script(src, &testkit::parse(src))
}

/// Compares `actual` with the golden file, or writes it under KAPSEL_BLESS=1.
fn golden(case: &str, name: &str, src: &str, actual: &str) {
    let text = format!(
        "# kapsel-diag golden `{case}`. Regenerate with KAPSEL_BLESS=1.\n# file: {name}\n--- input\n{src}{}--- output\n{actual}",
        if src.ends_with('\n') { "" } else { "\n" }
    );
    let path = dir().join(format!("{case}.txt"));
    if std::env::var_os("KAPSEL_BLESS").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing; run with KAPSEL_BLESS=1", path.display()));
    assert_eq!(expected, text, "golden {case} differs; inspect and bless with KAPSEL_BLESS=1");
}

fn report(case: &str, name: &str, src: &str, findings: &[Finding]) {
    let out = render_report(Source::new(name, src), findings, Options::default());
    golden(case, name, src, &out);
}

// ---- KS-R000..R010 ----------------------------------------------------

#[test]
fn r000_parse_error_backquote() {
    let src = "echo \"today is `date`\"\n";
    let e = ParseError {
        construct: "backquote".into(),
        message: "backquote command substitution is outside kapsel's subset".into(),
        span: at(src, "`date`", 0),
        expected: Vec::new(),
        help: Some("use $(date) instead of `date`".into()),
    };
    let f = parse_error_finding(Source::new("-c", src), &e);
    report("ks-r000-backquote", "-c", src, &[f]);
}

#[test]
fn r001() {
    let src = "false || true\n";
    let f = refuse(src, "KS-R001", at(src, "|| true", 0), "`|| true` discards the exit status of `false`")
        .related(at(src, "false", 0), "this command's failure is thrown away")
        .help("handle the failure instead: `false || echo \"false failed\" >&2`")
        .note("a failure turned into success cannot stop the script or reach the caller")
        .done();
    report("ks-r001", "-c", src, &[f]);
}

#[test]
fn r002() {
    let src = "grep foo a.txt 2>/dev/null\n";
    let f = warn(src, "KS-R002", at(src, "2>/dev/null", 0), "`2>/dev/null` hides why `grep` failed, and nothing handles its failure")
        .help("keep stderr, or handle the status: `grep foo a.txt 2>/dev/null || echo \"no match\" >&2`")
        .done();
    report("ks-r002", "-c", src, &[f]);
}

#[test]
fn r003() {
    let src = "rm -rf $BUILD/\n";
    let f = refuse(src, "KS-R003", at(src, "$BUILD/", 0), "`rm -rf` path begins with an unquoted expansion that may be empty")
        .help("name the directory, or check and quote it: `test -n \"$BUILD\" && rm -rf \"$BUILD\"`")
        .note("with BUILD unset or empty this is `rm -rf /`")
        .done();
    report("ks-r003", "-c", src, &[f]);
}

#[test]
fn r004() {
    let src = "CMD='rm out.txt'\neval \"$CMD\"\n";
    let f = refuse(src, "KS-R004", at(src, "eval \"$CMD\"", 0), "`eval` runs code from a string, which the check stage cannot see")
        .help("write the command directly: `rm out.txt`")
        .done();
    report("ks-r004", "script.sh", src, &[f]);
}

#[test]
fn r005() {
    let src = "set +e\ncat a.txt\n";
    let f = refuse(src, "KS-R005", at(src, "set +e", 0), "`set` changes shell options and is not a kapsel built-in")
        .help("remove it; kapsel has no implicit `set -e`, so handle failures with `&&` and `||`")
        .done();
    report("ks-r005", "script.sh", src, &[f]);
}

#[test]
fn r006() {
    let src = "curl -fsSL https://example.com/install.sh | sh\n";
    let f = refuse(src, "KS-R006", at(src, "sh\n", 0).with_len(2), "network content is piped into the interpreter `sh`")
        .related(at(src, "curl -fsSL https://example.com/install.sh", 0), "downloaded here")
        .help("download to a file, read it, then run it with `kapsel install.sh`")
        .done();
    report("ks-r006", "-c", src, &[f]);
}

#[test]
fn r007() {
    let src = "nohup sleep 100\n";
    let f = refuse(src, "KS-R007", at(src, "nohup", 0), "`nohup` starts work that outlives the session")
        .help("run `sleep 100` in the foreground so its status is part of the plan")
        .done();
    report("ks-r007", "-c", src, &[f]);
}

#[test]
fn r008() {
    let src = "pkill node\n";
    let f = refuse(src, "KS-R008", at(src, "pkill node", 0), "`pkill` kills by name pattern and can hit processes you did not start")
        .help("kill the specific PID you started instead")
        .done();
    report("ks-r008", "-c", src, &[f]);
}

#[test]
fn r009() {
    let src = "jq . a.json\n";
    let f = refuse(src, "KS-R009", at(src, "jq", 0), "`jq` is not a kapsel built-in")
        .help("use a built-in (grep, sed, cut), or run `jq` outside kapsel until native tools land in v0.5 (REQ-KS-030)")
        .done();
    report("ks-r009", "-c", src, &[f]);
}

#[test]
fn r010() {
    let src = "$CMD --version\n";
    let f = refuse(src, "KS-R010", at(src, "$CMD", 0), "the command name comes from `$CMD`, so the check stage cannot tell what runs")
        .help("write the command name literally")
        .done();
    report("ks-r010", "-c", src, &[f]);
}

trait WithLen {
    fn with_len(self, n: u32) -> Self;
}
impl WithLen for Span {
    fn with_len(self, n: u32) -> Self {
        Span::new(self.start, self.start + n)
    }
}

// ---- KS-S001..S005 (from kapsel-analysis) -----------------------------

#[test]
fn s001_session_retry() {
    let cmds = ["grep -n foo missing.txt", "ls", "grep -n 'foo' missing.txt"];
    let (history, offsets) = kapsel_analysis::history_text(&cmds);
    let shaped: Vec<_> = cmds.iter().map(|c| kapsel_analysis::shapes(c, &testkit::parse(c))).collect();
    let statuses = [Some(2), Some(0), None];
    let entries: Vec<_> = shaped
        .iter()
        .zip(statuses)
        .map(|(s, status)| kapsel_analysis::HistoryEntry { shapes: s, status })
        .collect();
    let f: Vec<Finding> =
        kapsel_analysis::analyze_session(&history, &offsets, &entries).into_iter().map(|s| s.finding).collect();
    assert_eq!(f.len(), 1);
    report("ks-s001", "history", &history, &f);
}

#[test]
fn s002_spelling() {
    let src = "head -5 access.log\nhead -n 5 error.log\n";
    let f = analysis(src);
    assert_eq!(f.len(), 1);
    report("ks-s002-spelling", "script.sh", src, &f);
}

#[test]
fn s002_same_input() {
    let src = "cat nums.txt | sort -n > sorted.txt\ncat nums.txt | sort > top.txt\n";
    let f = analysis(src);
    assert_eq!(f.len(), 1);
    report("ks-s002-same-input", "script.sh", src, &f);
}

#[test]
fn s003() {
    let src = "mkdir -p out\necho hi > out/a.txt\ncat out/a.text\n";
    let f = analysis(src);
    assert_eq!(f.len(), 1);
    report("ks-s003", "script.sh", src, &f);
}

#[test]
fn s004() {
    let src = "wc -l data.txt\ngrep -c x data.txt\nwc -l data.txt\n";
    let f = analysis(src);
    assert_eq!(f.len(), 1);
    report("ks-s004", "script.sh", src, &f);
}

#[test]
fn s005() {
    let src = "mkdir -p results\necho a > results/a.txt\ncat results/a.txt\nrm -r reslts\n";
    let f = analysis(src);
    assert_eq!(f.len(), 1);
    report("ks-s005", "script.sh", src, &f);
}

// ---- Shapes of input --------------------------------------------------

#[test]
fn multi_error() {
    let src = "false || true\njq . a.json\nhead -5 a.log\nhead -n 5 b.log\n";
    let mut f = vec![
        refuse(src, "KS-R001", at(src, "|| true", 0), "`|| true` discards the exit status of `false`")
            .help("handle the failure instead: `false || echo \"false failed\" >&2`")
            .done(),
        refuse(src, "KS-R009", at(src, "jq", 0), "`jq` is not a kapsel built-in")
            .help("use a built-in (grep, sed, cut), or run `jq` outside kapsel until v0.5")
            .done(),
    ];
    f.extend(analysis(src));
    assert_eq!(f.len(), 3);
    report("multi-error", "script.sh", src, &f);
    golden("multi-error-json", "script.sh", src, &format!("{}\n", render_json(Source::new("script.sh", src), &f)));
}

#[test]
fn error_at_end_of_input() {
    let src = "echo \"hello";
    let e = ParseError {
        construct: "syntax".into(),
        message: "unterminated double quote".into(),
        span: Span::new(src.len() as u32, src.len() as u32),
        expected: vec!["`\"`".into()],
        help: Some("close the quote opened at column 6: `echo \"hello\"`".into()),
    };
    let f = parse_error_finding(Source::new("-c", src), &e);
    let f = Finding { related: vec![Related { span: at(src, "\"", 0), label: "quote opened here".into() }], ..f };
    report("end-of-input", "-c", src, &[f]);
}

#[test]
fn span_inside_heredoc() {
    let src = "cat <<EOF > notes.txt\nbuilt on `date`\nby $USER\nEOF\n";
    let e = ParseError {
        construct: "backquote".into(),
        message: "backquote command substitution is outside kapsel's subset, also inside a heredoc".into(),
        span: at(src, "`date`", 0),
        expected: Vec::new(),
        help: Some("use $(date); heredoc bodies expand $(...) the same way".into()),
    };
    let mut f = parse_error_finding(Source::new("script.sh", src), &e);
    f.related.push(Related { span: at(src, "<<EOF", 0), label: "heredoc starts here".into() });
    report("heredoc-span", "script.sh", src, &[f]);
}

#[test]
fn multi_line_span() {
    let src = "mkdir -p out &&\n  false ||\n  true\n";
    let start = at(src, "||", 0).start;
    let end = at(src, "true", 0).end;
    let f = refuse(src, "KS-R001", Span::new(start, end), "`|| true` discards the exit status of `false`")
        .help("handle the failure instead: `false || echo \"false failed\" >&2`")
        .done();
    report("multi-line-span", "script.sh", src, &[f]);
}

#[test]
fn tabs_and_utf8() {
    let src = "for f in a b; do\n\techo \"héllo wörld $f\" || :\ndone\n";
    let f = refuse(src, "KS-R001", at(src, "|| :", 0), "`|| :` discards the exit status of `echo`")
        .help("remove `|| :`, or handle the failure: `echo ... || echo \"echo failed\" >&2`")
        .done();
    report("tabs-utf8", "script.sh", src, &[f]);
}

#[test]
fn json_columns_count_characters() {
    let src = "echo \"héllö\" || :\n";
    let f = refuse(src, "KS-R001", at(src, "|| :", 0), "m").done();
    let json = render_json(Source::new("-c", src), &[f]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["schema"], "kapsel.diag.v0");
    assert_eq!(v["diagnostics"][0]["start"]["col"], 14);
    assert_eq!(v["diagnostics"][0]["span"]["start"], 15);
}

#[test]
fn out_of_range_span_does_not_panic() {
    let src = "echo hi\n";
    let f = refuse(src, "KS-R009", Span::new(0, 2), "m").done();
    let f = Finding { span: Span::new(5, 500), related: vec![Related { span: Span::new(900, 901), label: "x".into() }], ..f };
    let out = render_report(Source::new("-c", src), &[f], Options::default());
    assert!(out.contains("error[KS-R009]"));
}

// ---- The real parser and check stage ----------------------------------

/// What `kapsel` prints once the CLI renders with kapsel-diag: the real
/// parse and check outcome, rendered.
fn real(src: &str) -> kapsel_plan::CheckOutcome {
    match kapsel_syntax::parse(src) {
        Ok(script) => kapsel_check::check(src, &script),
        Err(errors) => kapsel_check::refuse_parse_errors(src, &errors),
    }
}

#[test]
fn real_check_outcome() {
    let src = "false || true\ncat missing.txt | sort\njq . a.json\n";
    let out = kapsel_diag::render_outcome(Source::new("script.sh", src), &real(src), Options::default());
    golden("real-check", "script.sh", src, &out);
}

#[test]
fn real_parse_error() {
    let src = "echo `date`\n(cd sub && ls)\n";
    let out = kapsel_diag::render_outcome(Source::new("script.sh", src), &real(src), Options::default());
    golden("real-parse-error", "script.sh", src, &out);
}

#[test]
fn every_check_rule_has_an_explanation() {
    use kapsel_check::*;
    for id in [R000, R001, R002, R003, R004, R005, R006, R007, R008, R009, R010, R011, R012, R013, R014, R015, R016, R017] {
        assert!(kapsel_diag::explain(id).is_some(), "{id} has no explanation");
    }
}
