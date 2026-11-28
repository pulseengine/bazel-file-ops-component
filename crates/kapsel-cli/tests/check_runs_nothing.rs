//! REQ-KS-011: `kapsel check` runs nothing. Over every plant-pack script, parse
//! golden input and error-corpus case, in an empty scratch directory:
//!
//! * no file or directory is created, changed or removed there (a before/after
//!   snapshot of names, kinds, sizes, mtimes and contents), nor in the
//!   `KAPSEL_HOME` that check has no need for;
//! * no child process is started. Instrument: a PATH that holds a tripwire
//!   executable for every name a shell might run (the built-ins, `sh`, `curl`,
//!   ...). Each one appends to a log and exits 99, so a spawn is both recorded
//!   and visible; a second run with an empty PATH must give the same result.
//!   Blind spots: a program started by absolute path (`/bin/sh`), a spawn from
//!   code that sets its own PATH, and any network access. On Linux, when
//!   `strace` is installed, a third test counts `execve`, `fork`, `clone`
//!   without CLONE_THREAD and AF_INET sockets at the OS level. With
//!   `KAPSEL_REQUIRE_STRACE=1` (set in CI) a missing or non-tracing strace,
//!   or a non-Linux host, fails instead of skipping;
//! * the same results come out with HOME and KAPSEL_HOME unset.

#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    kind: &'static str,
    len: u64,
    mtime: Option<std::time::SystemTime>,
    content: Vec<u8>,
}

fn snapshot(dir: &Path) -> BTreeMap<String, Entry> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Entry>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().display().to_string();
            let md = std::fs::symlink_metadata(&p).unwrap();
            let kind = if md.is_dir() {
                "dir"
            } else if md.file_type().is_symlink() {
                "symlink"
            } else {
                "file"
            };
            let content = if kind == "file" {
                std::fs::read(&p).unwrap()
            } else {
                vec![]
            };
            out.insert(
                rel,
                Entry {
                    kind,
                    len: md.len(),
                    mtime: md.modified().ok(),
                    content,
                },
            );
            if kind == "dir" {
                walk(base, &p, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    // The directory's own mtime changes when an entry is created or removed.
    let md = std::fs::metadata(dir).unwrap();
    out.insert(
        ".".to_string(),
        Entry {
            kind: "dir",
            len: 0,
            mtime: md.modified().ok(),
            content: vec![],
        },
    );
    walk(dir, dir, &mut out);
    out
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Result3 {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Env {
    /// Normal PATH, KAPSEL_HOME in a directory outside the scratch directory.
    Baseline,
    /// Tripwire PATH, HOME and KAPSEL_HOME unset.
    Tripwire,
    /// Empty PATH, HOME and KAPSEL_HOME unset.
    EmptyPath,
}

struct Rig {
    scratch: tempfile::TempDir,
    home: tempfile::TempDir,
    trip: tempfile::TempDir,
    log: PathBuf,
}

impl Rig {
    fn new() -> Rig {
        let trip = tempfile::tempdir().unwrap();
        let log = trip.path().join("spawned.log");
        let bin = trip.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let mut names: Vec<&str> = kapsel_plan::BUILTINS.to_vec();
        names.extend([
            "sh", "bash", "dash", "zsh", "ksh", "env", "curl", "wget", "git", "python", "python3",
            "perl", "ruby", "node", "awk", "find", "xargs", "uname",
        ]);
        for n in names {
            let p = bin.join(n);
            std::fs::write(
                &p,
                format!(
                    "#!/bin/sh\necho \"{n} $*\" >> '{}'\nexit 99\n",
                    log.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Rig {
            scratch: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
            trip,
            log,
        }
    }

    fn tripwire_path(&self) -> PathBuf {
        self.trip.path().join("bin")
    }

    fn spawned(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn cmd(&self, env: Env) -> Command {
        let mut c = Command::cargo_bin("kapsel").unwrap();
        c.env_clear()
            .env("LC_ALL", "C")
            .current_dir(self.scratch.path());
        match env {
            Env::Baseline => {
                c.env("PATH", "/usr/bin:/bin")
                    .env("KAPSEL_HOME", self.home.path());
            }
            Env::Tripwire => {
                c.env("PATH", self.tripwire_path());
            }
            Env::EmptyPath => {
                c.env("PATH", "");
            }
        }
        c
    }
}

fn finish(o: std::process::Output) -> Result3 {
    Result3 {
        code: o.status.code(),
        stdout: o.stdout,
        stderr: o.stderr,
    }
}

/// The three ways of giving check an input: `-c`, a script file, `--json`.
fn modes(script: &Path, text: &str) -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("-c", vec!["check".into(), "-c".into(), text.to_string()]),
        ("file", vec!["check".into(), script.display().to_string()]),
        (
            "--json",
            vec![
                "check".into(),
                "--json".into(),
                script.display().to_string(),
            ],
        ),
    ]
}

#[test]
fn the_tripwire_catches_a_spawn() {
    // The instrument must be able to fail: running a stub records it.
    let rig = Rig::new();
    let stub = rig.tripwire_path().join("sh");
    let st = std::process::Command::new(stub).arg("x").status().unwrap();
    assert_eq!(st.code(), Some(99));
    assert_eq!(rig.spawned().trim(), "sh x");
}

#[test]
fn check_creates_changes_and_removes_nothing_and_spawns_nothing() {
    let rig = Rig::new();
    let inputs = common::check_inputs();
    assert!(inputs.len() >= 150, "only {} inputs", inputs.len());
    let before = snapshot(rig.scratch.path());
    let home_before = snapshot(rig.home.path());
    assert_eq!(before.len(), 1, "scratch directory must start empty");
    let (mut ok, mut refused) = (0, 0);
    for script in &inputs {
        let text = std::fs::read_to_string(script).unwrap();
        for (mode, args) in modes(script, &text) {
            let base = finish(rig.cmd(Env::Baseline).args(&args).output().unwrap());
            match base.code {
                Some(0) => ok += 1,
                Some(65) => refused += 1,
                other => panic!(
                    "{} {mode}: unexpected exit {other:?}: {}",
                    script.display(),
                    String::from_utf8_lossy(&base.stderr)
                ),
            }
            for env in [Env::Tripwire, Env::EmptyPath] {
                let r = finish(rig.cmd(env).args(&args).output().unwrap());
                assert_eq!(
                    r,
                    base,
                    "{} {mode}: result differs under {env:?}",
                    script.display()
                );
            }
            assert_eq!(
                snapshot(rig.scratch.path()),
                before,
                "{} {mode}: the scratch directory changed",
                script.display()
            );
            assert_eq!(
                snapshot(rig.home.path()),
                home_before,
                "{} {mode}: KAPSEL_HOME was written",
                script.display()
            );
            assert_eq!(
                rig.spawned(),
                "",
                "{} {mode}: a child process was started",
                script.display()
            );
        }
    }
    // Both outcomes occurred, so the loop did not test one kind of input only.
    assert!(ok > 50 && refused > 50, "ok {ok}, refused {refused}");
}

/// The syscall part of one `strace -f` line. strace prefixes the pid either as
/// `[pid N] ` or, with `-o`, as `N  ` (digits then spaces, as on ubuntu-24.04).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn syscall_of(line: &str) -> &str {
    let l = line.trim_start();
    let l = l
        .strip_prefix('[')
        .map_or(l, |r| r.split_once("] ").map_or(l, |x| x.1));
    match l.find(|c: char| !c.is_ascii_digit()) {
        Some(n) if n > 0 && l[n..].starts_with(' ') => l[n..].trim_start(),
        _ => l,
    }
}

#[test]
fn syscall_of_strips_both_pid_prefixes() {
    // Verbatim from the ubuntu-24.04 CI log of rust.yml run 46121479564, which
    // this parser miscounted as zero execs before it handled the `N  ` prefix.
    let ci = r#"9624  execve("/home/runner/work/kapsel/kapsel/target/debug/kapsel", ["/home/runner/work/kapsel/kapsel/"..., "check", "/home/runner/work/kapsel/kapsel/"...], 0x7fff8f6d9f00 /* 1 var */) = 0"#;
    assert!(syscall_of(ci).starts_with("execve("));
    assert!(syscall_of("[pid 9625] clone3({flags=CLONE_VM}, 88) = 9626").starts_with("clone3("));
    assert!(syscall_of(r#"execve("/bin/x", [], 0x0) = 0"#).starts_with("execve("));
    assert_eq!(syscall_of("1234"), "1234");
}

/// True when `KAPSEL_REQUIRE_STRACE=1`: CI installs strace and sets it, so a
/// skipped trace there is a failure, not a silent pass.
fn strace_required() -> bool {
    std::env::var("KAPSEL_REQUIRE_STRACE").as_deref() == Ok("1")
}

/// Skips the strace test with a message, or fails it when strace is required.
fn skip_or_fail(why: &str) {
    assert!(
        !strace_required(),
        "KAPSEL_REQUIRE_STRACE=1 but the OS-level trace cannot run: {why}"
    );
    eprintln!("strace test skipped: {why}");
}

#[test]
fn strace_requirement_is_read_from_the_environment() {
    // The switch the CI step sets must be the one this file reads.
    let ci =
        std::fs::read_to_string(common::repo_root().join(".github/workflows/rust.yml")).unwrap();
    assert!(
        ci.contains("KAPSEL_REQUIRE_STRACE: \"1\""),
        "rust.yml's test step must set KAPSEL_REQUIRE_STRACE"
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn check_under_strace_starts_no_process_and_opens_no_socket() {
    skip_or_fail("strace exists only on Linux");
}

/// OS-level count of process starts and network sockets, where `strace` exists.
#[cfg(target_os = "linux")]
#[test]
fn check_under_strace_starts_no_process_and_opens_no_socket() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let strace = std::env::split_paths(&path)
        .map(|d| d.join("strace"))
        .find(|p| p.is_file());
    let Some(strace) = strace else {
        skip_or_fail("strace is not installed");
        return;
    };
    let rig = Rig::new();
    let kapsel = Command::cargo_bin("kapsel")
        .unwrap()
        .get_program()
        .to_owned();
    let mut inputs = common::rule_scripts();
    inputs.extend(common::error_scripts());
    let mut traced = 0;
    for script in &inputs {
        let log = rig.trip.path().join("strace.log");
        let _ = std::fs::remove_file(&log);
        let out = std::process::Command::new(&strace)
            .args(["-f", "-qq", "-o"])
            .arg(&log)
            .args([
                "-e",
                "trace=execve,execveat,fork,vfork,clone,clone3,socket,connect",
            ])
            .arg(&kapsel)
            .arg("check")
            .arg(script)
            .current_dir(rig.scratch.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if !text.lines().any(|l| l.contains("execve(")) {
            skip_or_fail(&format!(
                "strace could not trace here (no execve line): {}",
                String::from_utf8_lossy(&out.stderr)
            ));
            return;
        }
        traced += 1;
        let mut execs = 0;
        for line in text.lines() {
            let l = syscall_of(line);
            if l.starts_with("execve(") || l.starts_with("execveat(") {
                execs += 1;
            } else if l.starts_with("fork(") || l.starts_with("vfork(") {
                panic!("{}: fork: {line}", script.display());
            } else if (l.starts_with("clone(") || l.starts_with("clone3("))
                && !l.contains("CLONE_THREAD")
            {
                panic!("{}: a process was cloned: {line}", script.display());
            } else if (l.starts_with("socket(") || l.starts_with("connect("))
                && l.contains("AF_INET")
            {
                panic!("{}: network access: {line}", script.display());
            }
        }
        assert_eq!(
            execs,
            1,
            "{}: more than the kapsel exec: {text}",
            script.display()
        );
    }
    println!(
        "strace: traced {traced} of {} `kapsel check` inputs, each with exactly one execve, no fork, no process clone, no AF_INET socket",
        inputs.len()
    );
    assert_eq!(traced, inputs.len());
    assert!(traced > 40);
    assert!(snapshot(rig.scratch.path()).len() == 1);
}
