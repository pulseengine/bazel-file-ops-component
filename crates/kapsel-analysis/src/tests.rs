//! Planted case and control for each KS-S rule.

use kapsel_plan::{Finding, Severity};

use crate::rules::*;
use crate::testkit::parse;
use crate::{analyze_script, shapes};

fn run(src: &str) -> Vec<Finding> {
    analyze_script(src, &parse(src))
}

fn rule(src: &str, id: &str) -> Vec<Finding> {
    run(src).into_iter().filter(|f| f.rule == id).collect()
}

fn text(src: &str, span: kapsel_plan::Span) -> &str {
    &src[span.start as usize..span.end as usize]
}

fn session(cmds: &[(&str, Option<i32>)]) -> (String, Vec<SessionFinding>) {
    let texts: Vec<&str> = cmds.iter().map(|c| c.0).collect();
    let (history, offsets) = history_text(&texts);
    let shaped: Vec<_> = cmds.iter().map(|(t, _)| shapes(t, &parse(t))).collect();
    let entries: Vec<HistoryEntry> =
        shaped.iter().zip(cmds).map(|(s, (_, st))| HistoryEntry { shapes: s, status: *st }).collect();
    let f = analyze_session(&history, &offsets, &entries);
    (history, f)
}

// ---- KS-S001 retry without change -------------------------------------

#[test]
fn s001_planted_identical_retry() {
    let (h, f) = session(&[("grep -n foo missing.txt", Some(2)), ("grep -n foo missing.txt", None)]);
    let f: Vec<_> = f.into_iter().filter(|f| f.finding.rule == KS_S001).collect();
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(f[0].command, 1);
    assert_eq!(f[0].finding.severity, Severity::Warn);
    assert_eq!(text(&h, f[0].finding.span), "grep -n foo missing.txt");
    assert_eq!(f[0].finding.related[0].span.start, 0);
    assert_eq!(f[0].finding.help.as_deref(), Some("it failed the same way before; change the input or read the error"));
}

#[test]
fn s001_planted_quoting_whitespace_flag_order() {
    let (_, f) = session(&[("grep -n -i foo missing.txt", Some(2)), ("grep  -i -n 'foo' \"missing.txt\"", None)]);
    let f: Vec<_> = f.into_iter().filter(|f| f.finding.rule == KS_S001).collect();
    assert_eq!(f.len(), 1);
    assert!(f[0].finding.notes[0].contains("only in quoting"));
}

#[test]
fn s001_planted_unrelated_command_in_between() {
    let (_, f) = session(&[("cat in.txt", Some(1)), ("echo hi > other.txt", Some(0)), ("cat in.txt", None)]);
    assert_eq!(f.iter().filter(|f| f.finding.rule == KS_S001).count(), 1);
}

#[test]
fn s001_control_input_changed_in_between() {
    let (_, f) = session(&[("cat in.txt", Some(1)), ("echo hi > in.txt", Some(0)), ("cat in.txt", None)]);
    assert!(f.iter().all(|f| f.finding.rule != KS_S001), "{f:#?}");
    let (_, f) = session(&[("cat out/a.txt", Some(1)), ("mkdir -p out", Some(0)), ("cat out/a.txt", None)]);
    assert!(f.iter().all(|f| f.finding.rule != KS_S001), "{f:#?}");
    let (_, f) = session(&[("cat a", Some(1)), ("cd sub", Some(0)), ("cat a", None)]);
    assert!(f.iter().all(|f| f.finding.rule != KS_S001));
}

#[test]
fn s001_control_success_or_different_command() {
    let (_, f) = session(&[("cat in.txt", Some(0)), ("cat in.txt", None)]);
    assert!(f.iter().all(|f| f.finding.rule != KS_S001));
    let (_, f) = session(&[("cat in.txt", Some(1)), ("cat in2.txt", None)]);
    assert!(f.iter().all(|f| f.finding.rule != KS_S001));
}

#[test]
fn s001_refused_command_in_between_does_not_count() {
    // The middle command was refused, so it never wrote in.txt.
    let (_, f) = session(&[("cat in.txt", Some(1)), ("echo hi > in.txt", None), ("cat in.txt", None)]);
    assert_eq!(f.iter().filter(|f| f.finding.rule == KS_S001).count(), 1);
}

// ---- KS-S002 inconsistent flags ---------------------------------------

#[test]
fn s002_planted_head_spelling() {
    let src = "head -5 a.txt\nhead -n 5 b.txt\n";
    let f = rule(src, KS_S002);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(text(src, f[0].span), "-n 5");
    assert_eq!(text(src, f[0].related[0].span), "-5");
    assert!(f[0].help.as_deref().unwrap().contains("POSIX"));
}

#[test]
fn s002_planted_grep_alias() {
    let src = "grep -r foo src\ngrep -R bar src\n";
    let f = rule(src, KS_S002);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(text(src, f[0].span), "-R");
}

#[test]
fn s002_planted_sort_same_input() {
    let src = "cat nums | sort -n > a\ncat nums | sort > b\n";
    let f = rule(src, KS_S002);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(text(src, f[0].related[0].span), "sort -n > a");
    assert!(f[0].message.contains("no flags"));
}

#[test]
fn s002_control() {
    // Same spelling everywhere; different inputs with different flags.
    assert!(rule("head -n 5 a\nhead -n 3 b\n", KS_S002).is_empty());
    assert!(rule("cat nums | sort -n\nls | sort\n", KS_S002).is_empty());
    assert!(rule("grep -r foo src\ngrep -r bar src\n", KS_S002).is_empty());
    // Flag clusters are a style choice, not a different spelling.
    assert!(rule("grep -rn foo src\ngrep -r -n bar src\n", KS_S002).is_empty());
}

// ---- KS-S003 near-miss path -------------------------------------------

#[test]
fn s003_planted() {
    let src = "mkdir -p out\necho hi > out/a.txt\ncat out/a.text\n";
    let f = rule(src, KS_S003);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(text(src, f[0].span), "out/a.text");
    assert_eq!(text(src, f[0].related[0].span), "out/a.txt");
    assert_eq!(f[0].help.as_deref(), Some("did you mean out/a.txt?"));
}

#[test]
fn s003_control() {
    // Correct path; a numbered series; a whole-name swap.
    assert!(rule("echo hi > out/a.txt\ncat out/a.txt\n", KS_S003).is_empty());
    assert!(rule("echo hi > log1\ncat log2\n", KS_S003).is_empty());
    assert!(rule("echo hi > out/a.txt\ncat out/b.txt\n", KS_S003).is_empty());
    // The near-miss was written *after* the read: not this rule.
    assert!(rule("cat out/a.text\necho hi > out/a.txt\n", KS_S003).is_empty());
}

#[test]
fn s003_across_session() {
    let (h, f) = session(&[("echo hi > notes.txt", Some(0)), ("cat notse.txt", None)]);
    let f: Vec<_> = f.into_iter().filter(|f| f.finding.rule == KS_S003).collect();
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(f[0].command, 1);
    assert_eq!(text(&h, f[0].finding.related[0].span), "notes.txt");
}

// ---- KS-S004 duplicated command ---------------------------------------

#[test]
fn s004_planted() {
    let src = "wc -l data.txt\ngrep x data.txt\nwc -l data.txt\n";
    let f = rule(src, KS_S004);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(f[0].severity, Severity::Warn);
    assert_eq!(f[0].span.start, 31);
    assert!(f[0].message.contains("line 1"));
}

#[test]
fn s004_control() {
    // State change between; the command writes; output-only commands.
    assert!(rule("wc -l data.txt\necho x >> data.txt\nwc -l data.txt\n", KS_S004).is_empty());
    assert!(rule("echo x >> log\necho x >> log\n", KS_S004).is_empty());
    assert!(rule("echo ---\necho ---\n", KS_S004).is_empty());
    assert!(rule("wc -l a\nwc -l b\n", KS_S004).is_empty());
}

// ---- KS-S005 destructive near-miss ------------------------------------

#[test]
fn s005_planted() {
    let src = "mkdir -p results\necho a > results/a.txt\ncat results/a.txt\nrm -r reslts\n";
    let f = rule(src, KS_S005);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(f[0].severity, Severity::Refuse);
    assert_eq!(text(src, f[0].span), "reslts");
    assert_eq!(text(src, f[0].related[0].span), "results");
    assert!(f[0].help.as_deref().unwrap().starts_with("did you mean `results`?"));
}

#[test]
fn s005_control() {
    // The real path; a target named elsewhere too; a path used once only.
    assert!(rule("mkdir -p results\necho a > results/a.txt\nrm -r results\n", KS_S005).is_empty());
    assert!(rule("mkdir -p results\ncat results/a\ntest -e reslts && rm -r reslts\n", KS_S005).is_empty());
    assert!(rule("echo a > results.txt\nrm reslts.txt\n", KS_S005).is_empty());
}

#[test]
fn findings_are_deterministic() {
    let src = "head -5 a.txt\nhead -n 5 b.txt\necho hi > out/a.txt\ncat out/a.text\nmkdir -p results\ncat results/x\nrm -r reslts\n";
    let a = run(src);
    assert_eq!(a, run(src));
    let ids: Vec<&str> = a.iter().map(|f| f.rule.as_str()).collect();
    assert_eq!(ids, [KS_S002, KS_S003, KS_S005]);
}
