//! Equivalence of the Rust file-operations component with the released
//! TinyGo artifact (REQ-KS-041).
//!
//! Every case in `testdata/kapsel/fileops/*.json` runs under
//! `wasmtime run` once with the oracle (`file_ops_component.wasm` from
//! kapsel v0.2.2, sha256-checked) and once with
//! `target/wasm32-wasip2/release/kapsel-fileops.wasm`, each in a fresh copy
//! of the same tree at the same host path. The test compares the files left
//! behind (path, kind, content or link target, mode), stdout, stderr and the
//! exit code. Output is normalized only for the tree and component paths
//! and for the "Time: N ms" line.
//!
//! Layouts (how the tree is preopened):
//! - `same`:     `--dir <T>::<T>`, guest paths equal host paths;
//! - `rooted`:   `--dir <T>::/w`, the tree is a top-level guest directory;
//! - `relative`: `--dir <T>::. --dir <T>::/c`, a relative `workspace_dir`
//!   (REQ-KS-017) with the config read from `/c`.
//!
//! A case may allowlist a difference for a layout by naming one of the
//! classes in `CLASSES`; the report counts each class. An allowlisted case
//! that agrees fails the test, so the list stays true. The two classes for
//! the fixed v0.2.2 same-path bug carry their own check: the Rust run must
//! equal the oracle's run of the same case under `--dir T::/w`, where the
//! oracle's walk works, and an escape must also keep the oracle's
//! same-path exit code and files.
//!
//! Run it after building the component:
//!   cargo build -p kapsel-fileops --target wasm32-wasip2 --release
//!   cargo test -p kapsel-fileops --test equivalence -- --ignored --nocapture
//! The oracle is downloaded with `gh` into the target directory, or taken
//! from `KAPSEL_FILEOPS_ORACLE_DIR`.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};

const ORACLE_TAG: &str = "v0.2.2";
const ORACLE_NAME: &str = "file_ops_component.wasm";
/// sha256 of the v0.2.2 release asset, as its .sha256 file states it.
const ORACLE_SHA256: &str = "fbf869e7075a42c804814965603c50231c93c86a65d3c35b529a942a07474a9f";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The oracle, downloaded once and refused unless its sha256 matches both
/// the downloaded .sha256 file and the pinned value.
fn oracle() -> PathBuf {
    let dir = std::env::var_os("KAPSEL_FILEOPS_ORACLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("oracle-{ORACLE_TAG}"))
        });
    let wasm = dir.join(ORACLE_NAME);
    let sum = dir.join(format!("{ORACLE_NAME}.sha256"));
    if !wasm.exists() || !sum.exists() {
        fs::create_dir_all(&dir).unwrap();
        let st = Command::new("gh")
            .args([
                "release",
                "download",
                ORACLE_TAG,
                "--repo",
                "pulseengine/kapsel",
                "--clobber",
                "-p",
            ])
            .arg(format!("{ORACLE_NAME}*"))
            .arg("--dir")
            .arg(&dir)
            .status()
            .expect("run gh to download the oracle");
        assert!(st.success(), "gh release download failed");
    }
    let stated = fs::read_to_string(&sum).unwrap();
    let stated = stated.split_whitespace().next().unwrap_or("").to_string();
    let actual = hex(&Sha256::digest(fs::read(&wasm).unwrap()));
    assert_eq!(
        actual,
        stated,
        "oracle sha256 does not match {}: refusing to run",
        sum.display()
    );
    assert_eq!(
        actual, ORACLE_SHA256,
        "oracle sha256 is not the pinned v0.2.2 value: refusing to run"
    );
    wasm
}

fn rust_component() -> PathBuf {
    let p = std::env::var_os("KAPSEL_FILEOPS_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            manifest_dir().join("../../target/wasm32-wasip2/release/kapsel-fileops.wasm")
        });
    assert!(
        p.exists(),
        "{} missing: run `cargo build -p kapsel-fileops --target wasm32-wasip2 --release` first",
        p.display()
    );
    p.canonicalize().unwrap()
}

#[derive(Debug, PartialEq)]
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
    files: BTreeMap<String, String>,
}

struct Layout {
    name: &'static str,
    w: String,
    c: String,
    preopens: Vec<String>,
}

fn layout(name: &str, t: &Path) -> Layout {
    let t = t.display().to_string();
    match name {
        "same" => Layout {
            name: "same",
            w: t.clone(),
            c: t.clone(),
            preopens: vec![format!("{t}::{t}")],
        },
        "rooted" => Layout {
            name: "rooted",
            w: "/w".into(),
            c: "/w".into(),
            preopens: vec![format!("{t}::/w")],
        },
        "relative" => Layout {
            name: "relative",
            w: ".".into(),
            c: "/c".into(),
            preopens: vec![format!("{t}::."), format!("{t}::/c")],
        },
        other => panic!("unknown layout {other}"),
    }
}

fn subst(s: &str, l: &Layout) -> String {
    s.replace("{W}", &l.w).replace("{C}", &l.c)
}

/// Lay down a case's tree at `t` (removed first).
fn build_tree(case: &Value, t: &Path, l: &Layout) {
    if t.exists() {
        fs::remove_dir_all(t).unwrap();
    }
    fs::create_dir_all(t).unwrap();
    let tree = &case["tree"];
    for d in tree["dirs"].as_array().into_iter().flatten() {
        fs::create_dir_all(t.join(d.as_str().unwrap())).unwrap();
    }
    for (p, content) in tree["files"].as_object().into_iter().flatten() {
        let path = t.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content.as_str().unwrap()).unwrap();
    }
    for (p, target) in tree["symlinks"].as_object().into_iter().flatten() {
        let path = t.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink(target.as_str().unwrap(), &path).unwrap();
    }
    let name = case["config_name"].as_str().unwrap_or("config.json");
    if let Some(text) = case["config_text"].as_str() {
        fs::write(t.join(name), subst(text, l)).unwrap();
    } else if !case["config"].is_null() {
        let text = serde_json::to_string(&case["config"]).unwrap();
        fs::write(t.join(name), subst(&text, l)).unwrap();
    }
}

/// Every entry under `t`, not following symlinks.
fn snapshot(t: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            let md = fs::symlink_metadata(&p).unwrap();
            let mode = md.permissions().mode() & 0o7777;
            if md.file_type().is_symlink() {
                out.insert(
                    rel,
                    format!("symlink -> {}", fs::read_link(&p).unwrap().display()),
                );
            } else if md.is_dir() {
                out.insert(rel, format!("dir {mode:o}"));
                walk(root, &p, out);
            } else {
                let content = fs::read(&p).unwrap();
                out.insert(
                    rel,
                    format!("file {mode:o} {:?}", String::from_utf8_lossy(&content)),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(t, t, &mut out);
    out
}

fn normalize(s: &str, t: &Path, component: &Path) -> String {
    let s = s
        .replace(&component.display().to_string(), "<component>")
        .replace(&t.display().to_string(), "<T>");
    s.lines()
        .map(|line| match line.strip_prefix("  Time: ") {
            Some(rest) if rest.ends_with(" ms") => "  Time: <ms> ms".to_string(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_one(component: &Path, case: &Value, t: &Path, l: &Layout) -> Run {
    build_tree(case, t, l);
    let args: Vec<String> = match case["args"].as_array() {
        Some(a) => a.iter().map(|v| subst(v.as_str().unwrap(), l)).collect(),
        None => vec![subst("{C}/config.json", l)],
    };
    let mut cmd = Command::new("wasmtime");
    // The wasmtime on PATH is a varve shim pinned by this repository.
    cmd.current_dir(manifest_dir()).arg("run");
    for p in &l.preopens {
        cmd.arg("--dir").arg(p);
    }
    if case["dev"].as_bool() == Some(true) {
        cmd.arg("--dir").arg("/dev::/dev");
    }
    cmd.arg(component).args(&args);
    let out = cmd.output().expect("run wasmtime");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: normalize(&String::from_utf8_lossy(&out.stdout), t, component),
        stderr: normalize(&String::from_utf8_lossy(&out.stderr), t, component),
        files: snapshot(t),
    }
}

fn describe_diff(a: &Run, b: &Run) -> String {
    let mut d = String::new();
    if a.code != b.code {
        d += &format!("    exit: oracle {} rust {}\n", a.code, b.code);
    }
    if a.stdout != b.stdout {
        d += &format!(
            "    stdout:\n      oracle {:?}\n      rust   {:?}\n",
            a.stdout, b.stdout
        );
    }
    if a.stderr != b.stderr {
        d += &format!(
            "    stderr:\n      oracle {:?}\n      rust   {:?}\n",
            a.stderr, b.stderr
        );
    }
    for k in a
        .files
        .keys()
        .chain(b.files.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        if a.files.get(k) != b.files.get(k) {
            d += &format!(
                "    file {k}: oracle {:?} rust {:?}\n",
                a.files.get(k),
                b.files.get(k)
            );
        }
    }
    d
}

/// Checks on the Rust run alone, so an allowlisted case still proves what
/// the Rust side must do.
fn check_expectations(case: &Value, r: &Run) -> Vec<String> {
    let mut bad = Vec::new();
    let e = &case["expect_rust"];
    if let Some(code) = e["code"].as_i64()
        && i64::from(r.code) != code
    {
        bad.push(format!("rust exit {} != expected {code}", r.code));
    }
    for p in e["exists"].as_array().into_iter().flatten() {
        if !r.files.contains_key(p.as_str().unwrap()) {
            bad.push(format!("rust left no {}", p.as_str().unwrap()));
        }
    }
    for p in e["absent"].as_array().into_iter().flatten() {
        if r.files.contains_key(p.as_str().unwrap()) {
            bad.push(format!("rust left {}", p.as_str().unwrap()));
        }
    }
    if let Some(s) = e["stderr_contains"].as_str()
        && !r.stderr.contains(s)
    {
        bad.push(format!("rust stderr lacks {s:?}: {:?}", r.stderr));
    }
    bad
}

fn load_corpus() -> Vec<(String, Value)> {
    let dir = manifest_dir().join("../../testdata/kapsel/fileops");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let mut cases = Vec::new();
    for f in files
        .into_iter()
        .filter(|f| f.extension().is_some_and(|e| e == "json"))
    {
        let group = f.file_stem().unwrap().to_string_lossy().into_owned();
        let v: Value = serde_json::from_str(&fs::read_to_string(&f).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for c in v.as_array().unwrap() {
            cases.push((
                format!("{group}/{}", c["name"].as_str().unwrap()),
                c.clone(),
            ));
        }
    }
    cases
}

const FIXED_SAME_PATH: &str = "fixed-v0.2.2-same-path";
const FIXED_SAME_PATH_REFUSAL: &str = "fixed-v0.2.2-same-path-refusal";

/// The allowlist classes a case may name for a layout, with the reason.
const CLASSES: &[(&str, &str)] = &[
    (
        "req-ks-017",
        "REQ-KS-017: a relative workspace_dir is accepted; TinyGo refuses it before running",
    ),
    (
        FIXED_SAME_PATH,
        "fixed v0.2.2 bug: in-workspace write under same-path preopen (owner 2026-11-28); \
         checked to equal the oracle's run of the same case under --dir T::/w",
    ),
    (
        FIXED_SAME_PATH_REFUSAL,
        "fixed v0.2.2 bug, escape still refused: v0.2.2 refused at its broken first check, the \
         fix refuses at the rule that applies; checked to give the oracle's same-path exit code \
         and files, and to equal the oracle's run under --dir T::/w",
    ),
    (
        "src-paths-alias",
        "src_paths alias (owner 2026-11-28): TinyGo ignores src_paths and fails with \
         \"requires sources\"; giving both is an error naming both fields",
    ),
    (
        "oracle-crash",
        "the TinyGo component crashes (panic: slice out of range in os.ReadFile, exit 134); \
         the Rust component reports an error and exits 1",
    ),
];

/// Replace the guest root `root` with `<T>` where it stands as a whole path
/// prefix ("/w" in "/w/ws" or "/w:", not in "/ws").
fn rebase(s: &str, root: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(root) {
        let after = &rest[i + root.len()..];
        let boundary = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'));
        out.push_str(&rest[..i]);
        out.push_str(if boundary { "<T>" } else { root });
        rest = after;
    }
    out.push_str(rest);
    out
}

fn rebase_run(r: &Run, root: &str) -> Run {
    Run {
        code: r.code,
        stdout: rebase(&r.stdout, root),
        stderr: rebase(&r.stderr, root),
        files: r
            .files
            .iter()
            .map(|(k, v)| (k.clone(), rebase(v, root)))
            .collect(),
    }
}

/// A fixed same-path run must do exactly what the oracle does with the same
/// case when the tree is a top-level guest directory, where v0.2.2's walk
/// works: same files, output and exit code, escapes refused alike.
fn check_fixed_same_path(
    oracle: &Path,
    case: &Value,
    scratch: &Path,
    name: &str,
    t_same: &Path,
    rust_same: &Run,
) -> Option<String> {
    let t = scratch.join(name.replace('/', "-")).join("cross-rooted");
    let l = layout("rooted", &t);
    let mut reference = rebase_run(&run_one(oracle, case, &t, &l), "/w");
    let _ = fs::remove_dir_all(&t);
    let mut ours = rebase_run(rust_same, &t_same.display().to_string());
    // The config file holds each layout's own paths; it is input, not output.
    let config = case["config_name"].as_str().unwrap_or("config.json");
    reference.files.remove(config);
    ours.files.remove(config);
    (ours != reference).then(|| {
        format!(
            "fixed same-path run differs from the oracle's rooted run\n{}",
            describe_diff(&reference, &ours)
        )
    })
}

#[test]
#[ignore = "needs wasmtime, gh (or KAPSEL_FILEOPS_ORACLE_DIR) and the wasm32-wasip2 build"]
fn rust_component_matches_tinygo_oracle() {
    let oracle = oracle();
    let rust = rust_component();
    let cases = load_corpus();
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fileops-eq");
    fs::create_dir_all(&scratch).unwrap();
    let scratch = scratch.canonicalize().unwrap();

    let (mut runs, mut agree, mut allowed) = (0, 0, 0);
    let mut per_layout: BTreeMap<&str, (u32, u32, u32)> = BTreeMap::new();
    let mut per_class: BTreeMap<&str, u32> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut allow_lines = Vec::new();
    for (name, case) in &cases {
        let layouts: Vec<String> = match case["layouts"].as_array() {
            Some(a) => a.iter().map(|v| v.as_str().unwrap().to_string()).collect(),
            None => vec!["same".into(), "rooted".into()],
        };
        for ln in &layouts {
            let t = scratch.join(name.replace('/', "-")).join(ln);
            let l = layout(ln, &t);
            let a = run_one(&oracle, case, &t, &l);
            let b = run_one(&rust, case, &t, &l);
            let _ = fs::remove_dir_all(&t);
            runs += 1;
            let entry = per_layout.entry(l.name).or_default();
            entry.0 += 1;
            let class = case["allow"][ln.as_str()]
                .as_str()
                .or_else(|| case["allow"]["*"].as_str())
                .map(|id| {
                    CLASSES
                        .iter()
                        .find(|(c, _)| *c == id)
                        .unwrap_or_else(|| panic!("{name}: unknown allowlist class {id}"))
                });
            for bad in check_expectations(case, &b) {
                failures.push(format!("{name} [{ln}]: {bad}"));
            }
            if let Some((id, _)) = class
                && (*id == FIXED_SAME_PATH || *id == FIXED_SAME_PATH_REFUSAL)
            {
                if ln != "same" {
                    failures.push(format!(
                        "{name} [{ln}]: {id} applies to the same layout only"
                    ));
                }
                if let Some(bad) = check_fixed_same_path(&oracle, case, &scratch, name, &t, &b) {
                    failures.push(format!("{name} [{ln}]: {bad}"));
                }
                if *id == FIXED_SAME_PATH_REFUSAL
                    && (a.code != b.code || a.code == 0 || a.files != b.files)
                {
                    failures.push(format!(
                        "{name} [{ln}]: an escape must stay refused with the oracle's exit code and files\n{}",
                        describe_diff(&a, &b)
                    ));
                }
            }
            match (a == b, class) {
                (true, None) => {
                    agree += 1;
                    entry.1 += 1;
                }
                (false, Some((id, reason))) => {
                    allowed += 1;
                    entry.2 += 1;
                    *per_class.entry(id).or_default() += 1;
                    allow_lines.push(format!(
                        "  [{id}] {name} [{ln}]: {reason}\n{}",
                        describe_diff(&a, &b)
                    ));
                }
                (true, Some(_)) => {
                    failures.push(format!("{name} [{ln}]: allowlisted but the outputs agree"))
                }
                (false, None) => {
                    failures.push(format!("{name} [{ln}]: differs\n{}", describe_diff(&a, &b)))
                }
            }
        }
    }

    let mut report = format!(
        "fileops equivalence: {} cases, {runs} runs, {agree} agree, {allowed} allowlisted, {} disagree\n",
        cases.len(),
        failures.len()
    );
    for (l, (n, a, al)) in &per_layout {
        report += &format!("  layout {l}: {n} runs, {a} agree, {al} allowlisted\n");
    }
    for (id, reason) in CLASSES {
        report += &format!(
            "  class {id}: {} runs ({reason})\n",
            per_class.get(id).copied().unwrap_or(0)
        );
    }
    report += "allowlisted differences:\n";
    for l in &allow_lines {
        report += l;
    }
    for f in &failures {
        report += &format!("FAIL {f}\n");
    }
    println!("{report}");
    fs::write(scratch.join("report.txt"), &report).unwrap();
    assert!(
        cases.len() >= 40,
        "the corpus needs at least 40 cases, has {}",
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "{} disagreements:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
