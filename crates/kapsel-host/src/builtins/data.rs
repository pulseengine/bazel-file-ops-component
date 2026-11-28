//! `json`, `yaml`, `toml` (REQ-KS-063, DD-KS-020): get and set by path
//! without an interpreter.
//!
//! ```text
//! json get PATH [FILE]   json set PATH VALUE FILE   json del PATH FILE   json merge FILE OTHER
//! yaml get PATH [FILE]   yaml set PATH VALUE FILE   yaml del PATH FILE
//! toml get PATH [FILE]   toml set PATH VALUE FILE
//! ```
//!
//! PATH is a jq-like subset: `.`, `.a.b`, `.a[0]`, `.a[-1]`, `."key with
//! space"`, `.["key"]`, and `.a[]` (each element; get only). A missing key is
//! an error (exit 1), never a silent `null`.
//!
//! Edits change the edited node and nothing else (DD-KS-020, the notary's
//! interlineation rule): JSON is spliced by byte range (`jsonedit.rs`);
//! YAML is spliced into the lossless rivet-yaml CST, so comments, quoting
//! and layout outside the node stay byte for byte; TOML goes through
//! toml_edit and keeps the old value's comments and number text. A YAML
//! number, boolean or flow VALUE is written as typed. Every edit is
//! read back and compared with the intended document before the file is
//! replaced (the lab notebook's countersignature); a mismatch leaves the file
//! untouched.

use super::{Ctx, XParsed, xgetopt};

// ---- paths -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Key(String),
    Index(i64),
    Each,
}

pub(super) fn show_steps(steps: &[Step]) -> String {
    if steps.is_empty() {
        return ".".to_string();
    }
    let mut s = String::new();
    for st in steps {
        match st {
            Step::Key(k)
                if !k.is_empty()
                    && k.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
            {
                s.push('.');
                s.push_str(k);
            }
            Step::Key(k) => {
                s.push('.');
                s.push_str(&serde_json::to_string(k).unwrap_or_default());
            }
            Step::Index(i) => s.push_str(&format!("[{i}]")),
            Step::Each => s.push_str("[]"),
        }
    }
    s
}

/// Parses a jq-like path.
pub fn parse_path(p: &str) -> Result<Vec<Step>, String> {
    let bad = |why: &str| format!("bad path '{p}': {why} (paths look like .a.b, .a[0], .\"a b\")");
    let c: Vec<char> = p.chars().collect();
    if c.first() != Some(&'.') {
        return Err(bad("a path starts with `.`"));
    }
    let mut i = 1;
    let mut steps = Vec::new();
    // After a `.`: an identifier, a quoted key, a bracket, or (first only) the end.
    let mut after_dot = true;
    while i < c.len() {
        match c[i] {
            '[' => {
                i += 1;
                let start = i;
                if c.get(i) == Some(&'"') {
                    let (k, n) = quoted(&c[i..]).map_err(|e| bad(&e))?;
                    i += n;
                    if c.get(i) != Some(&']') {
                        return Err(bad("expected `]`"));
                    }
                    steps.push(Step::Key(k));
                } else {
                    while i < c.len() && c[i] != ']' {
                        i += 1;
                    }
                    if i >= c.len() {
                        return Err(bad("missing `]`"));
                    }
                    let inner: String = c[start..i].iter().collect();
                    if inner.is_empty() {
                        steps.push(Step::Each);
                    } else {
                        let n = inner
                            .trim()
                            .parse::<i64>()
                            .map_err(|_| bad(&format!("`[{inner}]` is not an index")))?;
                        steps.push(Step::Index(n));
                    }
                }
                i += 1;
                after_dot = false;
            }
            '.' if !after_dot => {
                i += 1;
                after_dot = true;
                if i >= c.len() {
                    return Err(bad("ends with `.`"));
                }
            }
            '"' if after_dot => {
                let (k, n) = quoted(&c[i..]).map_err(|e| bad(&e))?;
                i += n;
                steps.push(Step::Key(k));
                after_dot = false;
            }
            ch if after_dot && (ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '$') => {
                let start = i;
                while i < c.len()
                    && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '-' || c[i] == '$')
                {
                    i += 1;
                }
                steps.push(Step::Key(c[start..i].iter().collect()));
                after_dot = false;
            }
            ch => return Err(bad(&format!("unexpected `{ch}`"))),
        }
    }
    Ok(steps)
}

/// A JSON string literal at the start of `c`; returns it and its length.
fn quoted(c: &[char]) -> Result<(String, usize), String> {
    let mut i = 1;
    while i < c.len() {
        match c[i] {
            '\\' => i += 2,
            '"' => {
                let lit: String = c[..=i].iter().collect();
                let s: String = serde_json::from_str(&lit).map_err(|e| e.to_string())?;
                return Ok((s, i + 1));
            }
            _ => i += 1,
        }
    }
    Err("unterminated quoted key".to_string())
}

pub(super) fn norm_index(i: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let j = if i < 0 { len + i } else { i };
    (0..len).contains(&j).then_some(j as usize)
}

// ---- shared command shape ----------------------------------------------------

pub(super) struct Cmd {
    pub sub: String,
    pub path: Vec<Step>,
    pub rest: Vec<String>,
    pub opts: XParsed,
}

/// `TOOL SUB ...`: parses flags, the subcommand and its PATH (all but merge).
pub(super) fn command(ctx: &mut Ctx, a: &[String], subs: &[&str]) -> Result<Cmd, i32> {
    let usage = |ctx: &mut Ctx, why: &str| {
        let n = ctx.name.clone();
        let forms: Vec<String> = subs
            .iter()
            .map(|s| match *s {
                "get" => format!("{n} get PATH [FILE]"),
                "set" => format!("{n} set PATH VALUE FILE"),
                "del" => format!("{n} del PATH FILE"),
                _ => format!("{n} merge FILE OTHER (rewrites FILE in a normal layout)"),
            })
            .collect();
        ctx.msg(&format!("{why}; usage: {}", forms.join(" | ")));
        2
    };
    let Ok(opts) = xgetopt(ctx, a) else {
        return Err(2);
    };
    let mut ops = opts.operands.clone();
    if ops.is_empty() {
        return Err(usage(ctx, "missing subcommand"));
    }
    let sub = ops.remove(0);
    if !subs.contains(&sub.as_str()) {
        return Err(usage(ctx, &format!("unknown subcommand '{sub}'")));
    }
    let want = match sub.as_str() {
        "get" => 1..=2,
        "set" => 3..=3,
        "del" => 2..=2,
        _ => 2..=2,
    };
    if !want.contains(&ops.len()) {
        return Err(usage(
            ctx,
            &format!("wrong number of operands for {sub} ({})", ops.len()),
        ));
    }
    let path = if sub == "merge" {
        Vec::new()
    } else {
        let p = ops.remove(0);
        match parse_path(&p) {
            Ok(s) => s,
            Err(e) => {
                ctx.msg(&e);
                return Err(2);
            }
        }
    };
    if sub != "get" && path.contains(&Step::Each) {
        ctx.msg("`[]` selects many values; set and del take one path");
        return Err(2);
    }
    if opts.has("--str") && sub != "set" {
        return Err(usage(ctx, "--str applies to set"));
    }
    Ok(Cmd {
        sub,
        path,
        rest: ops,
        opts,
    })
}

/// The text of `get`'s input: FILE, or stdin when FILE is omitted.
pub(super) fn get_input(ctx: &mut Ctx, rest: &[String]) -> Option<String> {
    let data = match rest.first() {
        Some(f) => ctx.read_input(f)?,
        None => ctx.stdin_all(),
    };
    match String::from_utf8(data) {
        Ok(s) => Some(s),
        Err(_) => {
            let src = rest.first().cloned().unwrap_or_else(|| "-".to_string());
            ctx.msg(&format!("{src}: not UTF-8 text"));
            None
        }
    }
}

/// FILE for an edit, as UTF-8 text.
pub(super) fn edit_input(ctx: &mut Ctx, file: &str) -> Option<(std::path::PathBuf, String)> {
    let (path, data) = ctx.edit_target(file)?;
    match String::from_utf8(data) {
        Ok(s) => Some((path, s)),
        Err(_) => {
            ctx.msg(&format!("{file}: not UTF-8 text"));
            None
        }
    }
}

// ---- yaml --------------------------------------------------------------------

use rivet_yaml::Value as Y;
use rivet_yaml::cst::{SyntaxKind as K, SyntaxNode};

fn y_key_text(k: &Y) -> Option<String> {
    match k {
        Y::String(s) => Some(s.clone()),
        Y::Number(n) => Some(n.to_string()),
        Y::Bool(b) => Some(b.to_string()),
        Y::Null => Some("null".to_string()),
        _ => None,
    }
}

fn y_kind(v: &Y) -> &'static str {
    match v {
        Y::Null => "null",
        Y::Bool(_) => "a boolean",
        Y::Number(_) => "a number",
        Y::String(_) => "a string",
        Y::Sequence(_) => "a sequence",
        Y::Mapping(_) => "a mapping",
        Y::Tagged(_) => "a tagged value",
    }
}

fn y_get<'a>(v: &'a Y, steps: &[Step], done: &[Step], out: &mut Vec<&'a Y>) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        out.push(v);
        return Ok(());
    };
    let mut next_done = done.to_vec();
    next_done.push(st.clone());
    match (st, v) {
        (Step::Key(k), Y::Mapping(m)) => {
            match m
                .iter()
                .find(|(mk, _)| y_key_text(mk).as_deref() == Some(k))
            {
                Some((_, c)) => y_get(c, rest, &next_done, out),
                None => Err(format!("{}: no such key", show_steps(&next_done))),
            }
        }
        (Step::Index(i), Y::Sequence(a)) => match norm_index(*i, a.len()) {
            Some(j) => y_get(&a[j], rest, &next_done, out),
            None => Err(format!(
                "{}: index out of range (length {})",
                show_steps(&next_done),
                a.len()
            )),
        },
        (Step::Each, Y::Sequence(a)) => {
            for c in a {
                y_get(c, rest, &next_done, out)?;
            }
            Ok(())
        }
        (Step::Each, Y::Mapping(m)) => {
            for (_, c) in m.iter() {
                y_get(c, rest, &next_done, out)?;
            }
            Ok(())
        }
        (st, v) => Err(format!(
            "{}: {} cannot be indexed with {}",
            show_steps(done),
            y_kind(v),
            show_steps(std::slice::from_ref(st))
        )),
    }
}

/// The intended document after a set (or a del when `new` is None): what
/// the spliced text must read back as.
fn y_apply(v: &mut Y, steps: &[Step], new: Option<&Y>) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        if let Some(n) = new {
            *v = n.clone();
        }
        return Ok(());
    };
    if v.is_null() && matches!(st, Step::Key(_)) && new.is_some() {
        *v = Y::Mapping(rivet_yaml::Mapping::new());
    }
    match (st, v) {
        (Step::Key(k), Y::Mapping(m)) => {
            let key = m
                .iter()
                .find(|(mk, _)| y_key_text(mk).as_deref() == Some(k))
                .map(|(mk, _)| mk.clone());
            match (key, new, rest.is_empty()) {
                (Some(key), None, true) => {
                    m.shift_remove(&key);
                    Ok(())
                }
                (Some(key), _, _) => y_apply(m.get_mut(&key).expect("key present"), rest, new),
                (None, Some(_), _) => {
                    let mut slot = Y::Null;
                    y_apply(&mut slot, rest, new)?;
                    m.insert(Y::String(k.clone()), slot);
                    Ok(())
                }
                (None, None, _) => Err("no such key".to_string()),
            }
        }
        (Step::Index(i), Y::Sequence(a)) => {
            if new.is_some() && *i == a.len() as i64 {
                a.push(Y::Null);
            }
            match norm_index(*i, a.len()) {
                Some(j) if new.is_none() && rest.is_empty() => {
                    a.remove(j);
                    Ok(())
                }
                Some(j) => y_apply(&mut a[j], rest, new),
                None => Err("index out of range".to_string()),
            }
        }
        _ => Err("not a mapping or sequence".to_string()),
    }
}

/// A scalar as YAML text that reads back as the same value in flow and
/// block context, and under YAML 1.1 readers too (`yes`, `on` are quoted).
fn y_scalar(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let risky = s.is_empty()
        || s.trim() != s
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.chars().any(|c| c.is_control() || ",[]{}".contains(c))
        || s.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c))
        || [
            "y", "n", "yes", "no", "on", "off", "true", "false", "null", "~",
        ]
        .contains(&lower.as_str())
        || rivet_yaml::from_str::<Y>(s).ok() != Some(Y::String(s.to_string()));
    if risky {
        serde_json::to_string(s).unwrap_or_default()
    } else {
        s.to_string()
    }
}

/// A value as one line of YAML (flow style for collections).
fn y_flow(v: &Y) -> String {
    match v {
        Y::Null => "null".to_string(),
        Y::Bool(b) => b.to_string(),
        Y::Number(n) => n.to_string(),
        Y::String(s) => y_scalar(s),
        Y::Sequence(a) => format!("[{}]", a.iter().map(y_flow).collect::<Vec<_>>().join(", ")),
        Y::Mapping(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}: {}", y_flow(k), y_flow(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Y::Tagged(_) => rivet_yaml::to_string(v)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
}

/// One child of a CST node, copied out so rowan's element type is never
/// named (rivet-yaml builds on a rowan fork; DD-KS-020).
struct El {
    kind: K,
    text: String,
    start: usize,
    end: usize,
    node: Option<SyntaxNode>,
}

/// A closure that copies a rowan element (node or token) into an [`El`].
macro_rules! el_of {
    () => {
        |e| {
            let r = e.text_range();
            El {
                kind: e.kind(),
                text: e
                    .as_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default(),
                start: usize::from(r.start()),
                end: usize::from(r.end()),
                node: e.into_node(),
            }
        }
    };
}

fn els(n: &SyntaxNode) -> Vec<El> {
    n.children_with_tokens().map(el_of!()).collect()
}

fn is_trivia(e: &El) -> bool {
    e.node.is_none()
        && (matches!(e.kind, K::Whitespace | K::Newline | K::Comment)
            || (e.kind == K::BlockScalarLine && e.text.trim().is_empty()))
}

fn is_scalar(k: K) -> bool {
    matches!(
        k,
        K::PlainScalar | K::SingleQuotedScalar | K::DoubleQuotedScalar
    )
}

/// End offset of the last significant token in `n` (not whitespace, a
/// newline, a comment or a blank block-scalar line).
fn sig_end(n: &SyntaxNode) -> usize {
    let mut end = usize::from(n.text_range().start());
    for e in n.descendants_with_tokens().map(el_of!()) {
        if e.node.is_none() && !is_trivia(&e) {
            end = e.end;
        }
    }
    end
}

/// The value inside a container (Root, Value, SequenceItem).
enum Content {
    Node(SyntaxNode),
    /// A scalar token's byte range.
    Scalar(usize, usize),
    Empty,
}

fn content_of(container: &SyntaxNode) -> Result<Content, String> {
    for e in els(container) {
        if is_trivia(&e)
            || (e.node.is_none() && matches!(e.kind, K::Dash | K::DirectiveMarker | K::DocumentEnd))
        {
            continue;
        }
        return match e.node {
            None if is_scalar(e.kind) => Ok(Content::Scalar(e.start, e.end)),
            None if matches!(e.kind, K::Anchor | K::Alias) => Err(
                "anchors and aliases are not edited; edit the anchored text with `edit`"
                    .to_string(),
            ),
            None => Err(format!("unexpected `{}`", e.text)),
            Some(_) if matches!(e.kind, K::Anchor | K::Alias) => Err(
                "anchors and aliases are not edited; edit the anchored text with `edit`"
                    .to_string(),
            ),
            Some(_) if e.kind == K::Error => Err("the YAML does not parse here".to_string()),
            Some(n) => Ok(Content::Node(n)),
        };
    }
    Ok(Content::Empty)
}

fn is_flow(n: &SyntaxNode) -> bool {
    n.kind() == K::FlowSequence
        || els(n)
            .iter()
            .any(|e| e.node.is_none() && e.kind == K::LBrace)
}

fn entry_key(entry: &SyntaxNode) -> Option<String> {
    let key = entry.children().find(|c| c.kind() == K::Key)?;
    let tok = els(&key)
        .into_iter()
        .find(|e| e.node.is_none() && is_scalar(e.kind))?;
    Some(rivet_yaml::cst_text::unquote_scalar(tok.kind, &tok.text))
}

pub(super) fn line_start(src: &str, at: usize) -> usize {
    src[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn column(src: &str, at: usize) -> usize {
    at - line_start(src, at)
}

/// End of the line holding `at`, including its newline.
/// End of the line holding `at`, before its newline: a new entry goes after
/// the last entry's trailing comment, which stays on its line.
fn line_body_end(src: &str, at: usize) -> usize {
    src[at..].find('\n').map_or(src.len(), |i| at + i)
}

fn line_end(src: &str, at: usize) -> usize {
    src[at..].find('\n').map_or(src.len(), |i| at + i + 1)
}

/// One text splice: replace `start..end` with `text`.
struct Splice {
    start: usize,
    end: usize,
    text: String,
}

/// Where a path lands in the CST.
enum Found {
    /// The node or scalar exists; `container` holds it (a Value, a
    /// SequenceItem, or the Root).
    Here {
        container: SyntaxNode,
        content: Content,
        /// The MappingEntry or SequenceItem it belongs to, and its parent
        /// collection (None at the root).
        member: Option<(SyntaxNode, SyntaxNode)>,
        /// For a MappingEntry: the colon's end, where an empty value goes.
        colon_end: Option<usize>,
    },
    /// A key is missing from a block mapping (or the document is empty);
    /// `steps` are the steps from the missing one on.
    MissingKey {
        mapping: Option<SyntaxNode>,
        steps: Vec<Step>,
    },
    /// Index = length of a block sequence.
    Append { seq: SyntaxNode, steps: Vec<Step> },
}

fn y_locate(root: &SyntaxNode, steps: &[Step]) -> Result<Found, String> {
    let mut container = root.clone();
    let mut member: Option<(SyntaxNode, SyntaxNode)> = None;
    let mut colon_end = None;
    for (i, st) in steps.iter().enumerate() {
        let here = show_steps(&steps[..i]);
        let content = content_of(&container).map_err(|e| format!("{here}: {e}"))?;
        let node = match content {
            Content::Node(n) => n,
            Content::Empty if container.kind() == K::Root => {
                return Ok(Found::MissingKey {
                    mapping: None,
                    steps: steps[i..].to_vec(),
                });
            }
            Content::Empty | Content::Scalar(..) => {
                return Err(format!(
                    "{here} is a scalar or empty; set {here} to a mapping or sequence first"
                ));
            }
        };
        if is_flow(&node) {
            return Err(format!(
                "{here} is a flow collection ({}); set the whole value at {here} instead",
                node.text()
            ));
        }
        match (st, node.kind()) {
            (Step::Key(k), K::Mapping) => {
                let entry = node
                    .children()
                    .filter(|c| c.kind() == K::MappingEntry)
                    .find(|e| entry_key(e).as_deref() == Some(k.as_str()));
                let Some(entry) = entry else {
                    return Ok(Found::MissingKey {
                        mapping: Some(node),
                        steps: steps[i..].to_vec(),
                    });
                };
                colon_end = els(&entry)
                    .iter()
                    .find(|e| e.node.is_none() && e.kind == K::Colon)
                    .map(|e| e.end);
                container = entry
                    .children()
                    .find(|c| c.kind() == K::Value)
                    .unwrap_or_else(|| entry.clone());
                member = Some((entry, node));
            }
            (Step::Index(ix), K::Sequence) => {
                let items: Vec<SyntaxNode> = node
                    .children()
                    .filter(|c| c.kind() == K::SequenceItem)
                    .collect();
                if *ix == items.len() as i64 {
                    return Ok(Found::Append {
                        seq: node,
                        steps: steps[i..].to_vec(),
                    });
                }
                let Some(j) = norm_index(*ix, items.len()) else {
                    return Err(format!(
                        "{}: index out of range (length {})",
                        show_steps(&steps[..=i]),
                        items.len()
                    ));
                };
                container = items[j].clone();
                colon_end = None;
                member = Some((items[j].clone(), node));
            }
            (st, kind) => {
                let what = match kind {
                    K::Mapping => "a mapping",
                    K::Sequence => "a sequence",
                    K::BlockScalar => "a block scalar",
                    _ => "not a collection",
                };
                return Err(format!(
                    "{here}: {what} cannot be indexed with {}",
                    show_steps(std::slice::from_ref(st))
                ));
            }
        }
    }
    let content = content_of(&container).map_err(|e| format!("{}: {e}", show_steps(steps)))?;
    Ok(Found::Here {
        container,
        content,
        member,
        colon_end,
    })
}

/// The value the missing steps build: `.a.b = v` from `a` on is `{b: v}`
/// under key `a`.
fn nest(steps: &[Step], v: &Y) -> Result<Y, String> {
    let mut cur = v.clone();
    for st in steps.iter().rev() {
        match st {
            Step::Key(k) => {
                let mut m = rivet_yaml::Mapping::new();
                m.insert(Y::String(k.clone()), cur);
                cur = Y::Mapping(m);
            }
            Step::Index(0) => cur = Y::Sequence(vec![cur]),
            _ => return Err("a missing sequence can only be created at index 0".to_string()),
        }
    }
    Ok(cur)
}

/// `key: value` lines for a new entry at `indent`; nested mappings become
/// block lines, anything else one flow value.
fn y_block_entry(key: &str, v: &Y, indent: usize) -> String {
    match v {
        Y::Mapping(m) if !m.is_empty() => {
            let mut s = format!("{}:", y_scalar(key));
            for (k, c) in m.iter() {
                let kt = y_key_text(k).unwrap_or_default();
                s.push('\n');
                s.push_str(&" ".repeat(indent + 2));
                s.push_str(&y_block_entry(&kt, c, indent + 2));
            }
            s
        }
        _ => format!("{}: {}", y_scalar(key), y_flow(v)),
    }
}

fn y_set_splice(
    src: &str,
    root: &SyntaxNode,
    steps: &[Step],
    v: &Y,
    lit: Option<&str>,
) -> Result<Splice, String> {
    // The VALUE as typed (`1.10` stays `1.10`) where it lands whole.
    let flow = |x: &Y| match lit {
        Some(l) if x == v => l.to_string(),
        _ => y_flow(x),
    };
    let entry = |k: &str, x: &Y, indent: usize| match lit {
        Some(l) if x == v && !matches!(x, Y::Mapping(m) if !m.is_empty()) => {
            format!("{}: {l}", y_scalar(k))
        }
        _ => y_block_entry(k, x, indent),
    };
    match y_locate(root, steps)? {
        Found::Here {
            container,
            content,
            colon_end,
            ..
        } => match content {
            Content::Scalar(s, e) => Ok(Splice {
                start: s,
                end: e,
                text: flow(v),
            }),
            Content::Empty => match colon_end {
                Some(c) => Ok(Splice {
                    start: c,
                    end: c,
                    text: format!(" {}", flow(v)),
                }),
                None => {
                    // An empty sequence item `-`, or an empty document.
                    let at = sig_end(&container).max(usize::from(container.text_range().start()));
                    let (pre, at) = if container.kind() == K::Root {
                        (String::new(), src.len())
                    } else {
                        (" ".to_string(), at)
                    };
                    Ok(Splice {
                        start: at,
                        end: at,
                        text: format!("{pre}{}", flow(v)),
                    })
                }
            },
            Content::Node(n) => {
                if container.kind() == K::Root {
                    // The whole document.
                    let mut text = rivet_yaml::to_string(v).map_err(|e| e.to_string())?;
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    return Ok(Splice {
                        start: usize::from(n.text_range().start()),
                        end: sig_end(&n),
                        text,
                    });
                }
                // A block collection or block scalar under a key starts on
                // the next line: replace from right after the colon.
                let start = match (container.kind(), colon_end) {
                    (K::Value, Some(c)) => c,
                    _ => usize::from(n.text_range().start()),
                };
                let pre = if container.kind() == K::Value {
                    " "
                } else {
                    ""
                };
                Ok(Splice {
                    start,
                    end: sig_end(&n),
                    text: format!("{pre}{}", flow(v)),
                })
            }
        },
        Found::MissingKey { mapping, steps } => {
            let Step::Key(k) = &steps[0] else {
                return Err("expected a key".to_string());
            };
            let inner = nest(&steps[1..], v)?;
            match mapping {
                None => {
                    // An empty document: the first entry.
                    let mut text = String::new();
                    if !src.is_empty() && !src.ends_with('\n') {
                        text.push('\n');
                    }
                    text.push_str(&entry(k, &inner, 0));
                    text.push('\n');
                    Ok(Splice {
                        start: src.len(),
                        end: src.len(),
                        text,
                    })
                }
                Some(m) => {
                    let first = m
                        .children()
                        .find(|c| c.kind() == K::MappingEntry)
                        .ok_or("empty mapping")?;
                    let indent = column(src, usize::from(first.text_range().start()));
                    let at = line_body_end(src, sig_end(&m));
                    Ok(Splice {
                        start: at,
                        end: at,
                        text: format!("\n{}{}", " ".repeat(indent), entry(k, &inner, indent)),
                    })
                }
            }
        }
        Found::Append { seq, steps } => {
            let inner = nest(&steps[1..], v)?;
            let first = seq
                .children()
                .find(|c| c.kind() == K::SequenceItem)
                .ok_or("empty sequence")?;
            let indent = column(src, usize::from(first.text_range().start()));
            let at = line_body_end(src, sig_end(&seq));
            Ok(Splice {
                start: at,
                end: at,
                text: format!("\n{}- {}", " ".repeat(indent), flow(&inner)),
            })
        }
    }
}

fn y_del_splice(src: &str, root: &SyntaxNode, steps: &[Step]) -> Result<Splice, String> {
    let shown = show_steps(steps);
    match y_locate(root, steps)? {
        Found::Here {
            member: Some((m, parent)),
            ..
        } => {
            let kind = m.kind();
            let siblings: Vec<SyntaxNode> =
                parent.children().filter(|c| c.kind() == kind).collect();
            if siblings.len() == 1 {
                // The last member: the parent becomes empty, `{}` or `[]`.
                let empty = if kind == K::MappingEntry {
                    Y::Mapping(rivet_yaml::Mapping::new())
                } else {
                    Y::Sequence(Vec::new())
                };
                return y_set_splice(src, root, &steps[..steps.len() - 1], &empty, None);
            }
            let ms = usize::from(m.text_range().start());
            let ls = line_start(src, ms);
            if src[ls..ms].trim().is_empty() {
                // The member owns its lines: remove them whole.
                let end = line_end(src, sig_end(&m).max(ms));
                Ok(Splice {
                    start: ls,
                    end,
                    text: String::new(),
                })
            } else {
                // `- a: 1` with more keys below: pull the next key up.
                let pos = siblings.iter().position(|s| s == &m).unwrap_or(0);
                match siblings.get(pos + 1) {
                    Some(next) if pos == 0 => Ok(Splice {
                        start: ms,
                        end: usize::from(next.text_range().start()),
                        text: String::new(),
                    }),
                    _ => Err(format!(
                        "{shown} shares its line with other YAML; delete it with `edit`"
                    )),
                }
            }
        }
        Found::Here { member: None, .. } => Err("cannot delete the whole document".to_string()),
        Found::MissingKey { .. } => Err(format!("{shown}: no such key")),
        Found::Append { .. } => Err(format!("{shown}: index out of range")),
    }
}

fn y_parse(ctx: &mut Ctx, what: &str, text: &str) -> Option<Y> {
    match rivet_yaml::from_str::<Y>(text) {
        Ok(v) => Some(v),
        Err(e) => {
            ctx.msg(&format!("{what}: invalid YAML: {e}"));
            None
        }
    }
}

fn y_show(v: &Y, json: bool) -> String {
    if json {
        return rivet_yaml::json::to_json(v, true).unwrap_or_else(|e| format!("<{e}>"));
    }
    match v {
        Y::String(s) => s.clone(),
        Y::Null | Y::Bool(_) | Y::Number(_) => y_flow(v),
        _ => {
            let mut s = rivet_yaml::to_string(v).unwrap_or_default();
            if s.ends_with('\n') {
                s.pop();
            }
            s
        }
    }
}

pub fn yaml(ctx: &mut Ctx, a: &[String]) -> i32 {
    let cmd = match command(ctx, a, &["get", "set", "del"]) {
        Ok(c) => c,
        Err(st) => return st,
    };
    let as_json = cmd.opts.has("-j");
    if cmd.sub != "get" && as_json {
        ctx.msg("-j applies to get");
        return 2;
    }
    if cmd.sub == "get" {
        let Some(text) = get_input(ctx, &cmd.rest) else {
            return 1;
        };
        let what = cmd.rest.first().cloned().unwrap_or_else(|| "-".into());
        let Some(doc) = y_parse(ctx, &what, &text) else {
            return 1;
        };
        let mut found = Vec::new();
        if let Err(e) = y_get(&doc, &cmd.path, &[], &mut found) {
            ctx.msg(&e);
            return 1;
        }
        let mut out = String::new();
        for v in found {
            out.push_str(&y_show(v, as_json));
            out.push('\n');
        }
        ctx.puts(&out);
        return 0;
    }
    let file = cmd.rest.last().cloned().unwrap_or_default();
    let new = if cmd.sub == "set" {
        let v = &cmd.rest[0];
        if cmd.opts.has("--str") {
            Some(Y::String(v.clone()))
        } else {
            match rivet_yaml::from_str::<Y>(v) {
                // `#x` is a comment, so it reads as null: say so instead of
                // writing a null nobody asked for.
                Ok(Y::Null) if !["null", "Null", "NULL", "~"].contains(&v.trim()) => {
                    ctx.msg(&format!(
                        "VALUE '{v}' reads as null in YAML; use --str to set it as a string, or write null"
                    ));
                    return 2;
                }
                Ok(y) => Some(y),
                Err(e) => {
                    ctx.msg(&format!(
                        "VALUE '{v}' is not YAML ({e}); use --str to set it as a string"
                    ));
                    return 2;
                }
            }
        }
    } else {
        None
    };
    let Some((path, text)) = edit_input(ctx, &file) else {
        return 1;
    };
    let Some(before) = y_parse(ctx, &file, &text) else {
        return 1;
    };
    let (green, errs) = rivet_yaml::cst::parse(&text);
    if let Some(e) = errs.first() {
        ctx.msg(&format!(
            "{file}: the YAML has a construct kapsel does not edit ({}); file unchanged",
            e.message
        ));
        return 1;
    }
    let root = SyntaxNode::new_root(green);
    // A number, boolean or flow collection is written as typed, so `1.10`
    // does not become `1.1`; the read-back below still checks it.
    let lit = match &new {
        Some(Y::Number(_) | Y::Bool(_) | Y::Sequence(_) | Y::Mapping(_))
            if !cmd.opts.has("--str") =>
        {
            let t = cmd.rest[0].trim();
            (!t.contains('\n')
                && (matches!(new, Some(Y::Number(_) | Y::Bool(_))) || t.starts_with(['[', '{'])))
            .then_some(t)
        }
        _ => None,
    };
    let splice = match &new {
        Some(v) => y_set_splice(&text, &root, &cmd.path, v, lit),
        None => y_del_splice(&text, &root, &cmd.path),
    };
    let splice = match splice {
        Ok(s) => s,
        Err(e) => {
            ctx.msg(&format!("{file}: {e}; file unchanged"));
            return 1;
        }
    };
    let out = format!(
        "{}{}{}",
        &text[..splice.start],
        splice.text,
        &text[splice.end..]
    );
    // Read back: the result must be exactly the intended document.
    let mut want = before;
    if let Err(e) = y_apply(&mut want, &cmd.path, new.as_ref()) {
        ctx.msg(&format!(
            "{file}: {}: {e}; file unchanged",
            show_steps(&cmd.path)
        ));
        return 1;
    }
    match rivet_yaml::from_str::<Y>(super::readback_text(&out)) {
        Ok(got) if got == want => {}
        Ok(_) | Err(_) => {
            ctx.msg(&format!(
                "{file}: the edit did not read back as the intended document; file unchanged \
                 (edit the text with `edit FILE --lines`)"
            ));
            return 1;
        }
    }
    if out == text {
        return 0;
    }
    ctx.commit(&file, &path, out.as_bytes())
}

// ---- toml --------------------------------------------------------------------

use toml_edit::{DocumentMut, Item, Table, Value as TV};

fn t_show_value(v: &TV) -> String {
    match v {
        TV::String(s) => s.value().clone(),
        _ => v.clone().decorated("", "").to_string(),
    }
}

fn t_get(item: &Item, steps: &[Step], done: &[Step], out: &mut Vec<String>) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        out.push(match item {
            Item::Value(v) => t_show_value(v),
            Item::Table(t) => t.to_string().trim_end().to_string(),
            Item::ArrayOfTables(a) => a.to_string().trim_end().to_string(),
            Item::None => String::new(),
        });
        return Ok(());
    };
    let mut next_done = done.to_vec();
    next_done.push(st.clone());
    let missing = || format!("{}: no such key", show_steps(&next_done));
    match st {
        Step::Key(k) => match item.as_table_like() {
            Some(t) => match t.get(k) {
                Some(c) => t_get(c, rest, &next_done, out),
                None => Err(missing()),
            },
            None => Err(format!(
                "{}: {} cannot be indexed with .{k}",
                show_steps(done),
                item.type_name()
            )),
        },
        Step::Index(i) => {
            let len = match item {
                Item::ArrayOfTables(a) => a.len(),
                Item::Value(TV::Array(a)) => a.len(),
                _ => {
                    return Err(format!(
                        "{}: {} cannot be indexed with [{i}]",
                        show_steps(done),
                        item.type_name()
                    ));
                }
            };
            match norm_index(*i, len).and_then(|j| item.get(j)) {
                Some(c) => t_get(c, rest, &next_done, out),
                None => Err(format!(
                    "{}: index out of range (length {len})",
                    show_steps(&next_done)
                )),
            }
        }
        Step::Each => {
            if let Some(t) = item.as_table_like() {
                for (_, c) in t.iter() {
                    t_get(c, rest, &next_done, out)?;
                }
                return Ok(());
            }
            let len = match item {
                Item::ArrayOfTables(a) => a.len(),
                Item::Value(TV::Array(a)) => a.len(),
                _ => 0,
            };
            for j in 0..len {
                if let Some(c) = item.get(j) {
                    t_get(c, rest, &next_done, out)?;
                }
            }
            Ok(())
        }
    }
}

/// Replaces `slot` with `new`, keeping the old value's comments and spacing.
fn t_put(slot: &mut Item, mut new: TV) {
    if let Item::Value(old) = slot {
        *new.decor_mut() = old.decor().clone();
    }
    *slot = Item::Value(new);
}

fn t_set_value(v: &mut TV, steps: &[Step], new: TV, here: &str) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        let mut new = new;
        *new.decor_mut() = v.decor().clone();
        *v = new;
        return Ok(());
    };
    match (st, v) {
        (Step::Key(k), TV::InlineTable(t)) => match t.get_mut(k) {
            Some(c) => t_set_value(c, rest, new, here),
            None if rest.is_empty() => {
                t.insert(k, new);
                Ok(())
            }
            None => {
                t.insert(k, TV::InlineTable(toml_edit::InlineTable::new()));
                let c = t.get_mut(k).expect("just inserted");
                t_set_value(c, rest, new, here)
            }
        },
        (Step::Index(i), TV::Array(a)) => {
            if rest.is_empty() && *i == a.len() as i64 {
                a.push(new);
                return Ok(());
            }
            let len = a.len();
            match norm_index(*i, len).and_then(|j| a.get_mut(j)) {
                Some(c) => t_set_value(c, rest, new, here),
                None => Err(format!("{here}: index out of range (length {len})")),
            }
        }
        (st, v) => Err(format!(
            "{here}: {} cannot be indexed with {}",
            v.type_name(),
            show_steps(std::slice::from_ref(st))
        )),
    }
}

fn t_set(item: &mut Item, steps: &[Step], new: TV, done: &mut Vec<Step>) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        t_put(item, new);
        return Ok(());
    };
    if let Item::Value(v) = item {
        return t_set_value(v, steps, new, &show_steps(done));
    }
    let here = show_steps(done);
    done.push(st.clone());
    match st {
        Step::Key(k) => {
            let Some(t) = item.as_table_like_mut() else {
                return Err(format!(
                    "{here}: {} cannot be indexed with .{k}",
                    item.type_name()
                ));
            };
            if t.get(k).is_none() {
                if rest.is_empty() {
                    t.insert(k, Item::Value(new));
                    return Ok(());
                }
                let mut nt = Table::new();
                nt.set_implicit(true);
                t.insert(k, Item::Table(nt));
            }
            let c = t.get_mut(k).expect("present");
            t_set(c, rest, new, done)
        }
        Step::Index(i) => match item {
            Item::ArrayOfTables(a) => {
                let len = a.len();
                let Some(tab) = norm_index(*i, len).and_then(|j| a.get_mut(j)) else {
                    return Err(format!(
                        "{}: index out of range (length {len})",
                        show_steps(done)
                    ));
                };
                if rest.is_empty() {
                    return Err(format!(
                        "{}: is a table; set one of its keys",
                        show_steps(done)
                    ));
                }
                let mut wrapped = Item::Table(std::mem::take(tab));
                let r = t_set(&mut wrapped, rest, new, done);
                if let Item::Table(t) = wrapped {
                    *tab = t;
                }
                r
            }
            _ => Err(format!(
                "{here}: {} cannot be indexed with [{i}]",
                item.type_name()
            )),
        },
        Step::Each => Err("`[]` selects many values".to_string()),
    }
}

pub fn toml(ctx: &mut Ctx, a: &[String]) -> i32 {
    let cmd = match command(ctx, a, &["get", "set"]) {
        Ok(c) => c,
        Err(st) => return st,
    };
    if cmd.sub == "get" {
        let Some(text) = get_input(ctx, &cmd.rest) else {
            return 1;
        };
        let what = cmd.rest.first().cloned().unwrap_or_else(|| "-".into());
        let doc = match text.parse::<DocumentMut>() {
            Ok(d) => d,
            Err(e) => {
                ctx.msg(&format!("{what}: invalid TOML: {}", e.message()));
                return 1;
            }
        };
        let mut found = Vec::new();
        if let Err(e) = t_get(doc.as_item(), &cmd.path, &[], &mut found) {
            ctx.msg(&e);
            return 1;
        }
        let mut out = String::new();
        for s in found {
            out.push_str(&s);
            out.push('\n');
        }
        ctx.puts(&out);
        return 0;
    }
    let (v, file) = (cmd.rest[0].clone(), cmd.rest[1].clone());
    let new = if cmd.opts.has("--str") {
        TV::from(v.as_str())
    } else {
        match v.parse::<TV>() {
            Ok(t) => t,
            Err(_) => {
                ctx.msg(&format!(
                    "VALUE '{v}' is not a TOML value; use --str to set it as a string"
                ));
                return 2;
            }
        }
    };
    if cmd.path.is_empty() {
        ctx.msg("set needs a key path, e.g. .package.version");
        return 2;
    }
    let Some((path, text)) = edit_input(ctx, &file) else {
        return 1;
    };
    let mut doc = match text.parse::<DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            ctx.msg(&format!("{file}: invalid TOML: {}", e.message()));
            return 1;
        }
    };
    let shown = new.clone().decorated("", "").to_string();
    if let Err(e) = t_set(doc.as_item_mut(), &cmd.path, new, &mut Vec::new()) {
        ctx.msg(&format!("{file}: {e}; file unchanged"));
        return 1;
    }
    let out = doc.to_string();
    // Read back: the file parses and the path holds the new value.
    let back = super::readback_text(&out)
        .parse::<DocumentMut>()
        .ok()
        .and_then(|d| {
            let mut got = Vec::new();
            t_get(d.as_item(), &cmd.path, &[], &mut got).ok()?;
            got.pop()
        });
    let want = match shown.parse::<TV>() {
        Ok(TV::String(s)) => s.value().clone(),
        _ => shown,
    };
    if back.as_deref() != Some(want.as_str()) {
        ctx.msg(&format!(
            "{file}: the edit did not read back as {}; file unchanged",
            show_steps(&cmd.path)
        ));
        return 1;
    }
    if out == text {
        return 0;
    }
    ctx.commit(&file, &path, out.as_bytes())
}
