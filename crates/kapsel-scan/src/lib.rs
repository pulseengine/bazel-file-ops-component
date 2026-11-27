//! kapsel-scan: an offline scanner over a repository's `.github/workflows`.
//!
//! Rule 1 is `fork-guard`, the Rust port of the former
//! `scripts/check_selfhosted_fork_guard.py` (REQ-KS-015, REQ-KS-064). The
//! workflow structure is read with rivet-yaml (DD-KS-014); `${{ ... }}`
//! expressions are opaque strings and are never evaluated.
//!
//! The port is stricter than the script, never looser: wherever the script's
//! line regex and a YAML-aware reading disagree, the verdict is the stricter
//! of the two (see `tests/equivalence.rs`).

use std::path::{Path, PathBuf};

use rivet_yaml::Value;
use rivet_yaml::cst::{SyntaxKind, SyntaxNode, parse};

pub const GUARD: &str = "${{ (github.event_name != 'pull_request' || \
github.event.pull_request.head.repo.full_name == github.repository) && fromJSON('[";
pub const TAIL: &str = "]') || 'ubuntu-24.04' }}";

pub const RULES: &[&str] = &["fork-guard"];

/// One self-hosted `runs-on` found in a workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub line: usize,
    pub ok: bool,
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
fn workflow_files(root: &Path) -> Vec<PathBuf> {
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

/// Keys named `runs-on` in the CST, in document order: (line, canonical
/// single-line plain value).
fn cst_sites(src: &str) -> Vec<(usize, Option<String>)> {
    let (green, _) = parse(src);
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
        let line = src[..off.min(src.len())].matches('\n').count() + 1;
        // Canonical guard form: the value's raw text on one line, no comment
        // after it. (The CST lexes `[`, `,` and quotes inside a plain
        // `${{ ... }}` scalar as flow tokens, so the raw text is the value.)
        let canon = node
            .children()
            .find(|c| c.kind() == SyntaxKind::Value)
            .and_then(|v| {
                let multi = v
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| matches!(t.kind(), SyntaxKind::Newline | SyntaxKind::Comment));
                let end: usize = v.text_range().end().into();
                let rest = src[end..].split('\n').next().unwrap_or("");
                (!multi && rest.trim().is_empty()).then(|| v.text().to_string().trim().to_string())
            });
        out.push((line, canon));
    }
    out
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
        // Aliases, flow mappings or anything else that breaks the pairing:
        // fail closed rather than guess a line.
        rep.errors.push(format!(
            "{name}: {} runs-on key(s) in the text but {} in the parsed value; cannot pair them",
            cst.len(),
            found.len()
        ));
        return;
    }
    for ((on, matrix), (line, canon)) in found.into_iter().zip(cst) {
        if !selects_self_hosted(on, matrix) {
            continue;
        }
        let guarded = canon
            .as_deref()
            .is_some_and(|v| v.starts_with(GUARD) && v.ends_with(TAIL))
            && on.as_str().is_some();
        rep.sites.push(Site {
            file: name.to_string(),
            line,
            ok: guarded && !prt,
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
