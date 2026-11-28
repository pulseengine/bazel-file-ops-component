//! A built-in's own error line (`cat: <name>: No such file or directory`)
//! escapes control characters in the operand, as the rendered diagnostics
//! do (`kapsel_diag::escape_controls`): a file name holding ESC cannot drive
//! the terminal. Data a built-in writes to stdout is never escaped.

use assert_cmd::Command;

const BAD: &str = "bad\x1b[31mname";
const ESCAPED: &str = "bad\\x1b[31mname";
/// File content with ESC in it: `cat` must pass it through byte for byte.
const DATA: &[u8] = b"a\x1b[1mb\n";

struct Out {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

fn run(dir: &std::path::Path, script: &str) -> Out {
    std::fs::write(dir.join("s.sh"), script).unwrap();
    let o = Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(dir)
        .env("KAPSEL_HOME", dir.join(".kapsel-home"))
        .arg("s.sh")
        .output()
        .unwrap();
    Out {
        code: o.status.code(),
        stdout: o.stdout,
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn assert_escaped(o: &Out, line: &str) {
    assert!(
        !o.stderr.contains('\x1b'),
        "raw ESC on stderr: {:?}",
        o.stderr
    );
    assert!(o.stderr.contains(line), "want {line:?} in {:?}", o.stderr);
}

/// Mutation: write the line unescaped in `kapsel_host::shell::err_line`:
/// the raw ESC reaches stderr.
#[test]
fn cat_error_line_escapes_the_operand_and_keeps_stdout_raw() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("good.txt"), DATA).unwrap();
    let o = run(dir.path(), &format!("cat good.txt '{BAD}'\n"));
    assert_eq!(o.code, Some(1), "stderr: {}", o.stderr);
    assert_escaped(&o, &format!("cat: {ESCAPED}: No such file or directory"));
    assert_eq!(o.stdout, DATA, "stdout changed");
}

#[test]
fn rm_error_line_escapes_the_operand() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("good.txt"), DATA).unwrap();
    let o = run(dir.path(), &format!("rm '{BAD}'\ncat good.txt\n"));
    assert_escaped(
        &o,
        &format!("rm: cannot remove '{ESCAPED}': No such file or directory"),
    );
    assert_eq!(o.stdout, DATA, "stdout changed");
}

/// A failed redirect's line is written by the host, not the built-in; it is
/// escaped the same way.
#[test]
fn redirect_error_line_escapes_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(dir.path(), &format!("cat < '{BAD}'\n"));
    assert_ne!(o.code, Some(0));
    assert_escaped(&o, ESCAPED);
    assert_eq!(o.stdout, b"");
}
