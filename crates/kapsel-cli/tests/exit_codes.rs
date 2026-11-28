//! REQ-KS-018: exit codes through the shipped binary, and the toolchain pins.

mod common;

use std::path::Path;

use assert_cmd::Command;

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let scratch = tempfile::tempdir().unwrap();
    let o = Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(scratch.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("KAPSEL_HOME", scratch.path().join("home"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn a_failing_built_in_exits_2_and_false_exits_1_and_exit_7_exits_7() {
    let (code, out, err) = run(&["-c", "grep x missing.txt"]);
    assert_eq!(code, Some(2), "{err}");
    assert_eq!(out, "");
    assert!(
        err.contains("grep: missing.txt: No such file or directory"),
        "{err}"
    );
    assert_eq!(run(&["-c", "false"]).0, Some(1));
    assert_eq!(run(&["-c", "exit 7"]).0, Some(7));
    // The same through a script file.
    let dir = tempfile::tempdir().unwrap();
    for (text, want) in [("grep x missing.txt\n", 2), ("false\n", 1), ("exit 7\n", 7)] {
        let f = dir.path().join("s.sh");
        std::fs::write(&f, text).unwrap();
        assert_eq!(run(&[f.to_str().unwrap()]).0, Some(want), "{text:?}");
    }
}

/// `1.100`, `1.100.0` -> (1, 100, 0).
fn version(s: &str) -> (u32, u32, u32) {
    let mut it = s.trim().split('.').map(|p| p.parse::<u32>().unwrap());
    (
        it.next().unwrap(),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// The quoted value of `key = "value"` in a TOML file (first match, no tables).
fn toml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        Some(v.trim().trim_matches('"').to_string())
    })
}

const MIN_RUST: (u32, u32, u32) = (1, 100, 0);

#[test]
fn rust_toolchain_and_every_rust_version_are_at_least_1_100() {
    let root = common::repo_root();
    let tc = std::fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
    let channel = toml_value(&tc, "channel").expect("rust-toolchain.toml has no channel");
    assert!(
        version(&channel) >= MIN_RUST,
        "rust-toolchain.toml channel {channel} is older than 1.100"
    );
    let ws = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let ws_rv = toml_value(&ws, "rust-version").expect("workspace has no rust-version");
    assert!(
        version(&ws_rv) >= MIN_RUST,
        "workspace rust-version {ws_rv} is older than 1.100"
    );
    let mut crates = 0;
    for e in std::fs::read_dir(root.join("crates")).unwrap() {
        let manifest = e.unwrap().path().join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        let name = manifest.parent().unwrap().file_name().unwrap().to_owned();
        let rv = match toml_value(&text, "rust-version") {
            Some(v) => v,
            None if text
                .lines()
                .any(|l| l.replace(' ', "") == "rust-version.workspace=true") =>
            {
                ws_rv.clone()
            }
            None => panic!("{}: no rust-version", Path::new(&name).display()),
        };
        assert!(
            version(&rv) >= MIN_RUST,
            "{}: rust-version {rv} is older than 1.100",
            Path::new(&name).display()
        );
        crates += 1;
    }
    assert!(crates >= 14, "only {crates} crates found");
}

#[test]
fn varve_toolchain_layer_is_2026_11_28_or_later() {
    let text = std::fs::read_to_string(common::repo_root().join("varve.toml")).unwrap();
    let layer = toml_value(&text, "layer").expect("varve.toml has no layer");
    let parts: Vec<u32> = layer.split('.').map(|p| p.parse().unwrap()).collect();
    assert_eq!(parts.len(), 3, "layer {layer} is not YYYY.MM.DD");
    assert!(
        (parts[0], parts[1], parts[2]) >= (2026, 11, 28),
        "varve.toml layer {layer} is older than 2026.11.28"
    );
    assert_eq!(toml_value(&text, "realm").as_deref(), Some("covalent"));
}

/// REQ-KS-014 and KS-R018: a script's own `exit 64|65|70|78` gives the same
/// status as kapsel's own failure, and the two are told apart the way
/// docs/first-shell.md says: kapsel's failures print `kapsel:` or an
/// `error[KS-…]` diagnostic on stderr, and `check --json` settles refusal
/// before anything runs.
#[test]
fn a_script_exit_code_that_collides_is_told_apart_from_kapsel_s_own() {
    // A line that starts with `kapsel:` or `error[KS-`.
    let marked = |err: &str| {
        err.lines()
            .any(|l| l.starts_with("kapsel:") || l.starts_with("error[KS-"))
    };
    for (code, kapsel_failure) in [
        (64, vec!["-c"]),
        (65, vec!["-c", "false || true"]),
        (78, vec!["--rw", "/definitely/not/here", "-c", "echo"]),
    ] {
        let (k_code, _, k_err) = run(&kapsel_failure);
        let script = format!("exit {code}");
        let (s_code, _, s_err) = run(&["-c", &script]);
        assert_eq!(k_code, Some(code), "{kapsel_failure:?}: {k_err}");
        assert_eq!(s_code, Some(code), "{script}: {s_err}");
        assert!(
            marked(&k_err),
            "kapsel's own failure is marked on stderr: {k_err}"
        );
        assert!(
            !marked(&s_err),
            "the script's own exit is not marked as kapsel's: {s_err}"
        );
        assert!(s_err.contains("warning[KS-R018]"), "{s_err}");
        // check --json: a plan (exit 0) for the script, a refusal for kapsel's.
        let (c_code, c_out, _) = run(&["check", "--json", "-c", &script]);
        assert_eq!(c_code, Some(0));
        assert!(c_out.contains("\"KS-R018\""), "{c_out}");
    }
    let (c_code, c_out, _) = run(&["check", "--json", "-c", "false || true"]);
    assert_eq!(c_code, Some(65));
    assert!(c_out.contains("\"KS-R001\""), "{c_out}");
}
