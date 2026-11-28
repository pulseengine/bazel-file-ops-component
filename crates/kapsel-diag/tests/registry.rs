//! REQ-KS-016: one registry of rule ids. `kapsel_check::RULES` (KS-R) and
//! `kapsel_analysis::RULES` (KS-S) are the source of truth; the explain table
//! must list the same ids, and no source file may name a rule id outside them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn registry() -> Vec<&'static str> {
    kapsel_check::RULES
        .iter()
        .chain(kapsel_analysis::RULES.iter())
        .copied()
        .collect()
}

#[test]
fn registry_has_no_duplicates_and_the_explain_table_matches_it() {
    let reg = registry();
    let set: BTreeSet<_> = reg.iter().collect();
    assert_eq!(set.len(), reg.len(), "duplicate rule id in the registry");
    assert_eq!(
        kapsel_diag::RULE_IDS,
        reg.as_slice(),
        "kapsel_diag::RULE_IDS must be kapsel_check::RULES then kapsel_analysis::RULES"
    );
    for id in &reg {
        assert!(kapsel_diag::explain(id).is_some(), "{id}: no explanation");
    }
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            // Test directories are not shipped code.
            if name != "tests" {
                rust_sources(&p, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            out.push(p);
        }
    }
}

/// Every `"KS-Rnnn"` / `"KS-Snnn"` string literal in the shipped source of
/// every crate. A literal that is not a complete id (`"KS-R"`, `"KS-R{n}"`)
/// is returned as it is: ids must not be built at run time.
fn rule_literals() -> BTreeSet<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for c in std::fs::read_dir(&crates).unwrap() {
        let src = c.unwrap().path().join("src");
        if src.is_dir() {
            rust_sources(&src, &mut files);
        }
    }
    assert!(files.len() > 30, "found only {} source files", files.len());
    let mut found = BTreeSet::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        // Inline test modules are not shipped code.
        let text = text.split("#[cfg(test)]").next().unwrap().to_string();
        for line in text.lines() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(i) = rest.find("\"KS-") {
                let lit = &rest[i + 1..];
                let end = lit.find('"').unwrap_or(lit.len());
                found.insert((
                    f.file_name().unwrap().to_string_lossy().into_owned(),
                    lit[..end].to_string(),
                ));
                rest = &rest[(i + 2 + end).min(rest.len())..];
            }
        }
    }
    found
}

#[test]
fn every_rule_id_in_the_source_is_in_the_registry() {
    let reg = registry();
    let lits = rule_literals();
    assert!(
        lits.iter().any(|(_, l)| l == "KS-R001"),
        "the scan found no rule literal at all: {lits:?}"
    );
    for (file, lit) in &lits {
        assert!(
            reg.contains(&lit.as_str()),
            "{file}: rule id literal {lit:?} is not in kapsel_check::RULES / kapsel_analysis::RULES"
        );
    }
}
