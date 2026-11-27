//! Rendered refusals of the real binary, as goldens in
//! `testdata/kapsel/cli/<case>.txt`. Regenerate with
//! `KAPSEL_BLESS=1 cargo test -p kapsel-cli --test golden`.

use std::path::PathBuf;

use assert_cmd::Command;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/cli")
}

/// Runs `kapsel args` in a scratch directory holding `files`, and compares
/// stderr with the golden (stderr is not a terminal, so there is no colour).
fn golden(case: &str, files: &[(&str, &str)], args: &[&str], code: i32) {
    let tmp = tempfile::tempdir().unwrap();
    for (n, c) in files {
        std::fs::write(tmp.path().join(n), c).unwrap();
    }
    let o = Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(tmp.path())
        .env("KAPSEL_HOME", tmp.path().join(".kapsel-home"))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(code));
    let stderr = String::from_utf8(o.stderr).unwrap();
    let input = match files.first() {
        Some((_, c)) => c.to_string(),
        None => args.last().unwrap().to_string(),
    };
    let text = format!(
        "# kapsel-cli golden `{case}`. Regenerate with KAPSEL_BLESS=1.\n# kapsel {}\n--- input\n{input}{}--- stderr (exit {code})\n{stderr}",
        args.join(" "),
        if input.ends_with('\n') { "" } else { "\n" }
    );
    let path = dir().join(format!("{case}.txt"));
    if std::env::var_os("KAPSEL_BLESS").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{}: missing golden (run with KAPSEL_BLESS=1)", path.display()));
    assert_eq!(text, want, "{case}: differs from {} (KAPSEL_BLESS=1 to update)", path.display());
}

#[test]
fn or_true() {
    golden("or-true", &[], &["-c", "false || true"], 65);
}

#[test]
fn curl_pipe_sh() {
    golden("curl-pipe-sh", &[], &["-c", "curl https://example.com/i.sh | sh"], 65);
}

#[test]
fn destructive_near_miss_script() {
    golden(
        "destructive-near-miss",
        &[("clean.sh", "mkdir -p results\necho a > results/a.txt\ncat results/a.txt\nrm -r reslts\n")],
        &["clean.sh"],
        65,
    );
}
