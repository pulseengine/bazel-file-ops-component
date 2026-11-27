//! Golden tests over `testdata/kapsel/parse/` (REQ-KS-010).
//!
//! - `accept/*.sh` must parse; the AST must equal `*.json`; every AST span
//!   must be exact and nested; the tree text must equal the input.
//! - `refuse/*.sh` must fail; the full error list (construct, span text,
//!   expected, help) must equal `*.expected`.
//!
//! Regenerate with `KAPSEL_BLESS=1 cargo test -p kapsel-syntax --test golden`
//! and review every changed golden by hand.

mod common;

use std::path::{Path, PathBuf};

fn dir(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/kapsel/parse")
        .join(kind)
}

fn cases(kind: &str) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir(kind))
        .expect("testdata dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "sh"))
        .collect();
    v.sort();
    v
}

fn bless() -> bool {
    std::env::var_os("KAPSEL_BLESS").is_some()
}

/// Compares `actual` with the golden file, or writes it when blessing.
fn golden(path: &Path, actual: &str, failures: &mut Vec<String>) {
    if bless() {
        std::fs::write(path, actual).expect("write golden");
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(expected) if expected == actual => {}
        Ok(expected) => failures.push(format!(
            "{}: golden differs\n--- expected\n{expected}\n--- actual\n{actual}",
            path.display()
        )),
        Err(_) => failures.push(format!(
            "{}: missing golden (run with KAPSEL_BLESS=1)",
            path.display()
        )),
    }
}

#[test]
fn accept_goldens() {
    let cases = cases("accept");
    assert!(
        cases.len() >= 40,
        "need at least 40 accept cases, have {}",
        cases.len()
    );
    let mut failures = Vec::new();
    for case in &cases {
        let src = std::fs::read_to_string(case).expect("read case");
        match kapsel_syntax::parse(&src) {
            Ok(script) => {
                let json = serde_json::to_string_pretty(&script).expect("serialize") + "\n";
                golden(&case.with_extension("json"), &json, &mut failures);
            }
            Err(errors) => failures.push(format!(
                "{}: expected to parse, got\n{}",
                case.display(),
                common::render_errors(&src, &errors)
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn accept_spans_exact_and_nested() {
    let mut failures = Vec::new();
    for case in cases("accept") {
        let src = std::fs::read_to_string(&case).expect("read case");
        if let Ok(script) = kapsel_syntax::parse(&src)
            && let Err(e) = common::check_spans(&src, &script)
        {
            failures.push(format!("{}:\n{e}", case.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn refuse_goldens() {
    let cases = cases("refuse");
    assert!(
        cases.len() >= 25,
        "need at least 25 refuse cases, have {}",
        cases.len()
    );
    let mut failures = Vec::new();
    for case in &cases {
        let src = std::fs::read_to_string(case).expect("read case");
        let parse = kapsel_syntax::parse_green(&src);
        if parse.ok() {
            failures.push(format!("{}: expected errors, parsed fine", case.display()));
            continue;
        }
        if let Err(e) = common::check_errors(&src, parse.errors()) {
            failures.push(format!("{}: {e}", case.display()));
        }
        let rendered = common::render_errors(&src, parse.errors());
        golden(&case.with_extension("expected"), &rendered, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn lossless_on_all_goldens() {
    for case in cases("accept").into_iter().chain(cases("refuse")) {
        let src = std::fs::read_to_string(&case).expect("read case");
        let tree = kapsel_syntax::parse_green(&src).syntax_node();
        assert_eq!(tree.text().to_string(), src, "{}", case.display());
    }
}

/// Differential sanity: dash also accepts every accept case.
#[test]
fn dash_accepts_accept_cases() {
    let dash = Path::new("/bin/dash");
    if !dash.exists() {
        eprintln!("skipping: /bin/dash not found");
        return;
    }
    let mut failures = Vec::new();
    for case in cases("accept") {
        let out = std::process::Command::new(dash)
            .arg("-n")
            .arg(&case)
            .output()
            .expect("run dash");
        if !out.status.success() {
            failures.push(format!(
                "{}: {}",
                case.display(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The dash differential corpus must parse too.
#[test]
fn corpus_parses() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/kapsel/corpus");
    let Ok(rd) = std::fs::read_dir(&corpus) else {
        return;
    };
    let mut failures = Vec::new();
    for e in rd {
        let p = e.expect("entry").path();
        if p.extension().is_none_or(|e| e != "sh") {
            continue;
        }
        let src = std::fs::read_to_string(&p).expect("read");
        if let Err(errors) = kapsel_syntax::parse(&src) {
            failures.push(format!(
                "{}:\n{}",
                p.display(),
                common::render_errors(&src, &errors)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn incomplete_inputs() {
    for src in [
        "echo 'abc",
        "echo \"abc",
        "cat <<EOF\nline\n",
        "cat <<EOF",
        "echo $(ls",
        "for x in a b; do echo $x",
        "while true; do",
        "echo a |",
        "echo a &&",
        "echo a ||",
        "echo a \\",
        "if true; then",
        "echo ${x",
        "echo `ls",
    ] {
        assert!(
            kapsel_syntax::is_incomplete(src),
            "should be incomplete: {src:?}"
        );
    }
    for src in [
        "echo a",
        "echo )",
        "echo a; ",
        "cat <<EOF\nx\nEOF\n",
        "for x in a; do echo; done",
        "",
    ] {
        assert!(
            !kapsel_syntax::is_incomplete(src),
            "should be complete: {src:?}"
        );
    }
}
