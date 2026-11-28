//! `json` (REQ-KS-063, DD-KS-020): get, set, del and merge by path, by byte
//! range.
//!
//! A small lossless reader records the byte range of every value. `set` and
//! `del` splice only the target's bytes (and, for `del`, the comma beside
//! it); a new key or element copies the separator and indentation of its
//! siblings. Everything else in the file, numbers like `1.10` or `1e400`
//! included, stays byte for byte. The result is read back and compared with
//! the intended document before the file is replaced; a mismatch leaves it
//! untouched. `get` prints numbers as written. `merge` is the one form that
//! rewrites the file in a normal layout (its usage line says so); it too
//! keeps every number's text.

use super::Ctx;
use super::data::{Step, command, edit_input, get_input, line_start, norm_index, show_steps};

// ---- lossless reader -----------------------------------------------------------

/// A value and its byte range in the source.
struct N {
    start: usize,
    end: usize,
    v: NV,
}

enum NV {
    /// `true`, `false`, `null`.
    Lit,
    Num,
    Str(String),
    Arr(Vec<N>),
    Obj(Vec<Member>),
}

struct Member {
    key: String,
    kstart: usize,
    kend: usize,
    v: N,
}

/// A value with numbers and literals kept as their source text. Two
/// documents are the same when their `JV`s are equal: `1.10` and `1.1` differ.
#[derive(Clone, Debug, PartialEq)]
enum JV {
    Lit(String),
    Num(String),
    Str(String),
    Arr(Vec<JV>),
    Obj(Vec<(String, JV)>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

const MAX_DEPTH: usize = 256;

impl P<'_> {
    fn ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn err(&self, what: &str) -> String {
        let at = self.i.min(self.s.len());
        let before = &self.s[..at];
        let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
        let col = at
            - before
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |p| p + 1)
            + 1;
        format!("{what} at line {line} column {col}")
    }

    fn value(&mut self) -> Result<N, String> {
        self.ws();
        let start = self.i;
        let v = match self.s.get(self.i) {
            None => return Err(self.err("unexpected end of input")),
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => NV::Str(self.string()?),
            Some(b't' | b'f' | b'n') => {
                let rest = &self.s[self.i..];
                let lit: &[u8] = [b"true".as_slice(), b"false", b"null"]
                    .into_iter()
                    .find(|l| rest.starts_with(l))
                    .ok_or_else(|| self.err("expected a value"))?;
                self.i += lit.len();
                NV::Lit
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                self.number()?;
                NV::Num
            }
            Some(_) => return Err(self.err("expected a value")),
        };
        Ok(N {
            start,
            end: self.i,
            v,
        })
    }

    fn digits(&mut self) -> usize {
        let s = self.i;
        while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
            self.i += 1;
        }
        self.i - s
    }

    /// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`
    fn number(&mut self) -> Result<(), String> {
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(c) if c.is_ascii_digit() => {
                self.digits();
            }
            _ => return Err(self.err("invalid number")),
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err(self.err("invalid number"));
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(self.err("invalid number"));
            }
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, String> {
        let start = self.i;
        self.i += 1;
        loop {
            match self.s.get(self.i) {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => break,
                Some(b'\\') => self.i += 2,
                Some(c) if *c < 0x20 => return Err(self.err("control character in string")),
                Some(_) => self.i += 1,
            }
        }
        self.i += 1;
        let raw = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.err("not UTF-8"))?;
        serde_json::from_str::<String>(raw).map_err(|_| {
            let p = P {
                s: self.s,
                i: start,
                depth: 0,
            };
            p.err("invalid string escape")
        })
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("nested too deeply"));
        }
        self.i += 1;
        Ok(())
    }

    fn array(&mut self) -> Result<NV, String> {
        self.enter()?;
        let mut items = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            self.depth -= 1;
            return Ok(NV::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(NV::Arr(items));
                }
                _ => return Err(self.err("expected `,` or `]`")),
            }
        }
    }

    fn object(&mut self) -> Result<NV, String> {
        self.enter()?;
        let mut members = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            self.depth -= 1;
            return Ok(NV::Obj(members));
        }
        loop {
            self.ws();
            if self.s.get(self.i) != Some(&b'"') {
                return Err(self.err("expected a key"));
            }
            let kstart = self.i;
            let key = self.string()?;
            let kend = self.i;
            self.ws();
            if self.s.get(self.i) != Some(&b':') {
                return Err(self.err("expected `:`"));
            }
            self.i += 1;
            let v = self.value()?;
            members.push(Member {
                key,
                kstart,
                kend,
                v,
            });
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(NV::Obj(members));
                }
                _ => return Err(self.err("expected `,` or `}`")),
            }
        }
    }
}

/// Parses a whole JSON document.
fn parse(src: &str) -> Result<N, String> {
    let mut p = P {
        s: src.as_bytes(),
        i: 0,
        depth: 0,
    };
    let n = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(n)
}

fn to_jv(src: &str, n: &N) -> JV {
    match &n.v {
        NV::Lit => JV::Lit(src[n.start..n.end].to_string()),
        NV::Num => JV::Num(src[n.start..n.end].to_string()),
        NV::Str(s) => JV::Str(s.clone()),
        NV::Arr(a) => JV::Arr(a.iter().map(|c| to_jv(src, c)).collect()),
        NV::Obj(m) => JV::Obj(
            m.iter()
                .map(|mb| (mb.key.clone(), to_jv(src, &mb.v)))
                .collect(),
        ),
    }
}

fn kind(v: &JV) -> &'static str {
    match v {
        JV::Lit(s) if s == "null" => "null",
        JV::Lit(_) => "a boolean",
        JV::Num(_) => "a number",
        JV::Str(_) => "a string",
        JV::Arr(_) => "an array",
        JV::Obj(_) => "an object",
    }
}

fn is_null(v: &JV) -> bool {
    matches!(v, JV::Lit(s) if s == "null")
}

/// The last member with this key (as serde and jq read duplicates).
fn find_key<T>(m: &[T], key: impl Fn(&T) -> &str, k: &str) -> Option<usize> {
    m.iter().rposition(|x| key(x) == k)
}

// ---- writing -------------------------------------------------------------------

struct Style {
    /// None: one line.
    indent: Option<String>,
    colon: String,
    comma: String,
}

impl Style {
    fn pretty() -> Style {
        Style {
            indent: Some("  ".to_string()),
            colon: ": ".to_string(),
            comma: ", ".to_string(),
        }
    }
    fn compact() -> Style {
        Style {
            indent: None,
            colon: ":".to_string(),
            comma: ",".to_string(),
        }
    }
    fn inline(&self) -> Style {
        Style {
            indent: None,
            colon: self.colon.clone(),
            comma: self.comma.clone(),
        }
    }
}

/// The layout a file already uses: its indent unit, and its `:` and `,`
/// spacing on one line.
fn file_style(src: &str, root: &N) -> Style {
    let body = src.trim_end();
    let indent = body.find('\n').map(|i| {
        let ws: String = body[i + 1..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if ws.is_empty() { "  ".to_string() } else { ws }
    });
    fn first<'a>(n: &'a N, f: &dyn Fn(&'a N) -> Option<String>) -> Option<String> {
        if let Some(s) = f(n) {
            return Some(s);
        }
        match &n.v {
            NV::Arr(a) => a.iter().find_map(|c| first(c, f)),
            NV::Obj(m) => m.iter().find_map(|mb| first(&mb.v, f)),
            _ => None,
        }
    }
    let colon = first(root, &|n| match &n.v {
        NV::Obj(m) => m
            .iter()
            .map(|mb| &src[mb.kend..mb.v.start])
            .find(|g| !g.contains('\n'))
            .map(str::to_string),
        _ => None,
    })
    .unwrap_or_else(|| if indent.is_some() { ": " } else { ":" }.to_string());
    let gap = |a: usize, b: usize| Some(&src[a..b]).filter(|g| !g.contains('\n'));
    let comma = first(root, &|n| match &n.v {
        NV::Arr(a) if a.len() > 1 => gap(a[0].end, a[1].start).map(str::to_string),
        NV::Obj(m) if m.len() > 1 => gap(m[0].v.end, m[1].kstart).map(str::to_string),
        _ => None,
    })
    .unwrap_or_else(|| if colon.ends_with(' ') { ", " } else { "," }.to_string());
    Style {
        indent,
        colon,
        comma,
    }
}

fn jstr(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// Writes `v`; nested lines start with `base` plus indent units.
fn render(v: &JV, st: &Style, base: &str) -> String {
    let mut out = String::new();
    write(&mut out, v, st, base);
    out
}

fn write(out: &mut String, v: &JV, st: &Style, base: &str) {
    match v {
        JV::Lit(s) | JV::Num(s) => out.push_str(s),
        JV::Str(s) => out.push_str(&jstr(s)),
        JV::Arr(a) if a.is_empty() => out.push_str("[]"),
        JV::Obj(m) if m.is_empty() => out.push_str("{}"),
        JV::Arr(a) => {
            out.push('[');
            match &st.indent {
                None => {
                    for (i, c) in a.iter().enumerate() {
                        if i > 0 {
                            out.push_str(&st.comma);
                        }
                        write(out, c, st, base);
                    }
                }
                Some(u) => {
                    let inner = format!("{base}{u}");
                    for (i, c) in a.iter().enumerate() {
                        out.push_str(if i > 0 { ",\n" } else { "\n" });
                        out.push_str(&inner);
                        write(out, c, st, &inner);
                    }
                    out.push('\n');
                    out.push_str(base);
                }
            }
            out.push(']');
        }
        JV::Obj(m) => {
            out.push('{');
            match &st.indent {
                None => {
                    for (i, (k, c)) in m.iter().enumerate() {
                        if i > 0 {
                            out.push_str(&st.comma);
                        }
                        out.push_str(&jstr(k));
                        out.push_str(&st.colon);
                        write(out, c, st, base);
                    }
                }
                Some(u) => {
                    let inner = format!("{base}{u}");
                    for (i, (k, c)) in m.iter().enumerate() {
                        out.push_str(if i > 0 { ",\n" } else { "\n" });
                        out.push_str(&inner);
                        out.push_str(&jstr(k));
                        out.push_str(&st.colon);
                        write(out, c, st, &inner);
                    }
                    out.push('\n');
                    out.push_str(base);
                }
            }
            out.push('}');
        }
    }
}

/// The blanks at the start of the line holding `at`.
fn line_indent(src: &str, at: usize) -> String {
    src[line_start(src, at)..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

// ---- the model: what an edit must produce --------------------------------------

fn j_get<'a>(
    v: &'a JV,
    steps: &[Step],
    done: &[Step],
    out: &mut Vec<&'a JV>,
) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        out.push(v);
        return Ok(());
    };
    let mut next = done.to_vec();
    next.push(st.clone());
    match (st, v) {
        (Step::Key(k), JV::Obj(m)) => match find_key(m, |x| &x.0, k) {
            Some(i) => j_get(&m[i].1, rest, &next, out),
            None => Err(format!("{}: no such key", show_steps(&next))),
        },
        (Step::Index(i), JV::Arr(a)) => match norm_index(*i, a.len()) {
            Some(j) => j_get(&a[j], rest, &next, out),
            None => Err(format!(
                "{}: index out of range (length {})",
                show_steps(&next),
                a.len()
            )),
        },
        (Step::Each, JV::Arr(a)) => a.iter().try_for_each(|c| j_get(c, rest, &next, out)),
        (Step::Each, JV::Obj(m)) => m.iter().try_for_each(|(_, c)| j_get(c, rest, &next, out)),
        (st, v) => Err(format!(
            "{}: {} cannot be indexed with {}",
            show_steps(done),
            kind(v),
            show_steps(std::slice::from_ref(st))
        )),
    }
}

/// The value missing steps build: from `.a.b` on, `{"b": v}` under `a`.
fn nest(steps: &[Step], v: JV, at: &[Step]) -> Result<JV, String> {
    let mut cur = v;
    for st in steps.iter().rev() {
        cur = match st {
            Step::Key(k) => JV::Obj(vec![(k.clone(), cur)]),
            Step::Index(0) => JV::Arr(vec![cur]),
            _ => {
                return Err(format!(
                    "{}: a missing array can only be created at index 0",
                    show_steps(at)
                ));
            }
        };
    }
    Ok(cur)
}

/// Applies set (Some) or del (None) to the model.
fn apply(v: &mut JV, steps: &[Step], new: Option<&JV>, done: &mut Vec<Step>) -> Result<(), String> {
    let Some((st, rest)) = steps.split_first() else {
        return match new {
            Some(n) => {
                *v = n.clone();
                Ok(())
            }
            None => Err("cannot delete the whole document".to_string()),
        };
    };
    let here = show_steps(done);
    done.push(st.clone());
    if let (Some(n), true, Step::Key(_)) = (new, is_null(v), st) {
        *v = nest(steps, n.clone(), done)?;
        return Ok(());
    }
    match (st, v) {
        (Step::Key(k), JV::Obj(m)) => match (find_key(m, |x| &x.0, k), new) {
            (Some(i), None) if rest.is_empty() => {
                m.remove(i);
                Ok(())
            }
            (Some(i), _) => apply(&mut m[i].1, rest, new, done),
            (None, Some(n)) => {
                m.push((k.clone(), nest(rest, n.clone(), done)?));
                Ok(())
            }
            (None, None) => Err(format!("{}: no such key", show_steps(done))),
        },
        (Step::Index(i), JV::Arr(a)) => {
            if let (Some(n), true) = (new, *i == a.len() as i64) {
                a.push(nest(rest, n.clone(), done)?);
                return Ok(());
            }
            match norm_index(*i, a.len()) {
                Some(j) if new.is_none() && rest.is_empty() => {
                    a.remove(j);
                    Ok(())
                }
                Some(j) => apply(&mut a[j], rest, new, done),
                None => Err(format!(
                    "{}: index out of range (length {}{})",
                    show_steps(done),
                    a.len(),
                    if new.is_some() {
                        "; set at the length appends"
                    } else {
                        ""
                    }
                )),
            }
        }
        (st, v) => Err(format!(
            "{here}: {} cannot be indexed with {}",
            kind(v),
            show_steps(std::slice::from_ref(st))
        )),
    }
}

/// Deep merge: objects merge key by key (OTHER wins), anything else is replaced.
fn merge(a: &mut JV, b: JV) {
    match (a, b) {
        (JV::Obj(am), JV::Obj(bm)) => {
            for (k, bv) in bm {
                match find_key(am, |x| &x.0, &k) {
                    Some(i) => merge(&mut am[i].1, bv),
                    None => am.push((k, bv)),
                }
            }
        }
        (a, b) => *a = b,
    }
}

// ---- splicing ------------------------------------------------------------------

struct Splice {
    start: usize,
    end: usize,
    text: String,
}

/// Where a path lands in the source.
enum Loc<'a> {
    /// The value exists; `parent` is its container and its index there.
    Here {
        node: &'a N,
        parent: Option<(&'a N, usize)>,
    },
    /// A key is missing from an object, or the path continues below a
    /// `null` (`from` is the first step not found).
    Missing { node: &'a N, from: usize },
    /// Index = length of an array.
    Append { node: &'a N, from: usize },
}

fn locate<'a>(src: &str, root: &'a N, steps: &[Step], for_set: bool) -> Result<Loc<'a>, String> {
    let mut cur = root;
    let mut parent = None;
    for (i, st) in steps.iter().enumerate() {
        let here = show_steps(&steps[..i]);
        match (st, &cur.v) {
            (Step::Key(k), NV::Obj(m)) => match find_key(m, |x| &x.key, k) {
                Some(j) => {
                    parent = Some((cur, j));
                    cur = &m[j].v;
                }
                None if for_set => return Ok(Loc::Missing { node: cur, from: i }),
                None => return Err(format!("{}: no such key", show_steps(&steps[..=i]))),
            },
            (Step::Index(ix), NV::Arr(a)) => {
                if for_set && *ix == a.len() as i64 {
                    return Ok(Loc::Append { node: cur, from: i });
                }
                match norm_index(*ix, a.len()) {
                    Some(j) => {
                        parent = Some((cur, j));
                        cur = &a[j];
                    }
                    None => {
                        return Err(format!(
                            "{}: index out of range (length {})",
                            show_steps(&steps[..=i]),
                            a.len()
                        ));
                    }
                }
            }
            (Step::Key(_), NV::Lit) if for_set && &src[cur.start..cur.end] == "null" => {
                return Ok(Loc::Missing { node: cur, from: i });
            }
            (st, _) => {
                return Err(format!(
                    "{here}: {} cannot be indexed with {}",
                    kind(&to_jv(src, cur)),
                    show_steps(std::slice::from_ref(st))
                ));
            }
        }
    }
    Ok(Loc::Here { node: cur, parent })
}

/// Children's (start, end) byte ranges: a member runs from its key.
fn spans(n: &N) -> Vec<(usize, usize)> {
    match &n.v {
        NV::Arr(a) => a.iter().map(|c| (c.start, c.end)).collect(),
        NV::Obj(m) => m.iter().map(|mb| (mb.kstart, mb.v.end)).collect(),
        _ => Vec::new(),
    }
}

/// Inserts one entry (`"k": v` or `v`) after a container's last child,
/// copying the separator and indentation its children already use.
fn insert_child(src: &str, c: &N, st: &Style, key: Option<&str>, v: &JV) -> Splice {
    let kids = spans(c);
    let colon = match &c.v {
        NV::Obj(m) => m
            .iter()
            .map(|mb| &src[mb.kend..mb.v.start])
            .find(|g| !g.contains('\n'))
            .unwrap_or(&st.colon)
            .to_string(),
        _ => st.colon.clone(),
    };
    let entry = |indent: &str, inline: bool| {
        let vst = if inline {
            st.inline()
        } else {
            Style {
                indent: st.indent.clone(),
                colon: colon.clone(),
                comma: st.comma.clone(),
            }
        };
        let val = render(v, &vst, indent);
        match key {
            Some(k) => format!("{}{colon}{val}", jstr(k)),
            None => val,
        }
    };
    if kids.is_empty() {
        let open = line_indent(src, c.start);
        let (o, cl) = (&src[c.start..=c.start], &src[c.end - 1..c.end]);
        let text = match &st.indent {
            Some(u) => {
                let inner = format!("{open}{u}");
                format!("{o}\n{inner}{}\n{open}{cl}", entry(&inner, false))
            }
            None => format!("{o}{}{cl}", entry("", true)),
        };
        return Splice {
            start: c.start,
            end: c.end,
            text,
        };
    }
    let sep = if kids.len() > 1 {
        src[kids[0].1..kids[1].0].to_string()
    } else {
        let lead = &src[c.start + 1..kids[0].0];
        if lead.contains('\n') {
            format!(",{lead}")
        } else {
            st.comma.clone()
        }
    };
    let last = kids[kids.len() - 1];
    let inline = !sep.contains('\n');
    let indent = line_indent(src, last.0);
    Splice {
        start: last.1,
        end: last.1,
        text: format!("{sep}{}", entry(&indent, inline)),
    }
}

fn set_splice(src: &str, root: &N, steps: &[Step], new: &JV, st: &Style) -> Result<Splice, String> {
    let at = |from: usize| steps[..=from].to_vec();
    match locate(src, root, steps, true)? {
        Loc::Here { node, parent } => {
            let inline = match parent {
                Some((p, _)) => {
                    let k = spans(p);
                    !src[p.start..k.first().map_or(p.end, |x| x.0)].contains('\n')
                }
                None => st.indent.is_none(),
            };
            let vst = if inline {
                st.inline()
            } else {
                Style {
                    indent: st.indent.clone(),
                    colon: st.colon.clone(),
                    comma: st.comma.clone(),
                }
            };
            Ok(Splice {
                start: node.start,
                end: node.end,
                text: render(new, &vst, &line_indent(src, node.start)),
            })
        }
        Loc::Missing { node, from } => {
            let Step::Key(k) = &steps[from] else {
                return Err("expected a key".to_string());
            };
            let v = nest(&steps[from + 1..], new.clone(), &at(from))?;
            if matches!(node.v, NV::Lit) {
                // `null` becomes an object holding the rest of the path.
                let v = JV::Obj(vec![(k.clone(), v)]);
                return Ok(Splice {
                    start: node.start,
                    end: node.end,
                    text: render(&v, &st.inline(), ""),
                });
            }
            Ok(insert_child(src, node, st, Some(k), &v))
        }
        Loc::Append { node, from } => {
            let v = nest(&steps[from + 1..], new.clone(), &at(from))?;
            Ok(insert_child(src, node, st, None, &v))
        }
    }
}

fn del_splice(src: &str, root: &N, steps: &[Step]) -> Result<Splice, String> {
    match locate(src, root, steps, false)? {
        Loc::Here {
            parent: Some((p, i)),
            ..
        } => {
            let kids = spans(p);
            let (start, end) = if kids.len() == 1 {
                // The only child: the container becomes `{}` or `[]`.
                (p.start + 1, p.end - 1)
            } else if i + 1 == kids.len() {
                // The last child takes the comma before it.
                (kids[i - 1].1, kids[i].1)
            } else {
                // Any other child takes the comma and blanks after it.
                (kids[i].0, kids[i + 1].0)
            };
            Ok(Splice {
                start,
                end,
                text: String::new(),
            })
        }
        Loc::Here { parent: None, .. } => Err("cannot delete the whole document".to_string()),
        Loc::Missing { .. } | Loc::Append { .. } => Err("no such path".to_string()),
    }
}

// ---- the command -----------------------------------------------------------------

fn parse_or_say(ctx: &mut Ctx, what: &str, text: &str) -> Option<N> {
    match parse(text) {
        Ok(n) => Some(n),
        Err(e) => {
            ctx.msg(&format!("{what}: invalid JSON: {e}"));
            None
        }
    }
}

pub fn json(ctx: &mut Ctx, a: &[String]) -> i32 {
    let cmd = match command(ctx, a, &["get", "set", "del", "merge"]) {
        Ok(c) => c,
        Err(st) => return st,
    };
    let compact = cmd.opts.has("-c") || cmd.opts.has("-cr") || cmd.opts.has("-rc");
    let raw = cmd.opts.has("-r") || cmd.opts.has("-cr") || cmd.opts.has("-rc");
    if cmd.sub != "get" && (compact || raw) {
        ctx.msg("-c and -r apply to get; an edit keeps the file's layout");
        return 2;
    }
    if cmd.sub == "get" {
        let Some(text) = get_input(ctx, &cmd.rest) else {
            return 1;
        };
        let what = cmd.rest.first().cloned().unwrap_or_else(|| "-".into());
        let Some(root) = parse_or_say(ctx, &what, &text) else {
            return 1;
        };
        let doc = to_jv(&text, &root);
        let mut found = Vec::new();
        if let Err(e) = j_get(&doc, &cmd.path, &[], &mut found) {
            ctx.msg(&e);
            return 1;
        }
        let st = if compact {
            Style::compact()
        } else {
            Style::pretty()
        };
        let mut out = String::new();
        for v in found {
            match v {
                JV::Str(s) if raw => out.push_str(s),
                _ => out.push_str(&render(v, &st, "")),
            }
            out.push('\n');
        }
        ctx.puts(&out);
        return 0;
    }
    let file = if cmd.sub == "merge" {
        cmd.rest[0].clone()
    } else {
        cmd.rest.last().cloned().unwrap_or_default()
    };
    // Read everything first: nothing is written unless all of it parses.
    let new = if cmd.sub == "set" {
        let v = &cmd.rest[0];
        if cmd.opts.has("--str") {
            Some(JV::Str(v.clone()))
        } else {
            match parse(v) {
                Ok(n) => Some(to_jv(v, &n)),
                Err(e) => {
                    ctx.msg(&format!(
                        "VALUE '{v}' is not JSON ({e}); use --str to set it as a string"
                    ));
                    return 2;
                }
            }
        }
    } else {
        None
    };
    let other = if cmd.sub == "merge" {
        let o = cmd.rest[1].clone();
        let Some(d) = ctx.read_input(&o) else {
            return 1;
        };
        let text = String::from_utf8_lossy(&d).into_owned();
        let Some(n) = parse_or_say(ctx, &o, &text) else {
            return 1;
        };
        Some(to_jv(&text, &n))
    } else {
        None
    };
    let Some((path, text)) = edit_input(ctx, &file) else {
        return 1;
    };
    let Some(root) = parse_or_say(ctx, &file, &text) else {
        return 1;
    };
    let style = file_style(&text, &root);
    let mut want = to_jv(&text, &root);
    let fail = |ctx: &mut Ctx, e: &str| {
        ctx.msg(&format!("{file}: {e}; file unchanged"));
        1
    };
    let out = if let Some(o) = other {
        // merge: the one form that rewrites the layout; numbers keep their text.
        merge(&mut want, o);
        let mut s = render(&want, &style, "");
        if text.ends_with('\n') {
            s.push('\n');
        }
        s
    } else {
        if let Err(e) = apply(&mut want, &cmd.path, new.as_ref(), &mut Vec::new()) {
            return fail(ctx, &e);
        }
        let sp = match &new {
            Some(n) => set_splice(&text, &root, &cmd.path, n, &style),
            None => del_splice(&text, &root, &cmd.path),
        };
        let sp = match sp {
            Ok(s) => s,
            Err(e) => return fail(ctx, &format!("{}: {e}", show_steps(&cmd.path))),
        };
        format!("{}{}{}", &text[..sp.start], sp.text, &text[sp.end..])
    };
    // Read back: the result must be exactly the intended document.
    match parse(&out) {
        Ok(n) if to_jv(&out, &n) == want => {}
        _ => {
            return fail(
                ctx,
                "the edit did not read back as the intended document (edit the text with `edit`)",
            );
        }
    }
    if out == text {
        return 0;
    }
    ctx.commit(&file, &path, out.as_bytes())
}
