//! REQ-KS-013: the published plan schema is generated from the Rust types.
//!
//! `schemas/kapsel.plan.v0.json` is the JSON Schema of `CheckOutcome`
//! (`Plan`, `Refusal`, `Finding` and what they contain). This test regenerates
//! it and fails if the committed file differs. Re-bless with
//! `KAPSEL_BLESS=1 cargo test -p kapsel-plan --test schema`.

use std::path::Path;

use kapsel_plan::CheckOutcome;

/// The same value with every object's keys in sorted order, so the document
/// does not depend on whether serde_json's `preserve_order` is enabled by
/// another crate in the build.
fn sorted(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            let mut entries: Vec<_> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The schema document, exactly as committed (pretty JSON, trailing newline).
fn generate() -> String {
    let schema = schemars::schema_for!(CheckOutcome);
    let mut v = serde_json::to_value(&schema).unwrap();
    let o = v.as_object_mut().unwrap();
    o.insert(
        "title".into(),
        "kapsel.plan.v0: the outcome of `kapsel check`".into(),
    );
    o.insert(
        "description".into(),
        "A plan or a refusal. `kapsel check --json` adds one top-level `diagnostics` \
         field (kapsel.diag.v0), which this schema permits. The `schema` field of a \
         plan or refusal is \"kapsel.plan.v0\"."
            .into(),
    );
    let mut s = serde_json::to_string_pretty(&sorted(v)).unwrap();
    s.push('\n');
    s
}

fn committed_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/kapsel.plan.v0.json")
}

#[test]
fn committed_schema_matches_the_rust_types() {
    let want = generate();
    let path = committed_path();
    if std::env::var_os("KAPSEL_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; generate it with KAPSEL_BLESS=1 cargo test -p kapsel-plan --test schema",
            path.display()
        )
    });
    assert!(
        have == want,
        "schemas/kapsel.plan.v0.json differs from the types in kapsel-plan; if the change is \
         intended (it is a contract change), re-bless with KAPSEL_BLESS=1 and review the diff"
    );
}

#[test]
fn schema_names_the_types_and_the_version_field() {
    let v: serde_json::Value = serde_json::from_str(&generate()).unwrap();
    let text = v.to_string();
    for needle in [
        "Plan",
        "Refusal",
        "Finding",
        "Severity",
        "Grants",
        "schema",
        "source_sha256",
        "rule",
        "excerpt",
    ] {
        assert!(text.contains(needle), "the schema never mentions {needle}");
    }
    assert_eq!(kapsel_plan::PLAN_SCHEMA, "kapsel.plan.v0");
}
