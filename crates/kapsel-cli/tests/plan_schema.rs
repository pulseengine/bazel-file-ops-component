//! REQ-KS-013: every plan and refusal `kapsel check --json` produces validates
//! against the published schema, `schemas/kapsel.plan.v0.json` (generated from
//! the Rust types and kept in step by the kapsel-plan `schema` test).

mod common;

use assert_cmd::Command;
use serde_json::{Value, json};

fn schema() -> Value {
    let p = common::repo_root().join("schemas/kapsel.plan.v0.json");
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

fn check_json(arg: &[&str]) -> (Option<i32>, Value) {
    let scratch = tempfile::tempdir().unwrap();
    let o = Command::cargo_bin("kapsel")
        .unwrap()
        .current_dir(scratch.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("KAPSEL_HOME", scratch.path().join("home"))
        .arg("check")
        .arg("--json")
        .args(arg)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    let v: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{arg:?}: stdout is not JSON ({e}): {text}"));
    (o.status.code(), v)
}

fn errors(validator: &jsonschema::Validator, instance: &Value) -> Vec<String> {
    validator
        .iter_errors(instance)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect()
}

#[test]
fn every_plan_and_refusal_validates_against_the_published_schema() {
    let validator = jsonschema::validator_for(&schema()).expect("the schema compiles");
    let mut inputs = common::check_inputs();
    inputs.extend(common::corpus_scripts());
    let (mut plans, mut refusals, mut with_findings) = (0, 0, 0);
    for f in &inputs {
        let (code, doc) = check_json(&[f.to_str().unwrap()]);
        let bad = errors(&validator, &doc);
        assert!(
            bad.is_empty(),
            "{}: does not validate against schemas/kapsel.plan.v0.json:\n{}",
            f.display(),
            bad.join("\n")
        );
        assert_eq!(doc["schema"], "kapsel.plan.v0", "{}", f.display());
        match doc["outcome"].as_str() {
            Some("plan") => {
                assert_eq!(code, Some(0), "{}", f.display());
                plans += 1;
                if !doc["warnings"].as_array().unwrap().is_empty() {
                    with_findings += 1;
                }
            }
            Some("refused") => {
                assert_eq!(code, Some(65), "{}", f.display());
                assert!(!doc["findings"].as_array().unwrap().is_empty());
                refusals += 1;
            }
            other => panic!("{}: outcome {other:?}", f.display()),
        }
    }
    assert!(plans > 80, "{plans} plans");
    assert!(refusals > 50, "{refusals} refusals");
    assert!(with_findings > 10, "{with_findings} plans with warnings");
    // `-c` gives the same document shape.
    let (_, doc) = check_json(&["-c", "cat a | sort -u > b"]);
    assert!(errors(&validator, &doc).is_empty());
    assert_eq!(doc["grants"]["read"], json!(["a"]));
    assert_eq!(doc["grants"]["write"], json!(["b"]));
}

#[test]
fn the_schema_rejects_documents_that_break_the_contract() {
    // The validation above must be able to fail.
    let validator = jsonschema::validator_for(&schema()).unwrap();
    let (_, plan) = check_json(&["-c", "cat a | sort -u > b"]);
    let (_, refusal) = check_json(&["-c", "false || true"]);
    assert!(errors(&validator, &plan).is_empty());
    assert!(errors(&validator, &refusal).is_empty());
    let drop = |mut v: Value, path: &[&str]| {
        let (last, parents) = path.split_last().unwrap();
        let mut cur = &mut v;
        for p in parents {
            cur = cur.get_mut(*p).unwrap();
        }
        cur.as_object_mut().unwrap().remove(*last).unwrap();
        v
    };
    for (doc, path) in [
        (plan.clone(), vec!["grants"]),
        (plan.clone(), vec!["grants", "write"]),
        (plan.clone(), vec!["body"]),
        (plan.clone(), vec!["source_sha256"]),
        (refusal.clone(), vec!["findings"]),
        (refusal.clone(), vec!["schema"]),
    ] {
        let broken = drop(doc, &path);
        assert!(
            !errors(&validator, &broken).is_empty(),
            "a document without {path:?} still validates"
        );
    }
    let mut wrong = refusal.clone();
    wrong["findings"][0]["severity"] = json!("fatal");
    assert!(!errors(&validator, &wrong).is_empty(), "unknown severity");
    let mut wrong = refusal;
    wrong["findings"][0]["rule"] = json!(7);
    assert!(
        !errors(&validator, &wrong).is_empty(),
        "rule must be a string"
    );
    let mut wrong = plan;
    wrong["outcome"] = json!("maybe");
    assert!(!errors(&validator, &wrong).is_empty(), "unknown outcome");
}
