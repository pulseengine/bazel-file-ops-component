//! REQ-KS-062: the regression corpus holds no private data, and the `run-error`
//! cases fail the way their expect lines say when run through the real binary.

mod common;

use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn all_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            all_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Longest runs of chars satisfying `f`, as (start, text).
fn runs(text: &str, f: impl Fn(char) -> bool) -> Vec<&str> {
    let mut v = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (f(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                v.push(&text[s..i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        v.push(&text[s..]);
    }
    v
}

/// What is private about `text`, one entry per hit. Empty means clean.
fn privacy_hits(text: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for p in ["/Users/", "/home/", "/Volumes/"] {
        if text.contains(p) {
            hits.push(format!("home path {p}"));
        }
    }
    // Host names ending in .local, and email addresses.
    for w in runs(text, |c| is_word(c) || "-.@+".contains(c)) {
        let w = w.trim_end_matches('.');
        if w.ends_with(".local") {
            hits.push(format!("host name {w}"));
        }
        if let Some((user, host)) = w.split_once('@')
            && !user.is_empty()
            && host.contains('.')
        {
            hits.push(format!("email address {w}"));
        }
    }
    // Tokens: a prefix at a word boundary.
    let b = text.as_bytes();
    for (prefix, min_tail) in [("ghp_", 1usize), ("github_pat_", 1), ("sk-", 8)] {
        for (i, _) in text.match_indices(prefix) {
            let boundary = i == 0 || !is_word(b[i - 1] as char);
            let tail = text[i + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if boundary && tail >= min_tail {
                hits.push(format!("token starting {prefix}"));
            }
        }
    }
    // Long hex runs and long base64-looking runs.
    for r in runs(text, |c| c.is_ascii_hexdigit()) {
        if r.len() >= 32 {
            hits.push(format!("hex run of {}", r.len()));
        }
    }
    for r in runs(text, |c| c.is_ascii_alphanumeric() || c == '+' || c == '/') {
        let mixed = r.chars().any(|c| c.is_ascii_uppercase())
            && r.chars().any(|c| c.is_ascii_lowercase())
            && r.chars().any(|c| c.is_ascii_digit());
        if r.len() >= 40 && mixed {
            hits.push(format!("base64-like run of {}", r.len()));
        }
    }
    // Private organisations: any GitHub owner but pulseengine.
    for (i, _) in text.match_indices("github.com") {
        let rest = &text[i + "github.com".len()..];
        let rest = rest.trim_start_matches([':', '/']);
        let owner: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !owner.is_empty() && !owner.eq_ignore_ascii_case("pulseengine") {
            hits.push(format!("organisation {owner}"));
        }
    }
    hits
}

#[test]
fn the_scanner_detects_each_class_and_passes_clean_text() {
    // The scan must be able to fail.
    for (text, what) in [
        ("cd /Users/someone/x", "home path"),
        ("cd /home/someone/x", "home path"),
        ("ls /Volumes/Disk", "home path"),
        ("ssh build.local", "host name"),
        ("mail me@example.com", "email address"),
        ("TOKEN=ghp_abcdef", "token"),
        ("TOKEN=github_pat_abc", "token"),
        ("KEY=sk-abcdefghijkl", "token"),
        ("sha=0123456789abcdef0123456789abcdef", "hex run"),
        (
            "k=QUJDREVGR0hJSjEyMzQ1Njc4OWFiY2RlZmdoaWprbG1ub3A",
            "base64",
        ),
        (
            "git clone https://github.com/acme-private/x",
            "organisation acme-private",
        ),
        (
            "git clone git@github.com:acme-private/x",
            "organisation acme-private",
        ),
    ] {
        let hits = privacy_hits(text);
        assert!(
            hits.iter().any(|h| h.contains(what)),
            "{text:?}: want {what}, got {hits:?}"
        );
    }
    for clean in [
        "grep -rn foo src | sort -u > out.txt",
        "curl https://github.com/pulseengine/kapsel/issues?since=1",
        "echo disk-usage tasks-list sk-",
        "# expect: refuse KS-R001",
        "testdata/kapsel/errors/ci/001-or-echo-swallows-build.sh",
    ] {
        assert_eq!(privacy_hits(clean), Vec::<String>::new(), "{clean:?}");
    }
}

#[test]
fn no_committed_corpus_file_holds_a_home_path_host_name_user_name_or_token() {
    let root = common::repo_root().join("testdata/kapsel");
    let mut files = Vec::new();
    all_files(&root.join("errors"), &mut files);
    all_files(&root.join("corpus"), &mut files);
    assert!(files.len() >= 55, "only {} files scanned", files.len());
    let mut bad = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for h in privacy_hits(&text) {
            bad.push(format!("{}: {h}", f.strip_prefix(&root).unwrap().display()));
        }
    }
    assert!(
        bad.is_empty(),
        "private data in the corpus:\n{}",
        bad.join("\n")
    );
}

/// `# expect: run-error exit N | MESSAGE`: the outcome the real binary gives.
fn parse_run_error(first_line: &str) -> Option<(i32, String)> {
    let rest = first_line.strip_prefix("# expect: run-error exit ")?;
    let (code, msg) = rest.split_once(" | ")?;
    Some((code.trim().parse().unwrap(), msg.trim().to_string()))
}

#[test]
fn run_error_cases_fail_through_the_binary_with_the_expected_code_and_message() {
    let mut n = 0;
    for f in common::error_scripts() {
        let text = std::fs::read_to_string(&f).unwrap();
        let first = text.lines().next().unwrap();
        if !first.starts_with("# expect: run-error") {
            continue;
        }
        let (want_code, want_msg) = parse_run_error(first).unwrap_or_else(|| {
            panic!(
                "{}: a run-error expect line reads `run-error exit N | MESSAGE`",
                f.display()
            )
        });
        n += 1;
        // An empty scratch directory: the inputs the cases name do not exist.
        let scratch = tempfile::tempdir().unwrap();
        let o = Command::cargo_bin("kapsel")
            .unwrap()
            .current_dir(scratch.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("KAPSEL_HOME", scratch.path().join("home"))
            .arg(&f)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(want_code), "{}: {err}", f.display());
        assert!(
            err.contains(&want_msg),
            "{}: stderr lacks {want_msg:?}: {err}",
            f.display()
        );
        assert!(
            !err.contains("error[KS-R"),
            "{}: a run-error case must pass the check stage: {err}",
            f.display()
        );
        // Nothing but the home was created by the failed run.
        let names: Vec<_> = std::fs::read_dir(scratch.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| n == "home"),
            "{}: wrote {names:?}",
            f.display()
        );
    }
    assert_eq!(n, 3, "the corpus has 3 run-error cases");
}
