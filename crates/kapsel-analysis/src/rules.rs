//! The KS-S rules: findings from comparing commands with each other.

use std::collections::{BTreeMap, BTreeSet};

use kapsel_plan::{Finding, Related, Severity, Span};

use crate::distance::near_miss;
use crate::shape::{CommandShape, Connector};

pub const KS_S001: &str = "KS-S001";
pub const KS_S002: &str = "KS-S002";
pub const KS_S003: &str = "KS-S003";
pub const KS_S004: &str = "KS-S004";
pub const KS_S005: &str = "KS-S005";

/// Rule ids and severities of the KS-S rules.
pub const RULES: &[(&str, Severity)] = &[
    (KS_S001, Severity::Warn),
    (KS_S002, Severity::Warn),
    (KS_S003, Severity::Warn),
    (KS_S004, Severity::Warn),
    (KS_S005, Severity::Refuse),
];

/// Work bounds. The rules compare commands pairwise only within a bucket
/// (same tool, same line key, or candidate paths) and only over a bounded
/// window, so a script or session of n commands costs O(n * bound), never
/// O(n²).
///
/// KS-S002 compares a command with at most this many earlier uses of the
/// same tool.
pub const MAX_BUCKET: usize = 64;
/// KS-S001 and KS-S004 look back at most this many commands or lines.
pub const MAX_LOOKBACK: usize = 256;
/// KS-S003 and KS-S005 compare a path with at most this many distinct
/// candidate paths (the most recent written ones; the most used ones).
pub const MAX_CANDIDATES: usize = 1024;

fn excerpt(source: &str, span: Span) -> String {
    source.get(span.start as usize..span.end as usize).unwrap_or("").to_string()
}

fn finding(source: &str, rule: &str, severity: Severity, span: Span, message: String) -> Finding {
    Finding {
        rule: rule.into(),
        severity,
        message,
        span,
        excerpt: excerpt(source, span),
        help: None,
        notes: Vec::new(),
        related: Vec::new(),
    }
}

fn related(span: Span, label: impl Into<String>) -> Related {
    Related { span, label: label.into() }
}

/// What a command or line depends on, for "could anything in between have
/// changed its result".
#[derive(Debug, Default)]
struct Inputs {
    paths: BTreeSet<String>,
    dynamic: bool,
    vars: BTreeSet<String>,
}

fn inputs<'a>(shapes: impl IntoIterator<Item = &'a CommandShape>) -> Inputs {
    let mut inp = Inputs::default();
    for s in shapes {
        inp.paths.extend(s.all_paths().map(|p| p.path.clone()));
        inp.dynamic |= s.dynamic_paths;
        inp.vars.extend(s.vars_used.iter().cloned());
    }
    inp
}

fn path_related(a: &str, b: &str) -> bool {
    a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

/// True when running `inter` may change what a command with inputs `inp`
/// sees: it changes directory, sets a variable the command uses, is not a
/// built-in, or writes or removes a path the command touches (any path at all
/// when either side's paths are not literal).
fn may_change(inter: &CommandShape, inp: &Inputs) -> bool {
    if inter.tool() == Some("cd") || inter.vars_set.iter().any(|v| inp.vars.contains(v)) {
        return true;
    }
    if inter.tool().is_some_and(|t| !kapsel_plan::BUILTINS.contains(&t)) || inter.dynamic_write() {
        return true;
    }
    let mut touched: Vec<&str> = inter.writes.iter().chain(&inter.removes).map(|p| p.path.as_str()).collect();
    if inter.tool() == Some("sed") && inter.flags.iter().any(|f| f.name == "-i") {
        touched.extend(inter.reads.iter().map(|p| p.path.as_str()));
        if inter.dynamic_paths {
            return true;
        }
    }
    if touched.is_empty() {
        return false;
    }
    inp.dynamic || touched.iter().any(|t| inp.paths.iter().any(|p| path_related(t, p)))
}

/// Tools whose purpose is their output or a delay; repeating them is normal.
fn output_only(tool: Option<&str>) -> bool {
    matches!(tool, Some("echo" | "printf" | "date" | "sleep" | "seq" | "true" | "false" | ":" | "exit" | "pwd"))
}

fn line_key(shapes: &[&CommandShape]) -> String {
    let mut k = String::new();
    for s in shapes {
        if s.loop_depth > 0 {
            k.push_str(&format!("[{}]", s.loop_depth));
        }
        k.push_str(match s.connector {
            Connector::Start if k.is_empty() => "",
            Connector::Start => " ; ",
            Connector::Pipe => " | ",
            Connector::And => " && ",
            Connector::Or => " || ",
        });
        k.push_str(&s.key);
    }
    k
}

fn line_span(shapes: &[&CommandShape]) -> Span {
    let start = shapes.iter().map(|s| s.span.start).min().unwrap_or(0);
    let end = shapes.iter().map(|s| s.span.end).max().unwrap_or(0);
    Span::new(start, end)
}

fn sort_findings(v: &mut [Finding]) {
    v.sort_by(|a, b| (a.span.start, a.span.end, &a.rule).cmp(&(b.span.start, b.span.end, &b.rule)));
}

/// Runs KS-S002..S005 over the shapes of one script, in source order.
pub fn analyze_shapes(source: &str, shapes: &[CommandShape]) -> Vec<Finding> {
    let mut out = Vec::new();
    s002_inconsistent_flags(source, shapes, &mut out);
    s003_near_miss_path(source, shapes, &mut out);
    s004_duplicate_line(source, shapes, &mut out);
    s005_destructive_near_miss(source, shapes, &mut out);
    sort_findings(&mut out);
    out
}

fn flag_list(s: &CommandShape) -> BTreeSet<String> {
    s.flags.iter().map(|f| f.canonical.clone()).collect()
}

fn show(set: &BTreeSet<String>) -> String {
    if set.is_empty() { "no flags".into() } else { set.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ") }
}

fn s002_inconsistent_flags(source: &str, shapes: &[CommandShape], out: &mut Vec<Finding>) {
    let mut by_tool: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (j, later) in shapes.iter().enumerate() {
        let Some(tool) = later.tool() else { continue };
        if matches!(tool, "echo" | "printf" | "test" | "[" | "export" | "exit") {
            continue;
        }
        // Bucketed by tool; only the last MAX_BUCKET uses of the tool are
        // compared, so the work is O(n * MAX_BUCKET).
        let bucket = by_tool.entry(tool).or_default();
        let from = bucket.len().saturating_sub(MAX_BUCKET);
        let earlier_idx: Vec<usize> = bucket[from..].to_vec();
        bucket.push(j);
        let earlier = earlier_idx.iter().map(|&i| &shapes[i]);
        // (a) the same flag spelled two ways.
        let mut hit = false;
        'spell: for e in earlier.clone() {
            for lf in &later.flags {
                for ef in &e.flags {
                    if lf.name == ef.name && lf.form != ef.form {
                        let mut f = finding(
                            source,
                            KS_S002,
                            Severity::Warn,
                            lf.span,
                            format!(
                                "inconsistent flags for `{tool}`: `{}` here, `{}` earlier in the script for the same option",
                                lf.raw, ef.raw
                            ),
                        );
                        f.related.push(related(ef.span, format!("spelled `{}` here", ef.raw)));
                        f.help = Some(if lf.form == "-NUM" || ef.form == "-NUM" {
                            let v = lf.canonical.trim_start_matches(&format!("{} ", lf.name)).to_string();
                            format!("use one spelling throughout; `{tool} -n {v}` is the POSIX form")
                        } else if lf.form.len() == 2 && ef.form.len() == 2 && lf.form != ef.form {
                            format!(
                                "use one of `{}` and `{}` everywhere; for `{tool}` they are aliases (GNU `-R` also follows symlinks), so mixing them suggests one is a slip",
                                ef.form, lf.form
                            )
                        } else {
                            format!("use one spelling of `{}` throughout the script", lf.name)
                        });
                        out.push(f);
                        hit = true;
                        break 'spell;
                    }
                }
            }
        }
        if hit {
            continue;
        }
        // (b) the same tool on the same input with different flags.
        for e in earlier {
            if e.operands.iter().map(|a| &a.text).ne(later.operands.iter().map(|a| &a.text)) {
                continue;
            }
            if e.upstream != later.upstream || e.reads.iter().map(|p| &p.path).ne(later.reads.iter().map(|p| &p.path)) {
                continue;
            }
            let (lf, ef) = (flag_list(later), flag_list(e));
            if lf == ef {
                continue;
            }
            let only_here: BTreeSet<String> = lf.difference(&ef).cloned().collect();
            let only_there: BTreeSet<String> = ef.difference(&lf).cloned().collect();
            if only_here.len() + only_there.len() > 2 {
                continue;
            }
            // Underline the tool name and its flags, not the redirects.
            let end_of = |s: &CommandShape| s.flags.iter().map(|f| f.span.end).max().unwrap_or(s.tool_span.end);
            let span = Span::new(later.tool_span.start, end_of(later));
            let mut f = finding(
                source,
                KS_S002,
                Severity::Warn,
                span,
                format!(
                    "inconsistent flags for `{tool}`: the same input is processed with {} here and {} earlier",
                    show(&lf),
                    show(&ef)
                ),
            );
            f.related.push(related(Span::new(e.tool_span.start, end_of(e)), format!("`{tool}` with {} here", show(&ef))));
            let earlier_form = std::iter::once(tool.to_string()).chain(ef.iter().cloned()).collect::<Vec<_>>().join(" ");
            f.help = Some(format!(
                "if both should give the same result, write `{earlier_form}` in both places; if they differ on purpose, make that visible with different outputs or a comment"
            ));
            f.notes.push(format!("only here: {}; only there: {}", show(&only_here), show(&only_there)));
            out.push(f);
            break;
        }
    }
}

fn s003_near_miss_path(source: &str, shapes: &[CommandShape], out: &mut Vec<Finding>) {
    // Paths seen in any non-read role, anywhere in the script.
    let seen: BTreeSet<&str> = shapes
        .iter()
        .flat_map(|s| s.writes.iter().chain(&s.removes).chain(&s.mentions))
        .map(|p| p.path.as_str())
        .collect();
    let mut reported = BTreeSet::new();
    // Distinct paths written so far, first write of each, in write order.
    let mut written: Vec<&crate::shape::PathUse> = Vec::new();
    let mut written_set: BTreeSet<&str> = BTreeSet::new();
    for s in shapes {
        for r in &s.reads {
            // Seen: the path itself, or a directory holding a seen path
            // (a range query, not a scan).
            let dir = format!("{}/", r.path);
            let is_seen = seen.contains(r.path.as_str())
                || seen.range::<str, _>((std::ops::Bound::Included(dir.as_str()), std::ops::Bound::Unbounded)).next().is_some_and(|p| p.starts_with(&dir));
            if is_seen || reported.contains(&r.path) {
                continue;
            }
            // Candidates: the most recent MAX_CANDIDATES distinct paths
            // written before this command, other than the read path's own
            // directories.
            let mut best: Option<(usize, usize, &crate::shape::PathUse)> = None;
            let from = written.len().saturating_sub(MAX_CANDIDATES);
            for (i, w) in written.iter().enumerate().skip(from) {
                if path_related(&r.path, &w.path) {
                    continue;
                }
                if let Some(d) = near_miss(&r.path, &w.path) {
                    if best.is_none_or(|(bd, bi, _)| (d, i) < (bd, bi)) {
                        best = Some((d, i, w));
                    }
                }
            }
            let Some((d, _, w)) = best else { continue };
            reported.insert(r.path.clone());
            let mut f = finding(
                source,
                KS_S003,
                Severity::Warn,
                r.span,
                format!("`{}` is read, but nothing before it writes or creates it", r.path),
            );
            f.related.push(related(w.span, format!("`{}` is written here", w.path)));
            f.help = Some(format!("did you mean {}?", w.path));
            f.notes.push(format!(
                "the two paths differ by {d} edit{}; a read of a path the script never made usually fails or reads stale data",
                if d == 1 { "" } else { "s" }
            ));
            out.push(f);
        }
        for w in &s.writes {
            if written_set.insert(w.path.as_str()) {
                written.push(w);
            }
        }
    }
}

fn items(shapes: &[CommandShape]) -> BTreeMap<usize, Vec<&CommandShape>> {
    let mut m: BTreeMap<usize, Vec<&CommandShape>> = BTreeMap::new();
    for s in shapes {
        m.entry(s.item).or_default().push(s);
    }
    m
}

fn line_of(source: &str, offset: u32) -> usize {
    source[..(offset as usize).min(source.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

fn s004_duplicate_line(source: &str, shapes: &[CommandShape], out: &mut Vec<Finding>) {
    let items = items(shapes);
    let list: Vec<(usize, &Vec<&CommandShape>)> = items.iter().map(|(k, v)| (*k, v)).collect();
    // Bucketed by line key: the nearest earlier identical line is a lookup.
    let mut last_by_key: BTreeMap<String, usize> = BTreeMap::new();
    for (b, (_, later)) in list.iter().enumerate() {
        let key = line_key(later);
        let prev = last_by_key.insert(key, b);
        if later.iter().any(|s| s.mutates() || s.tool().is_none()) || later.iter().all(|s| output_only(s.tool())) {
            continue;
        }
        // The nearest earlier identical line within MAX_LOOKBACK, with
        // nothing in between that may change the result.
        if let Some(a) = prev.filter(|&a| b - a <= MAX_LOOKBACK) {
            let (_, earlier) = list[a];
            let inp = inputs(later.iter().copied());
            if list[a + 1..b].iter().flat_map(|(_, v)| v.iter()).any(|s| may_change(s, &inp)) {
                continue;
            }
            let span = line_span(later);
            let first = line_span(earlier);
            let mut f = finding(
                source,
                KS_S004,
                Severity::Warn,
                span,
                format!(
                    "duplicated command: this repeats line {} and nothing in between can change its result",
                    line_of(source, first.start)
                ),
            );
            f.related.push(related(first, "first run here"));
            f.help = Some("remove the repeat, or save the first result (`> file`) and reuse it".into());
            f.notes.push("note-level: a repeat is harmless but usually a copy-paste slip".into());
            out.push(f);
        }
    }
}

fn s005_destructive_near_miss(source: &str, shapes: &[CommandShape], out: &mut Vec<Finding>) {
    // path -> (indices of commands using it, first span). Parent directories
    // of used paths count as used by the same commands.
    let mut uses: BTreeMap<String, (BTreeSet<usize>, Span)> = BTreeMap::new();
    for (i, s) in shapes.iter().enumerate() {
        for p in s.all_paths() {
            let mut path = p.path.as_str();
            loop {
                let e = uses.entry(path.to_string()).or_insert_with(|| (BTreeSet::new(), p.span));
                e.0.insert(i);
                match path.rfind('/') {
                    Some(k) if k > 0 => path = &path[..k],
                    _ => break,
                }
            }
        }
    }
    // Candidates: paths used by at least 2 commands, most used first,
    // at most MAX_CANDIDATES of them.
    let mut heavy: Vec<(&String, &(BTreeSet<usize>, Span))> = uses.iter().filter(|(_, (set, _))| set.len() >= 2).collect();
    heavy.sort_by(|a, b| (std::cmp::Reverse(a.1.0.len()), a.0).cmp(&(std::cmp::Reverse(b.1.0.len()), b.0)));
    heavy.truncate(MAX_CANDIDATES);
    for (r, s) in shapes.iter().enumerate() {
        if !matches!(s.tool(), Some("rm" | "mv")) {
            continue;
        }
        let tool = s.tool().unwrap();
        for t in &s.removes {
            let used_elsewhere = uses.get(&t.path).is_some_and(|(set, _)| set.iter().any(|&i| i != r));
            if used_elsewhere {
                continue;
            }
            let mut best: Option<(usize, usize, &String, Span)> = None;
            for &(u, (set, first)) in &heavy {
                let n = set.len() - usize::from(set.contains(&r));
                if n < 2 {
                    continue;
                }
                if let Some(d) = near_miss(&t.path, u) {
                    // Prefer more uses, then smaller distance, then name order.
                    let better = match best {
                        None => true,
                        Some((bn, bd, _, _)) => (std::cmp::Reverse(n), d) < (std::cmp::Reverse(bn), bd),
                    };
                    if better {
                        best = Some((n, d, u, *first));
                    }
                }
            }
            let Some((n, d, u, first)) = best else { continue };
            let mut f = finding(
                source,
                KS_S005,
                Severity::Refuse,
                t.span,
                format!(
                    "`{tool}` targets `{}`, which appears nowhere else, but `{u}` is used {n} times in this script",
                    t.path
                ),
            );
            f.related.push(related(first, format!("`{u}` is first used here")));
            f.help = Some(format!(
                "did you mean `{u}`? if `{}` is really the target, name it once more before the {tool} (e.g. `test -e {} && {tool} ...`)",
                t.path, t.path
            ));
            f.notes.push(format!(
                "refused: the paths differ by {d} edit{}, and a typo in `{tool}` deletes or moves the wrong file",
                if d == 1 { "" } else { "s" }
            ));
            out.push(f);
        }
    }
}

/// One command of a session's history, as the session analysis sees it.
#[derive(Debug, Clone, Copy)]
pub struct HistoryEntry<'a> {
    /// Shapes with spans relative to the command's own text.
    pub shapes: &'a [CommandShape],
    /// Exit status of the command, `None` if refused or not yet run.
    pub status: Option<i32>,
}

/// A finding from the session analysis: spans index into the history text
/// built by [`history_text`], and `command` is the index of the command that
/// holds the primary span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFinding {
    pub command: usize,
    pub finding: Finding,
}

/// Joins the commands of a session into one text, one command after another
/// with a newline between them, and returns each command's start offset.
/// Session findings index into this text, so `--> history:3:1` reads as "the
/// third line of the history".
pub fn history_text<S: AsRef<str>>(commands: &[S]) -> (String, Vec<u32>) {
    let mut text = String::new();
    let mut offsets = Vec::with_capacity(commands.len());
    for (i, c) in commands.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        offsets.push(text.len() as u32);
        text.push_str(c.as_ref());
    }
    (text, offsets)
}

fn shift(span: Span, by: u32) -> Span {
    Span::new(span.start + by, span.end + by)
}

fn shifted(shapes: &[CommandShape], by: u32, item: usize) -> Vec<CommandShape> {
    shapes
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.span = shift(s.span, by);
            s.tool_span = shift(s.tool_span, by);
            for f in &mut s.flags {
                f.span = shift(f.span, by);
            }
            for a in &mut s.operands {
                a.span = shift(a.span, by);
            }
            for p in s.reads.iter_mut().chain(&mut s.writes).chain(&mut s.removes).chain(&mut s.mentions) {
                p.span = shift(p.span, by);
            }
            s.item = item;
            s
        })
        .collect()
}

/// Cross-command analysis over a session's history.
///
/// - KS-S001: a command that failed is run again with the same shape and
///   nothing that ran in between could change its inputs.
/// - KS-S002, S003, S005 across commands: the history is analysed as one
///   script made of the commands that ran (status known) plus the last one;
///   only findings that involve two different commands are kept, since the
///   per-command check already reports the rest.
///
/// `history` and `offsets` come from [`history_text`].
pub fn analyze_session(history: &str, offsets: &[u32], entries: &[HistoryEntry<'_>]) -> Vec<SessionFinding> {
    assert_eq!(offsets.len(), entries.len());
    let command_of = |pos: u32| offsets.iter().rposition(|&o| o <= pos).unwrap_or(0);
    let mut out = Vec::new();

    // KS-S001
    let keys: Vec<String> = entries
        .iter()
        .map(|e| line_key(&e.shapes.iter().collect::<Vec<_>>()))
        .collect();
    // Bucketed by key: entry indices per key, in order.
    let mut by_key: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for j in 0..entries.len() {
        let earlier_same: Vec<usize> = by_key.get(keys[j].as_str()).cloned().unwrap_or_default();
        by_key.entry(keys[j].as_str()).or_default().push(j);
        if entries[j].shapes.is_empty() || earlier_same.is_empty() {
            continue;
        }
        let inp = inputs(entries[j].shapes);
        // The window: at most MAX_LOOKBACK entries back, and not past the
        // latest entry that ran and may have changed the inputs.
        let floor = j.saturating_sub(MAX_LOOKBACK);
        let last_change = (floor..j)
            .rev()
            .find(|&k| entries[k].status.is_some() && entries[k].shapes.iter().any(|s| may_change(s, &inp)))
            .unwrap_or(floor);
        for &i in earlier_same.iter().rev() {
            if i < last_change {
                break;
            }
            let ei = &entries[i];
            let status = match ei.status {
                // Refused: it never ran, so it says nothing about the retry.
                None => continue,
                Some(0) => break,
                Some(s) => s,
            };
            let end_i = offsets.get(i + 1).map(|&o| o as usize - 1).unwrap_or(history.len());
            let end_j = offsets.get(j + 1).map(|&o| o as usize - 1).unwrap_or(history.len());
            let span_i = Span::new(offsets[i], end_i as u32);
            let span_j = Span::new(offsets[j], end_j as u32);
            let mut f = finding(
                history,
                KS_S001,
                Severity::Warn,
                span_j,
                format!("retry without change: this command failed with exit status {status} and is run again as it was"),
            );
            f.related.push(related(span_i, format!("failed here with exit status {status}")));
            f.help = Some("it failed the same way before; change the input or read the error".into());
            if excerpt(history, span_i) != excerpt(history, span_j) {
                f.notes.push("it differs from the failed run only in quoting, whitespace or flag order, which does not change what runs".into());
            }
            f.notes.push(
                "nothing that ran in between writes a file it reads, changes directory or sets a variable it uses".into(),
            );
            out.push(SessionFinding { command: j, finding: f });
            break;
        }
    }

    // KS-S002, S003, S005 across commands.
    let last = entries.len().saturating_sub(1);
    let mut combined = Vec::new();
    for (k, e) in entries.iter().enumerate() {
        if e.status.is_some() || k == last {
            combined.extend(shifted(e.shapes, offsets[k], k));
        }
    }
    let mut cross = Vec::new();
    s002_inconsistent_flags(history, &combined, &mut cross);
    s003_near_miss_path(history, &combined, &mut cross);
    s005_destructive_near_miss(history, &combined, &mut cross);
    for f in cross {
        let c = command_of(f.span.start);
        if f.related.iter().any(|r| command_of(r.span.start) != c) {
            out.push(SessionFinding { command: c, finding: f });
        }
    }
    out.sort_by(|a, b| (a.command, a.finding.span.start, &a.finding.rule).cmp(&(b.command, b.finding.span.start, &b.finding.rule)));
    out
}
