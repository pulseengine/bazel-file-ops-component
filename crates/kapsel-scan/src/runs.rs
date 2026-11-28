//! Rule `run-blocks` (REQ-KS-015, DD-KS-014): `kapsel check` over every
//! `run:` block of a repository's workflows, offline.
//!
//! Each `${{ ... }}` is replaced by a same-length opaque word, so byte offsets
//! in the checked text equal byte offsets in the YAML scalar's value and a
//! finding maps back to a YAML line and column. The check runs under
//! `catch_unwind`: a panic is class `crash`, which must never happen and is the
//! one thing that fails the scan. Refusals are advisory in Phase 0.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use kapsel_plan::{CheckOutcome, Finding, Severity};
use rivet_yaml::Value;
use rivet_yaml::cst::{SyntaxKind, SyntaxNode, parse as cst_parse};

use crate::workflow_files;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Plan,
    Refused,
    NotShell,
    Crash,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Plan => "plan",
            Class::Refused => "refused",
            Class::NotShell => "not-shell",
            Class::Crash => "crash",
        }
    }
}

/// One rule hit inside a block, placed in the original YAML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockFinding {
    pub rule: String,
    pub refuse: bool,
    /// The out-of-subset construct for a parse refusal (`KS-R000`), else empty.
    pub construct: String,
    /// 1-based YAML line and column.
    pub line: usize,
    pub col: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub file: String,
    /// 1-based line of the `run:` key.
    pub line: usize,
    pub class: Class,
    /// The effective `shell:` value (None: the default, bash).
    pub shell: Option<String>,
    pub findings: Vec<BlockFinding>,
    /// Panic message for class `crash`.
    pub crash: Option<String>,
}

#[derive(Debug, Default)]
pub struct RunsReport {
    pub blocks: Vec<Block>,
    /// Files that could not be read or paired (fail closed, counted, not a crash).
    pub errors: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub total: usize,
    pub plan: usize,
    pub refused: usize,
    pub not_shell: usize,
    pub crash: usize,
}

impl Counts {
    pub fn add(&mut self, o: &Counts) {
        self.total += o.total;
        self.plan += o.plan;
        self.refused += o.refused;
        self.not_shell += o.not_shell;
        self.crash += o.crash;
    }
}

impl RunsReport {
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for b in &self.blocks {
            c.total += 1;
            match b.class {
                Class::Plan => c.plan += 1,
                Class::Refused => c.refused += 1,
                Class::NotShell => c.not_shell += 1,
                Class::Crash => c.crash += 1,
            }
        }
        c
    }

    /// Constructs of refused blocks (parse refusals), most frequent first.
    pub fn constructs(&self) -> Vec<(String, usize)> {
        tally(
            self.blocks
                .iter()
                .flat_map(|b| &b.findings)
                .filter(|f| f.refuse && !f.construct.is_empty())
                .map(|f| f.construct.clone()),
        )
    }

    /// Rule ids of refusals, most frequent first.
    pub fn refused_rules(&self) -> Vec<(String, usize)> {
        tally(
            self.blocks
                .iter()
                .flat_map(|b| &b.findings)
                .filter(|f| f.refuse)
                .map(|f| f.rule.clone()),
        )
    }

    /// Warning rule ids of blocks that planned, most frequent first.
    pub fn warnings(&self) -> Vec<(String, usize)> {
        tally(
            self.blocks
                .iter()
                .flat_map(|b| &b.findings)
                .filter(|f| !f.refuse)
                .map(|f| f.rule.clone()),
        )
    }

    /// Effective shells of skipped blocks, most frequent first.
    pub fn not_shells(&self) -> Vec<(String, usize)> {
        tally(
            self.blocks
                .iter()
                .filter(|b| b.class == Class::NotShell)
                .map(|b| shell_word(b.shell.as_deref().unwrap_or("?"))),
        )
    }
}

pub fn tally(it: impl Iterator<Item = String>) -> Vec<(String, usize)> {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for k in it {
        *m.entry(k).or_default() += 1;
    }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// The shell program of a `shell:` value: `bash -e {0}` is `bash`.
fn shell_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").to_string()
}

/// `bash`, `sh` and `kapsel` (with or without a `{0}` template) are shell.
pub fn is_shell(shell: &str) -> bool {
    let w = shell_word(shell);
    let base = w.rsplit('/').next().unwrap_or("");
    matches!(base, "bash" | "sh" | "kapsel")
}

/// Replaces each `${{ ... }}` by `x` of the same byte length. A newline inside
/// an expression becomes a backslash-newline continuation, so the placeholder
/// stays one word and the line structure is unchanged.
pub fn mask_expressions(src: &str) -> String {
    let mut b = src.as_bytes().to_vec();
    let mut i = 0;
    while i + 2 < b.len() {
        if b[i] == b'$' && b[i + 1] == b'{' && b[i + 2] == b'{' {
            let Some(rel) = src[i..].find("}}") else {
                break;
            };
            let end = i + rel + 2;
            for k in i..end {
                if src.as_bytes()[k] == b'\n' {
                    if k > i && src.as_bytes()[k - 1] != b'\n' {
                        b[k - 1] = b'\\';
                    }
                } else if k + 1 >= end || src.as_bytes()[k + 1] != b'\n' {
                    b[k] = b'x';
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
    // Only ASCII bytes overwrite whole expressions, so this stays valid UTF-8.
    String::from_utf8_lossy(&b).into_owned()
}

/// Maps offsets of a script to (line, column) of the original YAML.
struct Locator {
    /// 0-based source line of each script line (None: blank or no match).
    script_to_src: Vec<Option<usize>>,
    /// Byte column in the source line where each script line's text starts.
    script_col: Vec<usize>,
    script_line_starts: Vec<usize>,
    fallback_line: usize,
}

impl Locator {
    fn new(src: &str, script: &str, first: usize, last: usize) -> Self {
        let src_lines: Vec<&str> = src.lines().collect();
        let mut starts = vec![0usize];
        for (i, c) in script.char_indices() {
            if c == '\n' {
                starts.push(i + 1);
            }
        }
        let mut cursor = first;
        let mut map = Vec::new();
        let mut cols = Vec::new();
        for (n, ls) in starts.iter().enumerate() {
            let le = starts
                .get(n + 1)
                .map(|e| e - 1)
                .unwrap_or(script.len())
                .max(*ls);
            let line = script[*ls..le].trim_end_matches('\r');
            let t = line.trim_start();
            if t.is_empty() {
                map.push(None);
                cols.push(0);
                continue;
            }
            let lead = line.len() - t.len();
            let mut cut = t.len().min(24);
            while !t.is_char_boundary(cut) {
                cut -= 1;
            }
            // A folded `>` line is several YAML lines joined, so the whole
            // prefix may not occur in one: retry with fewer words, down to the
            // first one, which locates the first YAML line of the joined text.
            let mut prefix = &t[..cut];
            let mut found = None;
            loop {
                let mut s = cursor;
                while s <= last.min(src_lines.len().saturating_sub(1)) {
                    if let Some(p) = src_lines[s].find(prefix) {
                        found = Some((s, p));
                        break;
                    }
                    s += 1;
                }
                match prefix.trim_end().rfind(char::is_whitespace) {
                    Some(sp) if found.is_none() => prefix = &prefix[..sp],
                    _ => break,
                }
            }
            match found {
                Some((s, p)) => {
                    map.push(Some(s));
                    cols.push(p.saturating_sub(lead));
                    cursor = s + 1;
                }
                None => {
                    map.push(None);
                    cols.push(0);
                }
            }
        }
        Locator {
            script_to_src: map,
            script_col: cols,
            script_line_starts: starts,
            fallback_line: first,
        }
    }

    /// 1-based (line, col) of a script byte offset.
    fn at(&self, off: usize) -> (usize, usize) {
        let n = match self.script_line_starts.binary_search(&off) {
            Ok(n) => n,
            Err(n) => n.saturating_sub(1),
        };
        let col_in = off - self.script_line_starts[n];
        let (line0, col0) = match self.script_to_src[n] {
            Some(s) => (s, self.script_col[n]),
            None => {
                // Blank or unmatched line: the nearest matched line above,
                // plus the distance.
                let mut back = 0;
                let mut m = n;
                loop {
                    if m == 0 {
                        break (self.fallback_line + back, 0);
                    }
                    m -= 1;
                    back += 1;
                    if let Some(s) = self.script_to_src[m] {
                        break (s + back, 0);
                    }
                }
            }
        };
        (line0 + 1, col0 + col_in + 1)
    }
}

fn to_findings(out: &[Finding], loc: &Locator, script_len: usize) -> Vec<BlockFinding> {
    out.iter()
        .map(|f| {
            let off = (f.span.start as usize).min(script_len);
            let (line, col) = loc.at(off);
            let construct = if f.rule == "KS-R000" {
                f.message.split('`').nth(1).unwrap_or("").to_string()
            } else if f.rule == "KS-R007" {
                "background".to_string()
            } else {
                String::new()
            };
            BlockFinding {
                rule: f.rule.clone(),
                refuse: f.severity == Severity::Refuse,
                construct,
                line,
                col,
                message: f.message.clone(),
            }
        })
        .collect()
}

/// Runs `f`; a panic becomes `Err(message)`. This is the one place a crash is
/// counted, so it is tested with a closure that panics.
pub fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string())
    })
}

/// Parse and check one masked script; a panic is returned as `Err`.
pub fn check_text(text: &str) -> Result<CheckOutcome, String> {
    guarded(|| match kapsel_syntax::parse(text) {
        Ok(script) => kapsel_check::check(text, &script),
        Err(errors) => kapsel_check::refuse_parse_errors(text, &errors),
    })
}

/// Does this job's `runs-on` name a Windows label (and no expression)? A
/// `${{ matrix.os }}` selector is not decided: it keeps the bash default.
fn windows_runner(job: &Value) -> bool {
    let labels: Vec<&str> = match job.get("runs-on") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Sequence(v)) => v.iter().filter_map(|x| x.as_str()).collect(),
        _ => return false,
    };
    !labels.iter().any(|l| l.contains("${{"))
        && labels
            .iter()
            .any(|l| l.to_ascii_lowercase().contains("windows"))
}

fn defaults_shell(v: &Value) -> Option<String> {
    v.get("defaults")?
        .get("run")?
        .get("shell")?
        .as_str()
        .map(|s| s.to_string())
}

/// `run:` entries of the CST whose value is not a mapping: the key line and
/// the last line of the entry (both 0-based).
fn cst_runs(src: &str) -> Vec<(usize, usize)> {
    let (green, _) = cst_parse(src);
    let root = SyntaxNode::new_root(green);
    let line_of = |off: usize| src[..off.min(src.len())].matches('\n').count();
    let mut out = Vec::new();
    for node in root.descendants() {
        if node.kind() != SyntaxKind::MappingEntry {
            continue;
        }
        let Some(key) = node.children().find(|c| c.kind() == SyntaxKind::Key) else {
            continue;
        };
        let k = key.text().to_string();
        if k.trim().trim_matches(|c| c == '"' || c == '\'') != "run" {
            continue;
        }
        // A step is a mapping that is a sequence item: `outputs.run`,
        // `with.run` and `defaults.run` are not.
        let in_item = node
            .parent()
            .and_then(|m| m.parent())
            .is_some_and(|p| p.kind() == SyntaxKind::SequenceItem);
        if !in_item {
            continue;
        }
        let val = node.children().find(|c| c.kind() == SyntaxKind::Value);
        if val
            .as_ref()
            .is_some_and(|v| v.children().any(|c| c.kind() == SyntaxKind::Mapping))
        {
            continue;
        }
        let start: usize = key.text_range().start().into();
        let end: usize = node.text_range().end().into();
        out.push((line_of(start), line_of(end.saturating_sub(1).max(start))));
    }
    out
}

/// Scan one workflow file's text. Appends to `rep`.
pub fn scan_file(name: &str, src: &str, rep: &mut RunsReport) {
    let doc: Value = match rivet_yaml::from_str(src) {
        Ok(d) => d,
        Err(e) => {
            rep.errors
                .push(format!("{name}: cannot read as one YAML document: {e}"));
            return;
        }
    };
    let wf_shell = defaults_shell(&doc);
    let mut steps: Vec<(String, Option<String>)> = Vec::new();
    if let Some(jobs) = doc.get("jobs").and_then(|j| j.as_mapping()) {
        for (_, job) in jobs.iter() {
            // GitHub runs a shell-less `run:` with pwsh on a Windows runner.
            let job_shell = defaults_shell(job)
                .or_else(|| wf_shell.clone())
                .or_else(|| windows_runner(job).then(|| "pwsh".to_string()));
            let Some(seq) = job.get("steps").and_then(|s| s.as_sequence()) else {
                continue;
            };
            for step in seq {
                let Some(run) = step.get("run").and_then(|r| r.as_str()) else {
                    continue;
                };
                let shell = step
                    .get("shell")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| job_shell.clone());
                steps.push((run.to_string(), shell));
            }
        }
    }
    let cst = cst_runs(src);
    if cst.len() != steps.len() {
        rep.errors.push(format!(
            "{name}: {} run: key(s) in the text but {} step run(s) in the parsed value; \
             cannot pair them",
            cst.len(),
            steps.len()
        ));
        return;
    }
    for ((script, shell), (first, last)) in steps.into_iter().zip(cst) {
        let line = first + 1;
        if shell.as_deref().is_some_and(|sh| !is_shell(sh)) {
            rep.blocks.push(Block {
                file: name.to_string(),
                line,
                class: Class::NotShell,
                shell,
                findings: Vec::new(),
                crash: None,
            });
            continue;
        }
        let masked = mask_expressions(&script);
        let loc = Locator::new(src, &script, first, last);
        let (class, findings, crash) = match check_text(&masked) {
            Ok(CheckOutcome::Plan(p)) => (
                Class::Plan,
                to_findings(&p.warnings, &loc, script.len()),
                None,
            ),
            Ok(CheckOutcome::Refused(r)) => (
                Class::Refused,
                to_findings(&r.findings, &loc, script.len()),
                None,
            ),
            Err(msg) => (Class::Crash, Vec::new(), Some(msg)),
        };
        rep.blocks.push(Block {
            file: name.to_string(),
            line,
            class,
            shell,
            findings,
            crash,
        });
    }
}

/// Rule `run-blocks` over `<root>/.github/workflows`.
pub fn run_blocks(root: &Path) -> RunsReport {
    let mut rep = RunsReport::default();
    for p in workflow_files(root) {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        match std::fs::read_to_string(&p) {
            Ok(src) => scan_file(&name, &src, &mut rep),
            Err(e) => rep.errors.push(format!("{name}: {e}")),
        }
    }
    rep
}

fn top(list: &[(String, usize)], n: usize) -> String {
    if list.is_empty() {
        return "none".to_string();
    }
    list.iter()
        .take(n)
        .map(|(k, c)| format!("{k} x{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Text report for one repository.
pub fn render_text(repo: &str, rep: &RunsReport, verbose: bool) -> String {
    let c = rep.counts();
    let mut s = format!(
        "{repo}: {} run block(s): {} plan, {} refused, {} not-shell, {} crash\n",
        c.total, c.plan, c.refused, c.not_shell, c.crash
    );
    s.push_str(&format!(
        "  constructs: {}\n  refusal rules: {}\n  warnings: {}\n",
        top(&rep.constructs(), 10),
        top(&rep.refused_rules(), 10),
        top(&rep.warnings(), 10)
    ));
    for e in &rep.errors {
        s.push_str(&format!("  error: {e}\n"));
    }
    for b in &rep.blocks {
        let interesting = matches!(b.class, Class::Refused | Class::Crash);
        if !(verbose || interesting) {
            continue;
        }
        s.push_str(&format!("  {} {}:{}\n", b.class.as_str(), b.file, b.line));
        if let Some(m) = &b.crash {
            s.push_str(&format!("    PANIC: {m}\n"));
        }
        for f in &b.findings {
            if f.refuse || verbose {
                s.push_str(&format!(
                    "    {} {}:{}:{} {}\n",
                    f.rule, b.file, f.line, f.col, f.message
                ));
            }
        }
    }
    s
}

fn pairs(v: &[(String, usize)]) -> serde_json::Value {
    serde_json::Value::Array(
        v.iter()
            .map(|(k, n)| serde_json::json!({"name": k, "count": n}))
            .collect(),
    )
}

pub fn counts_json(c: &Counts) -> serde_json::Value {
    serde_json::json!({
        "total": c.total, "plan": c.plan, "refused": c.refused,
        "not_shell": c.not_shell, "crash": c.crash,
    })
}

/// JSON report for one repository.
pub fn render_json(repo: &str, rep: &RunsReport) -> serde_json::Value {
    let blocks: Vec<_> = rep
        .blocks
        .iter()
        .map(|b| {
            let findings: Vec<_> = b
                .findings
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "rule": f.rule,
                        "severity": if f.refuse { "refuse" } else { "warn" },
                        "construct": f.construct, "line": f.line, "col": f.col,
                        "message": f.message,
                    })
                })
                .collect();
            serde_json::json!({
                "file": b.file, "line": b.line, "class": b.class.as_str(),
                "shell": b.shell, "findings": findings, "crash": b.crash,
            })
        })
        .collect();
    serde_json::json!({
        "rule": "run-blocks", "repo": repo, "counts": counts_json(&rep.counts()),
        "constructs": pairs(&rep.constructs()), "refusal_rules": pairs(&rep.refused_rules()),
        "warnings": pairs(&rep.warnings()), "not_shell": pairs(&rep.not_shells()),
        "errors": rep.errors, "blocks": blocks,
    })
}
