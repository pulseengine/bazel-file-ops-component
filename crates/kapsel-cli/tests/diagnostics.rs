//! Diagnostics and analysis through the CLI (REQ-KS-060, REQ-KS-061):
//! `kapsel explain`, the KS-S rules in scripts and interactive sessions, and
//! `check --json` with its `diagnostics` field. Each test runs the real
//! binary with KAPSEL_HOME in a scratch directory.

use assert_cmd::Command;
use tempfile::TempDir;

struct Env {
    dir: TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("kapsel").unwrap();
        c.current_dir(self.dir.path())
            .env("KAPSEL_HOME", self.dir.path().join(".kapsel-home"));
        c
    }
    fn write(&self, rel: &str, s: &str) {
        std::fs::write(self.dir.path().join(rel), s).unwrap();
    }
}

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const S005_SCRIPT: &str =
    "mkdir -p results\necho a > results/a.txt\ncat results/a.txt\nrm -r reslts\n";

#[test]
fn explain_prints_the_long_form_text() {
    let e = Env::new();
    let o = e.cmd().args(["explain", "KS-R001"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o).trim_end(),
        kapsel_diag::explain("KS-R001").unwrap().trim_end()
    );
    assert!(out(&o).starts_with("KS-R001"), "{}", out(&o));
    // case-insensitive, as kapsel_diag::explain is
    assert_eq!(
        e.cmd()
            .args(["explain", "ks-s005"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn explain_of_an_unknown_id_exits_64_and_lists_the_known_ids() {
    let e = Env::new();
    let o = e.cmd().args(["explain", "KS-X999"]).output().unwrap();
    assert_eq!(o.status.code(), Some(64));
    assert_eq!(out(&o), "");
    let s = err(&o);
    assert!(s.contains("unknown rule id 'KS-X999'"), "{s}");
    for id in ["KS-R001", "KS-S001", "KS-S005"] {
        assert!(s.contains(id), "{id} missing from the list: {s}");
    }
    assert_eq!(
        e.cmd().args(["explain"]).output().unwrap().status.code(),
        Some(64)
    );
}

#[test]
fn interactive_retry_without_change_warns_ks_s001_across_lines() {
    let e = Env::new();
    // The first line fails (no such file); the second is the same command,
    // only the quoting differs, and nothing in between changed its input.
    let o = e
        .cmd()
        .write_stdin("cat in.txt\ncat  'in.txt'\n")
        .output()
        .unwrap();
    let s = err(&o);
    assert!(s.contains("warning[KS-S001]"), "{s}");
    assert!(s.contains("--> history:"), "{s}");
    assert!(s.contains("failed here with exit status 1"), "{s}");
    // a warning does not stop the line: it ran (and failed) again (the
    // built-in's own lines; each failure is also rendered as a run error)
    let own = s
        .lines()
        .filter(|l| !l.starts_with("error[") && l.contains("cat: in.txt: No such file"))
        .count();
    assert_eq!(own, 2, "{s}");
    assert_eq!(s.matches("error[run:not-found]").count(), 2, "{s}");
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn interactive_no_ks_s001_when_the_input_changed_in_between() {
    let e = Env::new();
    let o = e
        .cmd()
        .write_stdin("cat in.txt\necho hi > in.txt\ncat in.txt\n")
        .output()
        .unwrap();
    assert!(!err(&o).contains("KS-S001"), "{}", err(&o));
    assert_eq!(out(&o), "hi\n");
}

#[test]
fn script_ks_s005_refuses_and_nothing_runs() {
    let e = Env::new();
    e.write("clean.sh", S005_SCRIPT);
    let o = e.cmd().arg("clean.sh").output().unwrap();
    assert_eq!(o.status.code(), Some(65), "{}", err(&o));
    let s = err(&o);
    assert!(s.contains("error[KS-S005]"), "{s}");
    assert!(s.contains("--> clean.sh:4:"), "{s}");
    assert!(s.contains("did you mean `results`?"), "{s}");
    assert!(
        !e.dir.path().join("results").exists(),
        "a refused script must not run any part of itself"
    );
}

#[test]
fn interactive_ks_s005_refuses_the_line_and_the_session_goes_on() {
    let e = Env::new();
    let o = e
        .cmd()
        .write_stdin(format!("{S005_SCRIPT}echo after\n"))
        .output()
        .unwrap();
    assert!(err(&o).contains("error[KS-S005]"), "{}", err(&o));
    assert!(err(&o).contains("--> history:"), "{}", err(&o));
    assert_eq!(out(&o), "a\nafter\n", "{}", err(&o));
    assert!(
        e.dir.path().join("results").exists(),
        "the refused rm must not have run"
    );
}

#[test]
fn check_json_has_the_outcome_and_the_diagnostics() {
    let e = Env::new();
    let o = e
        .cmd()
        .args(["check", "--json", "-c", "false || true"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(65));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["outcome"], "refused");
    assert_eq!(v["findings"][0]["rule"], "KS-R001");
    assert_eq!(v["diagnostics"]["schema"], "kapsel.diag.v0");
    assert_eq!(v["diagnostics"]["file"], "-c");
    assert_eq!(v["diagnostics"]["errors"], 1);
    assert_eq!(v["diagnostics"]["diagnostics"][0]["rule"], "KS-R001");
    assert_eq!(v["diagnostics"]["diagnostics"][0]["start"]["col"], 7);
    // a plan carries the field too, with the plan's own fields untouched
    let o = e
        .cmd()
        .args(["check", "--json", "-c", "cat a | sort -u > b"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["outcome"], "plan");
    assert_eq!(v["diagnostics"]["errors"], 0);
    assert_eq!(
        v["diagnostics"]["diagnostics"][0]["rule"],
        v["warnings"][0]["rule"]
    );
    assert_eq!(v["diagnostics"]["diagnostics"][0]["level"], "warning");
    assert_eq!(v["grants"]["read"], serde_json::json!(["a"]));
}

#[test]
fn compile_still_seals_and_caches_by_plan_hash() {
    let e = Env::new();
    let o = e
        .cmd()
        .args(["compile", "-c", "echo hi", "-o", "a.wasm"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let o = e
        .cmd()
        .args(["compile", "-c", "echo   hi", "-o", "b.wasm"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    // same normalized plan: the second compile is served from the sealed cache
    assert_eq!(
        std::fs::read(e.dir.path().join("a.wasm")).unwrap(),
        std::fs::read(e.dir.path().join("b.wasm")).unwrap()
    );
}
