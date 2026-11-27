//! Equivalence of the Rust `fork-guard` rule with the frozen Python oracle
//! (REQ-KS-064). The oracle is run, never extended: for every fixture under
//! `testdata/kapsel/scan/fork-guard/<case>/` it runs from a temporary copy of
//! the fixture (it finds the repository root from its own location, so it is
//! placed at `<tmp>/scripts/`), and the Rust rule runs on the same fixture.
//!
//! Every fixture is classified below, and a case that is not listed fails.
//! Rust may be stricter than the script. It is wider in exactly two named
//! classes, each pinned by a test so the list cannot grow unnoticed:
//!   - `Accepts`: the script's one text form rejects a guard that is
//!     semantically correct (a fork PR cannot reach a non-hosted runner), and
//!     the expression evaluator accepts it;
//!   - `Looser`: the script's regex flags text that is not a YAML key.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq)]
enum Kind {
    /// Same stdout, same stderr, same exit code.
    Same,
    /// The script passes, Rust fails (or both fail and Rust lists more FAIL
    /// lines). The string is the reason, which is the report.
    Stricter(&'static str),
    /// Same verdict; Rust lists more sites, none of them better than the script's.
    MoreSites(&'static str),
    /// The script fails, Rust passes, because the guard is semantically correct.
    Accepts(&'static str),
    /// The script fails, Rust passes, because the script read text that is not
    /// a YAML key. Rust is looser here by design.
    Looser(&'static str),
}
use Kind::*;

const ORACLE_BODY_SHA256: &str = "2596656bac4c64210f86128ec2991c09842614f75a2182efa79df9ddb952e377";
const MARKER: &str = "# --- original below ---\n";

fn table() -> Vec<(&'static str, Kind)> {
    vec![
        // Same: identical output and exit code.
        ("real-kapsel", Same),
        ("guarded", Same),
        ("list-flow-unguarded", Same),
        ("list-unguarded-pr", Same),
        ("plain-unguarded", Same),
        ("fork-form", Same),
        ("gale-fork-form", Same),
        ("relay-fork-matrix", Same),
        ("prt-selfhosted", Same),
        ("prt-hosted-only", Same),
        ("prt-other-file", Same),
        ("matrix-hosted", Same),
        ("label-group-only", Same),
        ("no-runs-on", Same),
        ("reusable-input", Same),
        ("no-selfhosted", Same),
        ("crlf-guarded", Same),
        ("commented-out", Same),
        ("prt-comment-only", Same),
        ("prt-in-run-string", Same),
        ("yaml-extension", Same),
        ("empty-repo", Same),
        ("odd-spacing", Same),
        ("hosted-fallback-not-hosted", Same),
        ("hosted-fallback-selfhosted", Same),
        ("inverted-guard", Same),
        ("unknown-condition", Same),
        ("nested-outer-unguarded", Same),
        ("matrix-guard-custom-labels", Same),
        ("matrix-guard-gale-custom", Same),
        ("matrix-guard-unenumerable", Same),
        ("matrix-os-lean-custom", Same),
        ("matrix-os-rocq-custom", Same),
        // Stricter: the script passes or lists fewer failing sites.
        (
            "matrix-selfhosted",
            Stricter(
                "runs-on: ${{ matrix.os }} with self-hosted in the matrix; the script sees no self-hosted text on the line",
            ),
        ),
        (
            "list-block",
            Stricter("block-sequence runs-on; the labels are on later lines"),
        ),
        (
            "label-group-selfhosted",
            Stricter(
                "runs-on mapping with labels: [self-hosted, ...]; the labels are on a later line",
            ),
        ),
        (
            "multi-document",
            Stricter("a workflow file must be one YAML document; rivet-yaml refuses a stream"),
        ),
        (
            "flow-mapping-job",
            Stricter(
                "runs-on inside a flow mapping `{runs-on: self-hosted}`; the line regex needs the key to start a line",
            ),
        ),
        (
            "anchor-unguarded",
            Stricter(
                "unguarded label list anchored and aliased; the alias job also selects self-hosted",
            ),
        ),
        // MoreSites
        (
            "folded-guard",
            MoreSites(
                "guard as a folded block scalar: correct, and a second site the script cannot see",
            ),
        ),
        // Accepts: correct guards the script's text form rejects.
        (
            "hosted-fallback-latest",
            Accepts("hosted fallback 'ubuntu-latest' instead of 'ubuntu-24.04'"),
        ),
        (
            "guard-trailing-comment",
            Accepts("trailing YAML comment after the expression"),
        ),
        (
            "ordeal-comment",
            Accepts("trailing YAML comment after the expression (ordeal ci.yml shape)"),
        ),
        (
            "guard-quoted",
            Accepts("the guard in double quotes: the same string"),
        ),
        (
            "spar-nested",
            Accepts(
                "guard && (cond && A || B) || hosted: for a fork the guard is false, so the nested label arrays are unreachable",
            ),
        ),
        (
            "spar-nested-inner-unknown",
            Accepts(
                "as spar-nested with an unknown inner branch: unreachable for a fork PR, so still correct",
            ),
        ),
        (
            "matrix-guard-varve",
            Accepts(
                "matrix guard: every matrix.runner containing self-hosted falls back to hosted, the others are hosted labels",
            ),
        ),
        (
            "matrix-guard-gale-json",
            Accepts("matrix guard with fromJSON(matrix.runner) and JSON strings in the matrix"),
        ),
        (
            "matrix-os-lean",
            Accepts(
                "matrix.os == 'linux' && guard || matrix.os: every other matrix.os is a hosted label",
            ),
        ),
        (
            "matrix-os-rocq",
            Accepts(
                "matrix.os != 'ubuntu-latest' && matrix.os || guard: every other matrix.os is a hosted label",
            ),
        ),
        (
            "anchor-guarded",
            Accepts("anchored guard and its alias get the guard's verdict"),
        ),
        (
            "alias-runs-on",
            Accepts("anchored guard and its alias get the guard's verdict"),
        ),
        // Looser: the script's false positive.
        (
            "block-scalar-text",
            Looser(
                "`runs-on: self-hosted` is text inside a run: block scalar, not a key; the script flags it, the YAML reading does not",
            ),
        ),
    ]
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("testdata/kapsel/scan/fork-guard")
}

fn oracle_text() -> String {
    let p = repo().join("testdata/kapsel/scan/oracle/check_selfhosted_fork_guard.py.frozen");
    std::fs::read_to_string(p).expect("frozen oracle present")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

/// Run the oracle on a copy of the fixture, with the copy as cwd.
fn run_python(case: &str, oracle: &str) -> Out {
    let tmp =
        std::env::temp_dir().join(format!("kapsel-scan-oracle-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&fixtures().join(case), &tmp);
    std::fs::create_dir_all(tmp.join("scripts")).unwrap();
    std::fs::write(tmp.join("scripts/check_selfhosted_fork_guard.py"), oracle).unwrap();
    let o = Command::new("python3")
        .arg("scripts/check_selfhosted_fork_guard.py")
        .current_dir(&tmp)
        .output()
        .expect("python3 is needed to run the frozen oracle");
    let _ = std::fs::remove_dir_all(&tmp);
    Out {
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        code: o.status.code().unwrap_or(-1),
    }
}

fn run_rust(case: &str) -> Out {
    let (stdout, stderr, code) = kapsel_scan::fork_guard(&fixtures().join(case)).render();
    Out {
        stdout,
        stderr,
        code,
    }
}

fn lines(s: &str, tag: &str) -> BTreeSet<String> {
    s.lines()
        .filter(|l| l.starts_with(tag))
        .map(String::from)
        .collect()
}

/// `FAIL  file:line` site lines (not the summary or error lines).
fn fail_sites(s: &str) -> BTreeSet<String> {
    lines(s, "FAIL  ")
        .into_iter()
        .filter(|l| {
            l.rsplit_once(':')
                .is_some_and(|(_, n)| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty())
        })
        .collect()
}

#[test]
fn oracle_is_the_frozen_original() {
    let t = oracle_text();
    let body = t.split_once(MARKER).expect("marker line").1;
    let h: String = Sha256::digest(body.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(h, ORACLE_BODY_SHA256, "the frozen oracle was edited");
}

#[test]
fn every_fixture_is_classified_and_there_are_enough() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    let listed: BTreeSet<String> = table().iter().map(|(c, _)| c.to_string()).collect();
    assert_eq!(on_disk, listed, "fixture directories and the table differ");
    assert!(listed.len() >= 15);
}

#[test]
fn rust_agrees_with_or_is_stricter_than_the_oracle() {
    let oracle = oracle_text();
    let mut report = Vec::new();
    for (case, kind) in table() {
        let py = run_python(case, &oracle);
        let rs = run_rust(case);
        let same = py.code == rs.code && py.stdout == rs.stdout && py.stderr == rs.stderr;
        let py_fail = fail_sites(&py.stdout);
        let rs_fail = fail_sites(&rs.stdout);
        match &kind {
            Same => assert!(
                same,
                "{case}: expected identical output\npython {} {:?} {:?}\nrust   {} {:?} {:?}",
                py.code, py.stdout, py.stderr, rs.code, rs.stdout, rs.stderr
            ),
            Stricter(_) => {
                assert!(!same, "{case}: classified Stricter but identical");
                assert!(
                    rs.code >= py.code,
                    "{case}: Rust is looser (exit {} vs {})",
                    rs.code,
                    py.code
                );
                assert!(
                    py_fail.is_subset(&rs_fail),
                    "{case}: the script fails a site Rust passes"
                );
                assert!(
                    py.code == 0 || rs_fail.len() >= py_fail.len(),
                    "{case}: Rust lists fewer failing sites"
                );
            }
            MoreSites(_) => {
                assert!(!same, "{case}: classified MoreSites but identical");
                assert_eq!(rs.code, py.code, "{case}: verdict differs");
                let ok = |o: &Out| lines(&o.stdout, "ok  ");
                assert!(
                    ok(&py).is_subset(&ok(&rs)),
                    "{case}: the script passes a site Rust fails"
                );
                assert!(
                    py_fail.is_subset(&rs_fail),
                    "{case}: the script fails a site Rust passes"
                );
            }
            Accepts(_) => {
                assert_eq!(
                    (py.code, rs.code),
                    (1, 0),
                    "{case}: classified Accepts but is not"
                );
                assert!(
                    !py_fail.is_empty(),
                    "{case}: the script must fail on a named site"
                );
                assert!(rs_fail.is_empty(), "{case}: Rust must pass every site");
            }
            Looser(_) => {
                assert_eq!(
                    (py.code, rs.code),
                    (1, 0),
                    "{case}: classified Looser but is not"
                );
            }
        }
        report.push(format!(
            "{case:26} py={} rs={} {}",
            py.code,
            rs.code,
            match &kind {
                Same => "same".to_string(),
                Stricter(w) => format!("stricter: {w}"),
                MoreSites(w) => format!("more sites: {w}"),
                Accepts(w) => format!("ACCEPTS correct guard: {w}"),
                Looser(w) => format!("LOOSER (script false positive): {w}"),
            }
        ));
    }
    eprintln!("{}", report.join("\n"));
}

/// The one class where Rust is looser. Pinned so it cannot grow unnoticed.
#[test]
fn looser_cases_are_exactly_the_documented_ones() {
    let looser: Vec<&str> = table()
        .iter()
        .filter(|(_, k)| matches!(k, Looser(_)))
        .map(|(c, _)| *c)
        .collect();
    assert_eq!(looser, vec!["block-scalar-text"]);
    // Every Accepts case is one of the six variants the owner approved.
    let accepts: BTreeSet<&str> = table()
        .iter()
        .filter(|(_, k)| matches!(k, Accepts(_)))
        .map(|(c, _)| *c)
        .collect();
    let expected: BTreeSet<&str> = [
        "hosted-fallback-latest",
        "guard-trailing-comment",
        "ordeal-comment",
        "guard-quoted",
        "spar-nested",
        "spar-nested-inner-unknown",
        "matrix-guard-varve",
        "matrix-guard-gale-json",
        "matrix-os-lean",
        "matrix-os-rocq",
        "anchor-guarded",
        "alias-runs-on",
    ]
    .into_iter()
    .collect();
    assert_eq!(accepts, expected);
}

#[test]
fn cli_matches_library_and_json_is_valid() {
    let bin = env!("CARGO_BIN_EXE_kapsel-scan");
    let root = fixtures().join("guarded");
    let o = Command::new(bin).arg("--root").arg(&root).output().unwrap();
    let rs = run_rust("guarded");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&o.stdout), rs.stdout);
    let o = Command::new(bin)
        .args(["--rule", "fork-guard", "--json", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v[0]["rule"], "fork-guard");
    assert_eq!(v[0]["passed"], true);
    let o = Command::new(bin).args(["--rule", "nope"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = Command::new(bin)
        .arg("--root")
        .arg(fixtures().join("fork-form"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
}
