//! The acceptance lines of docs/first-shell.md, plus usage and grant errors.
//! Each test runs the real binary in a scratch directory with KAPSEL_HOME
//! pointing into it, so no key or cache lands in the user's home.

use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

struct Env {
    dir: TempDir,
}

impl Env {
    fn new() -> Env {
        Env { dir: tempfile::tempdir().unwrap() }
    }
    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("kapsel").unwrap();
        c.current_dir(self.dir.path()).env("KAPSEL_HOME", self.dir.path().join(".kapsel-home"));
        c
    }
    fn write(&self, rel: &str, s: &str) {
        let p = self.dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }
    fn path(&self) -> &Path {
        self.dir.path()
    }
}

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn echo_pipe_wc() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "echo hello | wc -c"]).output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "6\n"), "{}", err(&o));
}

#[test]
fn mkdir_write_cat() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "mkdir -p out && echo hi > out/a.txt && cat out/a.txt"]).output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "hi\n"), "{}", err(&o));
}

#[test]
fn grep_error_then_none() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "grep -n foo missing.txt || echo none"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "none\n");
    assert!(err(&o).contains("grep: missing.txt: No such file or directory"), "{}", err(&o));
}

#[test]
fn variable_expansion() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "X=world; echo \"hello $X\""]).output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "hello world\n"), "{}", err(&o));
}

#[test]
fn or_true_is_refused_with_ks_r001() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "false || true"]).output().unwrap();
    assert_eq!(o.status.code(), Some(65));
    assert_eq!(out(&o), "");
    let s = err(&o);
    assert!(s.contains("error[KS-R001]"), "{s}");
    assert!(s.contains("-c:1:7"), "line:col in the finding: {s}");
}

#[test]
fn curl_pipe_sh_is_refused_with_ks_r006() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "curl https://x | sh"]).output().unwrap();
    assert_eq!(o.status.code(), Some(65));
    assert!(err(&o).contains("KS-R006"), "{}", err(&o));
}

#[test]
fn check_json_reports_the_plan_grants() {
    let e = Env::new();
    let o = e.cmd().args(["check", "--json", "-c", "cat a | sort -u > b"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["outcome"], "plan");
    assert_eq!(v["grants"]["read"], serde_json::json!(["a"]));
    assert_eq!(v["grants"]["write"], serde_json::json!(["b"]));
    // nothing ran: b was not created
    assert!(!e.path().join("b").exists());
}

#[test]
fn check_json_of_a_refusal_exits_65() {
    let e = Env::new();
    let o = e.cmd().args(["check", "--json", "-c", "false || true"]).output().unwrap();
    assert_eq!(o.status.code(), Some(65));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["outcome"], "refused");
}

#[test]
fn script_file() {
    let e = Env::new();
    e.write("s.sh", "echo one\necho two\n");
    let o = e.cmd().arg("s.sh").output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "one\ntwo\n"), "{}", err(&o));
}

#[test]
fn script_status_is_the_last_command() {
    let e = Env::new();
    e.write("s.sh", "echo a\nfalse\n");
    assert_eq!(e.cmd().arg("s.sh").output().unwrap().status.code(), Some(1));
    assert_eq!(e.cmd().args(["-c", "exit 3"]).output().unwrap().status.code(), Some(3));
}

#[test]
fn interactive_session_keeps_cd_and_variables() {
    let e = Env::new();
    std::fs::create_dir(e.path().join("sub")).unwrap();
    let o = e.cmd().write_stdin("cd sub\nX=1\npwd\necho $X\n").output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let lines: Vec<String> = out(&o).lines().map(|s| s.to_string()).collect();
    assert!(lines[0].ends_with("/sub"), "{lines:?}");
    assert_eq!(lines[1], "1");
    assert!(err(&o).contains("kapsel$ "), "prompt on stderr: {}", err(&o));
}

#[test]
fn interactive_continuation_prompt_and_refused_line() {
    let e = Env::new();
    let o = e.cmd().write_stdin("for i in 1 2; do\necho $i\ndone\nfalse || true\necho after\n").output().unwrap();
    assert_eq!(out(&o), "1\n2\nafter\n", "{}", err(&o));
    assert!(err(&o).contains("> "), "{}", err(&o));
    assert!(err(&o).contains("KS-R001"), "{}", err(&o));
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn interactive_eof_exits_with_the_last_status_and_exit_ends_the_session() {
    let e = Env::new();
    assert_eq!(e.cmd().write_stdin("false\n").output().unwrap().status.code(), Some(1));
    let o = e.cmd().write_stdin("exit 4\necho never\n").output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(4), ""), "{}", err(&o));
}

#[test]
fn compile_writes_a_component() {
    let e = Env::new();
    let o = e.cmd().args(["compile", "-c", "echo hi", "-o", "out.wasm"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let b = std::fs::read(e.path().join("out.wasm")).unwrap();
    assert_eq!(&b[..4], b"\0asm");
}

#[test]
fn compile_of_a_refused_input_exits_65_and_writes_nothing() {
    let e = Env::new();
    let o = e.cmd().args(["compile", "-c", "false || true", "-o", "out.wasm"]).output().unwrap();
    assert_eq!(o.status.code(), Some(65));
    assert!(!e.path().join("out.wasm").exists());
}

#[test]
fn grants_ro_and_rw_flags() {
    let e = Env::new();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("f"), "from ro\n").unwrap();
    let p = other.path().canonicalize().unwrap();
    let ro = p.display().to_string();
    let o = e.cmd().args(["--ro", &ro, "-c", &format!("cat {ro}/f")]).output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "from ro\n"), "{}", err(&o));
    let o = e.cmd().args(["--ro", &ro, "-c", &format!("echo x > {ro}/g")]).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("outside the session's grants"), "{}", err(&o));
    assert!(!p.join("g").exists());
    let o = e.cmd().args(["--rw", &ro, "-c", &format!("echo x > {ro}/g")]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(p.join("g").exists());
    // without a flag the other directory is out of reach
    let o = e.cmd().args(["-c", &format!("cat {ro}/f")]).output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(1), ""));
}

#[test]
fn usage_errors_exit_64() {
    let e = Env::new();
    for args in [
        vec!["-c"],
        vec!["--bogus"],
        vec!["check"],
        vec!["compile", "-c", "echo"],
        vec!["-c", "echo", "file.sh"],
        vec!["--json", "-c", "echo"],
        vec!["a.sh", "b.sh"],
        vec!["--rw"],
    ] {
        let o = e.cmd().args(&args).output().unwrap();
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", err(&o));
        assert!(err(&o).contains("usage:"), "{args:?}");
    }
    let o = e.cmd().arg("missing.sh").output().unwrap();
    assert_eq!(o.status.code(), Some(64));
    assert!(err(&o).contains("missing.sh"), "{}", err(&o));
}

#[test]
fn invalid_grants_exit_78() {
    let e = Env::new();
    e.write("file", "");
    for args in [
        vec!["--rw", "/definitely/not/here", "-c", "echo"],
        vec!["--ro", "/definitely/not/here", "-c", "echo"],
        vec!["--rw", "file", "-c", "echo"],
    ] {
        let o = e.cmd().args(&args).output().unwrap();
        assert_eq!(o.status.code(), Some(78), "{args:?}: {}", err(&o));
        assert_eq!(out(&o), "");
    }
}

#[test]
fn a_missing_key_file_is_an_internal_error() {
    let e = Env::new();
    let o = e.cmd().args(["--key", "nokey.pem", "-c", "echo hi"]).output().unwrap();
    assert_eq!(o.status.code(), Some(70), "{}", err(&o));
    assert_eq!(out(&o), "");
}

#[test]
fn no_home_fails_closed_with_78_and_writes_nothing() {
    let e = Env::new();
    for args in [vec!["-c", "echo hi"], vec!["compile", "-c", "echo hi", "-o", "o.wasm"]] {
        let mut c = Command::cargo_bin("kapsel").unwrap();
        let o = c.current_dir(e.path()).env_clear().args(&args).output().unwrap();
        assert_eq!(o.status.code(), Some(78), "{args:?}: {}", err(&o));
        assert_eq!(err(&o), "kapsel: no KAPSEL_HOME or HOME; set KAPSEL_HOME\n");
        assert_eq!(out(&o), "");
    }
    let names: Vec<_> = std::fs::read_dir(e.path()).unwrap().map(|x| x.unwrap().file_name()).collect();
    assert!(names.is_empty(), "nothing may be created in the working directory: {names:?}");
    // an empty value counts as unset
    let mut c = Command::cargo_bin("kapsel").unwrap();
    let o = c.current_dir(e.path()).env_clear().env("HOME", "").env("KAPSEL_HOME", "").args(["-c", "echo hi"]).output().unwrap();
    assert_eq!(o.status.code(), Some(78));
    // check does not need a home
    let mut c = Command::cargo_bin("kapsel").unwrap();
    let o = c.current_dir(e.path()).env_clear().args(["check", "-c", "echo hi"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
}

#[test]
fn dotdot_works_inside_the_grant() {
    let e = Env::new();
    e.write("a.txt", "A\n");
    std::fs::create_dir(e.path().join("sub")).unwrap();
    let o = e.cmd().args(["-c", "cd sub; cat ../a.txt; pwd > ../where.txt"]).output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "A\n"), "{}", err(&o));
    assert!(std::fs::read_to_string(e.path().join("where.txt")).unwrap().trim_end().ends_with("/sub"));
    let o = e.cmd().args(["-c", "cd .."]).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("kapsel: ..: outside the session's grants"), "{}", err(&o));
}

#[test]
fn nothing_runs_when_check_refuses() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "echo ran > marker; false || true"]).output().unwrap();
    assert_eq!(o.status.code(), Some(65));
    assert!(!e.path().join("marker").exists(), "a refused input must not run any part of itself");
}

#[test]
fn unknown_command_is_refused_not_run() {
    let e = Env::new();
    let o = e.cmd().args(["-c", "frobnicate --now"]).output().unwrap();
    assert_eq!(o.status.code(), Some(65));
    assert!(err(&o).contains("KS-R009"), "{}", err(&o));
}
