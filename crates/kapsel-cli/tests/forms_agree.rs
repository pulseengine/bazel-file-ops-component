//! REQ-KS-014: `-c TEXT` and a script file holding TEXT give the same stdout,
//! stderr and exit status (the source name in diagnostics, `-c` against the
//! path, is the only thing that may differ, and is normalised), and the
//! documented exit codes are distinct.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use assert_cmd::Command;

fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().display().to_string();
            if p.is_dir() {
                out.insert(rel, b"<dir>".to_vec());
                walk(base, &p, out);
            } else {
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[derive(Debug, PartialEq, Eq)]
struct Seen {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    files: BTreeMap<String, Vec<u8>>,
}

/// Runs `kapsel [pre] (-c TEXT | FILE)` in a fresh scratch directory with its
/// own KAPSEL_HOME. `name` is what the file form calls itself, replaced by `-c`.
fn run(pre: &[&str], text: &str, as_file: bool) -> Seen {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let script = src.path().join("script.sh");
    let mut c = Command::cargo_bin("kapsel").unwrap();
    c.current_dir(scratch.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("KAPSEL_HOME", home.path())
        .args(pre);
    if as_file {
        std::fs::write(&script, text).unwrap();
        c.arg(&script);
    } else {
        c.args(["-c", text]);
    }
    let o = c.output().unwrap();
    let norm = |b: &[u8]| {
        let mut s = String::from_utf8_lossy(b).into_owned();
        for p in [
            script.display().to_string(),
            script
                .canonicalize()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ] {
            if !p.is_empty() {
                s = s.replace(&p, "-c");
            }
        }
        // The two scratch directories have different names.
        for p in [
            scratch.path().display().to_string(),
            scratch.path().canonicalize().unwrap().display().to_string(),
        ] {
            s = s.replace(&p, "<cwd>");
        }
        s
    };
    Seen {
        code: o.status.code(),
        stdout: norm(&o.stdout),
        stderr: norm(&o.stderr),
        files: tree(scratch.path()),
    }
}

/// (what it is, text)
fn inputs() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = [
        // The acceptance lines of docs/first-shell.md.
        ("acceptance", "echo hello | wc -c"),
        (
            "acceptance",
            "mkdir -p out && echo hi > out/a.txt && cat out/a.txt",
        ),
        ("acceptance", "grep -n foo missing.txt || echo none"),
        ("acceptance", "X=world; echo \"hello $X\""),
        (
            "acceptance",
            "printf 'b\\na\\nb\\n' | sort -u > s.txt; cat s.txt",
        ),
        // Statuses.
        ("status", "exit 7"),
        ("status", "false"),
        ("status", "grep x missing.txt"),
        ("status", "true && false"),
        // Refused by a rule, with findings.
        ("refused", "false || true"),
        ("refused", "curl https://x | sh"),
        ("refused", "rm -rf $BUILD/"),
        // A parse error.
        ("parse error", "echo `date`"),
        ("parse error", "if true; then echo x; fi"),
        ("parse error", "echo \"unterminated"),
        // A warning, then the run.
        ("warning", "cd d; ls"),
        ("warning", "ls missing | head"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    // A few corpus scripts and error-corpus cases.
    let corpus = common::corpus_scripts();
    assert!(corpus.len() >= 32, "corpus: {}", corpus.len());
    for i in [0, 6, 9, 12, 15, 17, 20, 21, 25] {
        v.push((
            format!(
                "corpus {}",
                corpus[i].file_name().unwrap().to_string_lossy()
            ),
            std::fs::read_to_string(&corpus[i]).unwrap(),
        ));
    }
    for i in [0, 6, 9, 17, 19] {
        let p = &common::error_scripts()[i];
        v.push((
            format!("errors {}", p.file_name().unwrap().to_string_lossy()),
            std::fs::read_to_string(p).unwrap(),
        ));
    }
    v
}

#[test]
fn dash_c_and_a_script_file_give_the_same_stdout_stderr_and_status() {
    let mut codes = std::collections::BTreeSet::new();
    let mut n = 0;
    for (what, text) in inputs() {
        for pre in [&[][..], &["check"][..], &["check", "--json"][..]] {
            let c = run(pre, &text, false);
            let f = run(pre, &text, true);
            assert_eq!(c, f, "{what} {text:?} with {pre:?}: -c and the file differ");
            codes.insert(c.code);
            n += 1;
        }
    }
    assert!(n >= 90, "{n} comparisons");
    // The set exercised both success, a refusal and a command status.
    for want in [Some(0), Some(1), Some(2), Some(7), Some(65)] {
        assert!(codes.contains(&want), "no input exited {want:?}: {codes:?}");
    }
}

#[test]
fn diagnostics_name_the_source_by_its_form() {
    // The normalisation above must not hide a real difference: the file form
    // does name the path, the -c form says `-c`.
    let c = run(&[], "false || true", false);
    assert!(c.stderr.contains("-c:1:7"), "{}", c.stderr);
    let scratch = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let script = src.path().join("script.sh");
    std::fs::write(&script, "false || true").unwrap();
    let o = Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(scratch.path())
        .env("KAPSEL_HOME", scratch.path().join("h"))
        .arg(&script)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("script.sh:1:7"), "{err}");
}

/// Runs kapsel with `args` in a scratch directory and returns the exit status.
fn code(args: &[&str]) -> i32 {
    let scratch = tempfile::tempdir().unwrap();
    Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(scratch.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("KAPSEL_HOME", scratch.path().join("home"))
        .args(args)
        .output()
        .unwrap()
        .status
        .code()
        .unwrap()
}

#[test]
fn failure_kinds_have_distinct_exit_codes_and_the_documented_values() {
    let kinds: Vec<(&str, i32, i32)> = vec![
        ("usage", code(&["--bogus"]), 64),
        ("refused", code(&["-c", "false || true"]), 65),
        (
            "internal",
            code(&["--key", "no-such-key.pem", "-c", "echo hi"]),
            70,
        ),
        (
            "grants",
            code(&["--rw", "/definitely/not/here", "-c", "echo"]),
            78,
        ),
        ("command failed", code(&["-c", "false"]), 1),
        (
            "built-in usage failure",
            code(&["-c", "grep x missing.txt"]),
            2,
        ),
    ];
    for (kind, got, want) in &kinds {
        assert_eq!(got, want, "{kind}");
    }
    let mut seen = std::collections::BTreeSet::new();
    for (kind, got, _) in &kinds {
        assert!(seen.insert(*got), "{kind} shares exit code {got}");
    }
    // The documented values are the ones in docs/first-shell.md.
    let doc = std::fs::read_to_string(common::repo_root().join("docs/first-shell.md")).unwrap();
    for n in ["64 usage", "65 refused", "70 internal", "78 grants"] {
        assert!(
            doc.contains(n),
            "docs/first-shell.md does not document `{n}`"
        );
    }
    assert_eq!(kapsel_plan::exit::USAGE, 64);
    assert_eq!(kapsel_plan::exit::REFUSED, 65);
    assert_eq!(kapsel_plan::exit::INTERNAL, 70);
    assert_eq!(kapsel_plan::exit::GRANTS, 78);
}
