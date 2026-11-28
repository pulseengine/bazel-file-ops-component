//! `X=1 > out.txt` through the binary (POSIX 2.9.1, compile rule KC-R23): a
//! command with assignments and redirects but no command name performs the
//! redirects (creating or truncating the file) and the assignments persist
//! in the current environment. Before KC-R23 the compiler declined the shape
//! (KC-001, exit 70).

use assert_cmd::Command;

fn kapsel(dir: &std::path::Path, script: &str) -> std::process::Output {
    Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(dir)
        .env("KAPSEL_HOME", dir.join(".kapsel-home"))
        .args(["-c", script])
        .output()
        .unwrap()
}

/// Mutation: decline `tool: None` with redirects again in
/// `kapsel_compile::lower::select_call`: exit 70, no out.txt.
#[test]
fn assign_with_redirect_creates_the_file_and_the_variable_persists() {
    let dir = tempfile::tempdir().unwrap();
    let o = kapsel(dir.path(), r#"X=1 > out.txt; echo "[$X]""#);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), "[1]\n");
    let out = dir.path().join("out.txt");
    assert!(out.is_file(), "out.txt was not created");
    assert_eq!(std::fs::read(&out).unwrap(), b"");
}

/// The redirect truncates an existing file, and the command alone exits 0.
#[test]
fn assign_with_redirect_truncates_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("out.txt"), "old\n").unwrap();
    let o = kapsel(dir.path(), "X=1 > out.txt");
    assert_eq!(
        o.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(std::fs::read(dir.path().join("out.txt")).unwrap(), b"");
}

/// A redirect that fails (the target is a directory): the status is non-zero
/// and the assignment still holds (the assignments are made before `:`
/// performs the redirects).
#[test]
fn assign_with_failing_redirect_is_nonzero_and_keeps_the_variable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    let o = kapsel(dir.path(), r#"X=1 > d; echo "[$?][$X]""#);
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(
        stdout.starts_with('['),
        "stdout {stdout:?}, stderr {stderr}"
    );
    assert_ne!(stdout, "[0][1]\n", "the failed redirect reported status 0");
    assert!(stdout.ends_with("][1]\n"), "X not kept: {stdout:?}");
    let o = kapsel(dir.path(), "X=1 > d");
    assert_ne!(o.status.code(), Some(0));
    assert_ne!(o.status.code(), Some(70), "compile decline");
}
