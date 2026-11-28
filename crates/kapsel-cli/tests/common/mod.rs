//! Helpers shared by the integration tests that read `testdata/kapsel/`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Files directly under `dir` with the extension `ext`, sorted.
pub fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .collect();
    v.sort();
    v
}

/// Every plant-pack script (`planted.sh`, `control.sh` of every rule).
pub fn rule_scripts() -> Vec<PathBuf> {
    let root = repo_root().join("testdata/kapsel/rules");
    let mut v = Vec::new();
    for id in kapsel_check::RULES {
        for f in ["planted.sh", "control.sh"] {
            v.push(root.join(id).join(f));
        }
    }
    v
}

/// The parse goldens' inputs (accepted and refused).
pub fn parse_scripts() -> Vec<PathBuf> {
    let root = repo_root().join("testdata/kapsel/parse");
    let mut v = files_with_ext(&root.join("accept"), "sh");
    v.extend(files_with_ext(&root.join("refuse"), "sh"));
    v
}

/// The error corpus.
pub fn error_scripts() -> Vec<PathBuf> {
    files_with_ext(&repo_root().join("testdata/kapsel/errors/ci"), "sh")
}

/// The dash corpus.
pub fn corpus_scripts() -> Vec<PathBuf> {
    files_with_ext(&repo_root().join("testdata/kapsel/corpus"), "sh")
}

/// Plant pack, parse goldens and error corpus: the inputs `kapsel check`
/// must handle without touching anything.
pub fn check_inputs() -> Vec<PathBuf> {
    let mut v = rule_scripts();
    v.extend(parse_scripts());
    v.extend(error_scripts());
    v
}
