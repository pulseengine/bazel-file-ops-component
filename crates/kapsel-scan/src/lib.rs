//! kapsel-scan: an offline scanner over a repository's `.github/workflows`.
//!
//! Rule 1 is `fork-guard`, the Rust port of the former
//! `scripts/check_selfhosted_fork_guard.py` (REQ-KS-015, REQ-KS-064). The
//! workflow structure is read with rivet-yaml (DD-KS-014). A `runs-on` that
//! can select `self-hosted` must not be able to do so for a fork PR: the
//! `${{ }}` expression is evaluated with `event_name == 'pull_request'`, a head
//! repository that differs from this one, and `strategy.matrix` values
//! enumerated (see [`expr`]). Anything not understood fails.
//!
//! Against the old script this is stricter where the script's line regex is
//! blind and wider where a guard is semantically correct but not byte-equal
//! to the one text form; see `tests/equivalence.rs` for the classified table.

pub mod expr;
pub mod runs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rivet_yaml::Value;
use rivet_yaml::cst::{SyntaxKind, SyntaxNode, parse as cst_parse};

use crate::expr::{
    Template, V, eval, hosted_label, matrix_keys, parse, selects_non_hosted, template,
};

pub const RULES: &[&str] = &["fork-guard", "run-blocks"];

/// Rules run when none is named: `run-blocks` is advisory (Phase 0) and opt-in.
pub const DEFAULT_RULES: &[&str] = &["fork-guard"];

/// One self-hosted `runs-on` found in a workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub line: usize,
    pub ok: bool,
    /// Why it failed (JSON output only; the text output keeps the script's format).
    pub reason: Option<String>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub rule: &'static str,
    pub sites: Vec<Site>,
    /// Files that could not be read or parsed: always a failure (fail closed).
    pub errors: Vec<String>,
}

impl Report {
    pub fn bad(&self) -> usize {
        self.sites.iter().filter(|s| !s.ok).count()
    }

    pub fn passed(&self) -> bool {
        !self.sites.is_empty() && self.bad() == 0 && self.errors.is_empty()
    }

    /// Text in the old script's format: `(stdout, stderr, exit code)`.
    pub fn render(&self) -> (String, String, i32) {
        let mut out = String::new();
        for s in &self.sites {
            let tag = if s.ok { "ok  " } else { "FAIL" };
            out.push_str(&format!("{tag}  {}:{}\n", s.file, s.line));
        }
        let mut err = String::new();
        for e in &self.errors {
            err.push_str(&format!("FAIL  {e}\n"));
        }
        if self.sites.is_empty() && self.errors.is_empty() {
            err.push_str("FAIL  no self-hosted job found: this check compared nothing\n");
            return (out, err, 1);
        }
        if self.bad() > 0 {
            err.push_str(&format!(
                "FAIL  {} self-hosted job(s) without the fail-closed fork guard, \
                 or in a pull_request_target workflow\n",
                self.bad()
            ));
            return (out, err, 1);
        }
        if !self.errors.is_empty() {
            return (out, err, 1);
        }
        out.push_str(&format!(
            "{} self-hosted job(s), each with the fail-closed fork guard\n",
            self.sites.len()
        ));
        (out, err, 0)
    }
}

/// Workflow files the script globbed: `*.y*ml` in `.github/workflows`.
pub(crate) fn workflow_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(".github").join("workflows");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    n.match_indices(".y")
                        .any(|(i, _)| i + 2 <= n.len().saturating_sub(2))
                        && n.ends_with("ml")
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    v
}

fn mentions(v: &Value, needle: &str) -> bool {
    match v {
        Value::String(s) => s.contains(needle),
        Value::Sequence(s) => s.iter().any(|x| mentions(x, needle)),
        Value::Mapping(m) => m
            .iter()
            .any(|(k, x)| mentions(k, needle) || mentions(x, needle)),
        Value::Tagged(t) => mentions(&t.value, needle),
        _ => false,
    }
}

/// `runs-on` keys in document order, with the sibling `strategy.matrix` if any.
fn collect<'a>(v: &'a Value, out: &mut Vec<(&'a Value, Option<&'a Value>)>) {
    match v {
        Value::Mapping(m) => {
            let matrix = m.get("strategy").and_then(|s| s.get("matrix"));
            for (k, x) in m.iter() {
                if k.as_str() == Some("runs-on") {
                    out.push((x, matrix));
                }
                collect(x, out);
            }
        }
        Value::Sequence(s) => s.iter().for_each(|x| collect(x, out)),
        Value::Tagged(t) => collect(&t.value, out),
        _ => {}
    }
}

/// Can this `runs-on` select a self-hosted runner? Substring test, like the
/// script, but over the whole value (list items, label maps) and, for a
/// `${{ matrix.x }}` selector, over the sibling matrix.
fn selects_self_hosted(on: &Value, matrix: Option<&Value>) -> bool {
    if mentions(on, "self-hosted") {
        return true;
    }
    mentions(on, "matrix") && matrix.is_some_and(|m| mentions(m, "self-hosted"))
}

/// Keys named `runs-on` in the CST, in document order, with their line.
fn cst_sites(src: &str) -> Vec<usize> {
    let (green, _) = cst_parse(src);
    let root = SyntaxNode::new_root(green);
    let mut out = Vec::new();
    for node in root.descendants() {
        if node.kind() != SyntaxKind::MappingEntry {
            continue;
        }
        let Some(key) = node.children().find(|c| c.kind() == SyntaxKind::Key) else {
            continue;
        };
        let k = key.text().to_string();
        if k.trim().trim_matches(|c| c == '"' || c == '\'') != "runs-on" {
            continue;
        }
        let off: usize = key.text_range().start().into();
        out.push(src[..off.min(src.len())].matches('\n').count() + 1);
    }
    out
}

fn to_v(v: &Value) -> V {
    match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::String(s) => V::Str(s.clone()),
        Value::Sequence(s) => V::Arr(s.iter().map(to_v).collect()),
        Value::Number(n) => V::Str(n.to_string()),
        _ => V::Unknown,
    }
}

/// Every value `matrix.<key>` can take: the listed values plus `include`s.
fn matrix_values(matrix: Option<&Value>, key: &str) -> Option<Vec<V>> {
    let m = matrix?.as_mapping()?;
    let mut out = Vec::new();
    for (k, x) in m.iter() {
        match k.as_str() {
            Some("include") => {
                for inc in x.as_sequence()? {
                    for (k2, x2) in inc.as_mapping()?.iter() {
                        if k2.as_str().is_some_and(|n| n.eq_ignore_ascii_case(key)) {
                            out.push(to_v(x2));
                        }
                    }
                }
            }
            Some("exclude") => {}
            Some(n) if n.eq_ignore_ascii_case(key) => match x {
                Value::Sequence(s) => out.extend(s.iter().map(to_v)),
                // a `${{ fromJSON(..) }}` matrix: not enumerable
                _ => return None,
            },
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Can this `runs-on` land on a non-hosted runner for a fork PR? `Err` carries
/// the reason (fail closed: anything not understood is a failure).
fn judge(on: &Value, matrix: Option<&Value>) -> Result<(), String> {
    let src = match on {
        Value::String(s) => s,
        Value::Sequence(items) => {
            let labels: Vec<V> = items
                .iter()
                .map(|i| match i.as_str() {
                    Some(s) if !s.contains("${{") => V::Str(s.to_string()),
                    _ => V::Unknown,
                })
                .collect();
            return if selects_non_hosted(&V::Arr(labels)) {
                Err("a label list selects self-hosted runners with no guard".into())
            } else {
                Ok(())
            };
        }
        _ => return Err("runs-on is a mapping; its labels cannot be checked".into()),
    };
    let expr = match template(src)? {
        Template::Plain(s) => {
            return if hosted_label(&s) {
                Ok(())
            } else {
                Err(format!(
                    "`{s}` is not a GitHub-hosted label and has no guard"
                ))
            };
        }
        Template::Expr(e) => parse(&e)?,
    };
    let mut keys = Vec::new();
    matrix_keys(&expr, &mut keys);
    let mut domains: Vec<(String, Vec<V>)> = Vec::new();
    for k in keys {
        let vals =
            matrix_values(matrix, &k).ok_or_else(|| format!("matrix.{k} cannot be enumerated"))?;
        domains.push((k, vals));
    }
    let total: usize = domains.iter().map(|(_, v)| v.len()).product();
    if total > 4096 {
        return Err("matrix too large to enumerate".into());
    }
    for n in 0..total {
        let mut rest = n;
        let mut env = HashMap::new();
        let mut desc = Vec::new();
        for (k, vals) in &domains {
            let v = vals[rest % vals.len()].clone();
            rest /= vals.len();
            desc.push(format!("matrix.{k}={v:?}"));
            env.insert(k.clone(), v);
        }
        if eval(&expr, &env).iter().any(selects_non_hosted) {
            return Err(format!(
                "a fork PR can select a non-hosted runner{}{}",
                if desc.is_empty() { "" } else { " when " },
                desc.join(", ")
            ));
        }
    }
    Ok(())
}

/// Scan one workflow file's text. Appends to `rep`.
pub fn fork_guard_file(name: &str, src: &str, rep: &mut Report) {
    let doc: Value = match rivet_yaml::from_str(src) {
        Ok(d) => d,
        Err(e) => {
            rep.errors
                .push(format!("{name}: cannot read as one YAML document: {e}"));
            return;
        }
    };
    // Conservative like the script: any mention of the trigger anywhere in
    // the parsed document counts (comments are not part of the parse).
    let prt = mentions(&doc, "pull_request_target");
    let mut found = Vec::new();
    collect(&doc, &mut found);
    let cst = cst_sites(src);
    if cst.len() != found.len() {
        // Flow mappings, anchored mappings holding a runs-on, anything that
        // breaks the pairing: fail closed rather than guess a line.
        rep.errors.push(format!(
            "{name}: {} runs-on key(s) in the text but {} in the parsed value; cannot pair them",
            cst.len(),
            found.len()
        ));
        return;
    }
    for ((on, matrix), line) in found.into_iter().zip(cst) {
        if !selects_self_hosted(on, matrix) {
            continue;
        }
        let verdict = if prt {
            Err("the workflow triggers on pull_request_target".to_string())
        } else {
            judge(on, matrix)
        };
        rep.sites.push(Site {
            file: name.to_string(),
            line,
            ok: verdict.is_ok(),
            reason: verdict.err(),
        });
    }
}

/// Rule `fork-guard` over `<root>/.github/workflows`.
pub fn fork_guard(root: &Path) -> Report {
    let mut rep = Report {
        rule: "fork-guard",
        ..Report::default()
    };
    for p in workflow_files(root) {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        match std::fs::read_to_string(&p) {
            Ok(src) => fork_guard_file(&name, &src, &mut rep),
            Err(e) => rep.errors.push(format!("{name}: {e}")),
        }
    }
    rep
}

pub fn run_rule(rule: &str, root: &Path) -> Option<Report> {
    match rule {
        "fork-guard" => Some(fork_guard(root)),
        _ => None,
    }
}
