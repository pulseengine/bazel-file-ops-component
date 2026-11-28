//! Session state, word expansion, redirects and built-in dispatch.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::HostError;
use crate::builtins;
use crate::grants::{self, GrantErr, Grants};
use crate::runerr::{ErrorClass, RunError, help_for};

/// A failed redirect: the class, the path concerned and the stderr line.
type RedirErr = (ErrorClass, Option<String>, String);

fn grant_class(w: GrantErr) -> ErrorClass {
    match w {
        GrantErr::Outside => ErrorClass::Grants,
        GrantErr::Symlink | GrantErr::Loop => ErrorClass::Symlink,
    }
}

pub type R<T> = Result<T, HostError>;

/// A cloneable, shared output stream (session stream, pipe or file).
#[derive(Clone)]
pub struct Sink(pub Rc<RefCell<dyn Write>>);

impl Sink {
    pub fn new<W: Write + 'static>(w: W) -> Self {
        Sink(Rc::new(RefCell::new(w)))
    }
    pub fn null() -> Self {
        Sink::new(io::sink())
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

/// An in-memory stream whose contents tests (and embedders) can read back.
#[derive(Clone, Default)]
pub struct SharedBuf(pub Rc<RefCell<Vec<u8>>>);

impl SharedBuf {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn contents(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }
    pub fn string(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Reads from the session's shared stdin.
struct SharedRead(Rc<RefCell<Box<dyn Read>>>);
impl Read for SharedRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.borrow_mut().read(buf)
    }
}

/// Parts of a word as received from the compiled plan.
#[derive(Debug, Clone, PartialEq)]
pub enum WPart {
    Lit(String, bool),
    Var(String, bool),
    Glob(String),
    Captured(u32, bool),
}
pub type WWord = Vec<WPart>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ROp {
    Read,
    Write,
    Append,
    DupOut,
    DupIn,
    Heredoc,
}

#[derive(Debug, Clone)]
pub struct WRedirect {
    pub fd: u32,
    pub op: ROp,
    pub target: WWord,
    pub target_fd: u32,
}

pub fn lit(s: &str) -> WWord {
    vec![WPart::Lit(s.to_string(), true)]
}

struct Scope {
    vars: BTreeMap<String, String>,
    exported: BTreeSet<String>,
    cwd: PathBuf,
    status: i32,
}

pub struct Shell {
    pub grants: Grants,
    pub vars: BTreeMap<String, String>,
    pub exported: BTreeSet<String>,
    pub cwd: PathBuf,
    pub status: i32,
    pub stdin: Rc<RefCell<Box<dyn Read>>>,
    pub stdout: Sink,
    pub stderr: Sink,
    pipes: Vec<Rc<RefCell<Vec<u8>>>>,
    lists: Vec<Vec<String>>,
    scopes: Vec<Scope>,
    pub exit_request: Option<i32>,
    /// The command named by the last WIT `site` call (REQ-KS-060).
    pub site: Option<u32>,
    /// Errors the running call recorded; kept only if it fails.
    pub pending_errors: Vec<RunError>,
    /// The run errors of the current (or last) run, in order.
    pub run_errors: Vec<RunError>,
}

impl Shell {
    pub fn new(grants: Grants, cwd: PathBuf) -> Self {
        Shell {
            grants,
            vars: BTreeMap::new(),
            exported: BTreeSet::new(),
            cwd,
            status: 0,
            stdin: Rc::new(RefCell::new(Box::new(io::empty()))),
            stdout: Sink::new(io::stdout()),
            stderr: Sink::new(io::stderr()),
            pipes: Vec::new(),
            lists: Vec::new(),
            scopes: Vec::new(),
            exit_request: None,
            site: None,
            pending_errors: Vec::new(),
            run_errors: Vec::new(),
        }
    }

    /// Clears the per-run state (pipes, lists, open scopes).
    pub fn begin_run(&mut self) {
        self.pipes.clear();
        self.lists.clear();
        self.scopes.clear();
        self.exit_request = None;
        self.site = None;
        self.pending_errors.clear();
        self.run_errors.clear();
    }

    /// Drops the per-run tables once a run is over.
    pub fn end_run(&mut self) {
        self.pipes.clear();
        self.lists.clear();
        self.scopes.clear();
    }

    /// Subshell semantics: snapshot variables, exported names, cwd and `$?`.
    /// Pipes and lists are per run, so they survive a pop.
    pub fn scope_push(&mut self) {
        self.scopes.push(Scope {
            vars: self.vars.clone(),
            exported: self.exported.clone(),
            cwd: self.cwd.clone(),
            status: self.status,
        });
    }

    pub fn scope_pop(&mut self) -> R<()> {
        let s = self
            .scopes
            .pop()
            .ok_or_else(|| HostError("scope-pop without scope-push".into()))?;
        self.vars = s.vars;
        self.exported = s.exported;
        self.cwd = s.cwd;
        self.status = s.status;
        Ok(())
    }

    // ---- host imports -------------------------------------------------

    pub fn new_pipe(&mut self) -> u32 {
        self.pipes.push(Rc::new(RefCell::new(Vec::new())));
        self.pipes.len() as u32
    }

    pub fn pipe_write(&mut self, h: u32, bytes: &[u8]) {
        self.pipes[h as usize - 1]
            .borrow_mut()
            .extend_from_slice(bytes);
    }

    pub fn pipe_read(&self, h: u32) -> Vec<u8> {
        self.pipes[h as usize - 1].borrow().clone()
    }

    fn pipe(&self, h: u32) -> R<Rc<RefCell<Vec<u8>>>> {
        if h == 0 || h as usize > self.pipes.len() {
            return Err(HostError(format!("bad pipe handle {h}")));
        }
        Ok(self.pipes[h as usize - 1].clone())
    }

    pub fn set_var(&mut self, name: &str, value: &WWord) -> R<()> {
        let v = self.expand_nosplit(value)?;
        self.vars.insert(name.to_string(), v);
        Ok(())
    }

    pub fn expand_list(&mut self, words: &[WWord]) -> R<(u32, u32)> {
        let mut items = Vec::new();
        for w in words {
            items.extend(self.expand_fields(w)?);
        }
        let n = items.len() as u32;
        self.lists.push(items);
        Ok((self.lists.len() as u32, n))
    }

    pub fn bind_item(&mut self, name: &str, list: u32, index: u32) -> R<()> {
        let item = self
            .lists
            .get((list as usize).wrapping_sub(1))
            .and_then(|l| l.get(index as usize))
            .ok_or_else(|| HostError(format!("bad list item {list}[{index}]")))?
            .clone();
        self.vars.insert(name.to_string(), item);
        Ok(())
    }

    // ---- expansion -----------------------------------------------------

    pub fn var_value(&self, name: &str) -> String {
        if name == "?" {
            return self.status.to_string();
        }
        self.vars.get(name).cloned().unwrap_or_default()
    }

    fn captured(&self, h: u32) -> R<String> {
        let p = self.pipe(h)?;
        let s = String::from_utf8_lossy(&p.borrow()).into_owned();
        Ok(s.trim_end_matches('\n').to_string())
    }

    fn part_value(&self, p: &WPart) -> R<Option<(String, bool)>> {
        Ok(match p {
            WPart::Var(n, q) => Some((self.var_value(n), *q)),
            WPart::Captured(h, q) => Some((self.captured(*h)?, *q)),
            _ => None,
        })
    }

    /// Concatenates the parts; no field splitting, no globbing.
    pub fn expand_nosplit(&self, w: &WWord) -> R<String> {
        let mut out = String::new();
        for p in w {
            match p {
                WPart::Lit(t, _) => out.push_str(t),
                WPart::Glob(g) => out.push_str(g),
                other => out.push_str(&self.part_value(other)?.unwrap().0),
            }
        }
        Ok(out)
    }

    /// Full expansion of one word into fields.
    pub fn expand_fields(&self, w: &WWord) -> R<Vec<String>> {
        let ifs = self
            .vars
            .get("IFS")
            .cloned()
            .unwrap_or_else(|| " \t\n".to_string());
        let mut fields: Vec<Vec<(char, bool)>> = Vec::new();
        let mut cur: Vec<(char, bool)> = Vec::new();
        let mut exists = false;
        for p in w {
            match p {
                WPart::Lit(t, q) => {
                    if *q || !t.is_empty() {
                        exists = true;
                    }
                    cur.extend(t.chars().map(|c| (c, false)));
                }
                WPart::Glob(g) => {
                    exists = true;
                    cur.extend(g.chars().map(|c| (c, true)));
                }
                other => {
                    let (val, quoted) = self.part_value(other)?.unwrap();
                    if quoted {
                        exists = true;
                        cur.extend(val.chars().map(|c| (c, false)));
                    } else {
                        for c in val.chars() {
                            if ifs.contains(c) {
                                if c == ' ' || c == '\t' || c == '\n' {
                                    if exists {
                                        fields.push(std::mem::take(&mut cur));
                                        exists = false;
                                    }
                                } else {
                                    fields.push(std::mem::take(&mut cur));
                                    exists = false;
                                }
                            } else {
                                cur.push((c, false));
                                exists = true;
                            }
                        }
                    }
                }
            }
        }
        if exists {
            fields.push(cur);
        }
        let mut out = Vec::new();
        for f in fields {
            if f.iter()
                .any(|(c, active)| *active && matches!(c, '*' | '?' | '['))
            {
                let m = self.glob_expand(&f);
                if m.is_empty() {
                    out.push(f.iter().map(|(c, _)| *c).collect());
                } else {
                    out.extend(m);
                }
            } else {
                out.push(f.iter().map(|(c, _)| *c).collect());
            }
        }
        Ok(out)
    }

    fn glob_expand(&self, field: &[(char, bool)]) -> Vec<String> {
        let segs: Vec<&[(char, bool)]> = field.split(|(c, _)| *c == '/').collect();
        let mut cands: Vec<String> = vec![if field.first().map(|x| x.0) == Some('/') {
            "/".to_string()
        } else {
            String::new()
        }];
        let n = segs.len();
        for (i, seg) in segs.iter().enumerate() {
            let last = i + 1 == n;
            if seg.is_empty() {
                // leading slash (handled), double slash, or trailing slash
                if last && i > 0 {
                    cands.retain(|c| self.dir_ok(c));
                }
                continue;
            }
            let has_glob = seg.iter().any(|(c, a)| *a && matches!(c, '*' | '?' | '['));
            let mut next = Vec::new();
            for cand in &cands {
                if !has_glob {
                    let name: String = seg.iter().map(|x| x.0).collect();
                    let p = format!("{cand}{name}");
                    if last {
                        if self.exists_visible(&p) {
                            next.push(p);
                        }
                    } else {
                        next.push(format!("{p}/"));
                    }
                } else {
                    for name in self.list_dir(cand) {
                        if name.starts_with('.') && seg[0].0 != '.' {
                            continue;
                        }
                        let chars: Vec<char> = name.chars().collect();
                        if fnmatch(seg, &chars) {
                            let p = format!("{cand}{name}");
                            if last {
                                next.push(p);
                            } else if self.dir_ok(&p) {
                                next.push(format!("{p}/"));
                            }
                        }
                    }
                }
            }
            cands = next;
        }
        cands.sort();
        cands.dedup();
        cands.retain(|c| !c.is_empty());
        cands
    }

    /// A path outside every grant does not exist as far as globbing can tell:
    /// no stat runs on it. The entry itself is probed without following a
    /// final symlink.
    fn exists_visible(&self, p: &str) -> bool {
        match grants::resolve_nofollow_last(&self.cwd, Path::new(p)) {
            Ok(r) if self.grants.can_read(&r) => std::fs::symlink_metadata(&r).is_ok(),
            _ => false,
        }
    }

    fn dir_ok(&self, p: &str) -> bool {
        match self.grants.check_read(&self.cwd, p) {
            Ok(r) => r.is_dir(),
            Err(_) => false,
        }
    }

    fn list_dir(&self, cand: &str) -> Vec<String> {
        let arg = if cand.is_empty() { "." } else { cand };
        let Ok(resolved) = self.grants.check_read(&self.cwd, arg) else {
            return Vec::new();
        };
        let mut v: Vec<String> = match std::fs::read_dir(&resolved) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        };
        v.sort();
        v
    }

    // ---- calls ---------------------------------------------------------

    /// Runs one built-in. Returns the status, or 256 + n for `exit n`.
    pub fn call(
        &mut self,
        argv: &[WWord],
        assigns: &[(String, WWord)],
        redirects: &[WRedirect],
        stdin: u32,
        stdout: u32,
    ) -> R<u32> {
        let mut args = Vec::new();
        for w in argv {
            args.extend(self.expand_fields(w)?);
        }
        if args.is_empty() {
            return Ok(0);
        }
        let mut avals = Vec::new();
        for (n, w) in assigns {
            avals.push((n.clone(), self.expand_nosplit(w)?));
        }
        let mut stdin_r: Box<dyn Read> = if stdin == 0 {
            Box::new(SharedRead(self.stdin.clone()))
        } else {
            let bytes = self.pipe(stdin)?.borrow().clone();
            Box::new(Cursor::new(bytes))
        };
        let mut out = if stdout == 0 {
            self.stdout.clone()
        } else {
            Sink(self.pipe(stdout)?)
        };
        let mut err = self.stderr.clone();
        self.pending_errors.clear();

        // Redirects, in order.
        let mut redirect_failed = false;
        for r in redirects {
            if let Err((class, operand, msg)) =
                self.apply_redirect(r, &mut stdin_r, &mut out, &mut err)
            {
                err_line(&mut err, &msg);
                let help = help_for(class, &args[0], operand.as_deref(), None);
                self.pending_errors.push(RunError {
                    site: self.site,
                    builtin: args[0].clone(),
                    class,
                    message: msg,
                    operand,
                    help,
                });
                redirect_failed = true;
                break;
            }
        }
        let status = if redirect_failed {
            1
        } else {
            // Temporary assignments, restored afterwards.
            let saved: Vec<(String, Option<String>)> = avals
                .iter()
                .map(|(n, _)| (n.clone(), self.vars.get(n).cloned()))
                .collect();
            for (n, v) in &avals {
                self.vars.insert(n.clone(), v.clone());
            }
            let st = builtins::run(self, &args, stdin_r, out.clone(), err.clone());
            for (n, old) in saved {
                match old {
                    Some(v) => self.vars.insert(n, v),
                    None => self.vars.remove(&n),
                };
            }
            st
        };
        let _ = out.flush();
        let _ = err.flush();
        // A run error counts only when the call failed (REQ-KS-060).
        let pending = std::mem::take(&mut self.pending_errors);
        if status != 0 && status < 256 {
            self.run_errors.extend(pending);
        }
        if status >= 256 {
            self.exit_request = Some(status - 256);
        }
        Ok(status as u32)
    }

    fn apply_redirect(
        &self,
        r: &WRedirect,
        stdin: &mut Box<dyn Read>,
        out: &mut Sink,
        err: &mut Sink,
    ) -> Result<(), RedirErr> {
        let other = |class: ErrorClass, m: String| (class, None, m);
        match r.op {
            ROp::Heredoc => {
                let body = self
                    .expand_nosplit(&r.target)
                    .map_err(|e| other(ErrorClass::Failed, e.0))?;
                *stdin = Box::new(Cursor::new(body.into_bytes()));
            }
            ROp::DupOut => match (r.fd, r.target_fd) {
                (1, 2) => *out = err.clone(),
                (2, 1) => *err = out.clone(),
                (a, b) if a == b => {}
                (a, b) => {
                    return Err(other(
                        ErrorClass::Usage,
                        format!("kapsel: {a}>&{b}: unsupported file descriptor"),
                    ));
                }
            },
            ROp::DupIn => {
                if r.fd != r.target_fd {
                    return Err(other(
                        ErrorClass::Usage,
                        format!(
                            "kapsel: {}<&{}: unsupported file descriptor",
                            r.fd, r.target_fd
                        ),
                    ));
                }
            }
            ROp::Read | ROp::Write | ROp::Append => {
                let fields = self
                    .expand_fields(&r.target)
                    .map_err(|e| other(ErrorClass::Failed, e.0))?;
                if fields.len() != 1 {
                    let text = self.expand_nosplit(&r.target).unwrap_or_default();
                    return Err(other(
                        ErrorClass::Failed,
                        format!("kapsel: {text}: ambiguous redirect"),
                    ));
                }
                let path = &fields[0];
                if r.op == ROp::Read {
                    if r.fd != 0 {
                        return Err(other(
                            ErrorClass::Usage,
                            format!("kapsel: {}<{path}: unsupported file descriptor", r.fd),
                        ));
                    }
                    if path == "/dev/null" {
                        *stdin = Box::new(io::empty());
                        return Ok(());
                    }
                    let resolved = self.grants.check_read(&self.cwd, path).map_err(|w| {
                        (
                            grant_class(w),
                            Some(path.clone()),
                            format!("kapsel: {path}: {}", w.reason()),
                        )
                    })?;
                    let f = std::fs::File::open(&resolved).map_err(|e| {
                        (
                            ErrorClass::of_io(&e),
                            Some(path.clone()),
                            format!("kapsel: {path}: {}", errstr(&e)),
                        )
                    })?;
                    *stdin = Box::new(f);
                } else {
                    if r.fd != 1 && r.fd != 2 {
                        return Err(other(
                            ErrorClass::Usage,
                            format!("kapsel: {}>{path}: unsupported file descriptor", r.fd),
                        ));
                    }
                    let sink = if path == "/dev/null" {
                        Sink::null()
                    } else {
                        let resolved =
                            self.grants
                                .check_write(&self.cwd, path, true)
                                .map_err(|w| {
                                    (
                                        grant_class(w),
                                        Some(path.clone()),
                                        format!("kapsel: {path}: {}", w.reason()),
                                    )
                                })?;
                        let mut oo = std::fs::OpenOptions::new();
                        oo.create(true).write(true);
                        if r.op == ROp::Append {
                            oo.append(true);
                        } else {
                            oo.truncate(true);
                        }
                        let f = oo.open(&resolved).map_err(|e| {
                            (
                                ErrorClass::of_io(&e),
                                Some(path.clone()),
                                format!("kapsel: {path}: {}", errstr(&e)),
                            )
                        })?;
                        Sink::new(f)
                    };
                    if r.fd == 1 {
                        *out = sink;
                    } else {
                        *err = sink;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Writes one diagnostic line (`name: operand: reason`, a grant refusal, a
/// failed redirect) to a built-in's stderr, with control characters escaped
/// as the rendered diagnostics escape them ([`kapsel_diag::escape_controls`]),
/// so an operand or path holding ESC cannot drive the terminal. Every
/// diagnostic line the host writes goes through here; data a built-in writes
/// (to stdout, or to stderr by redirection) never does.
pub fn err_line(err: &mut impl Write, line: &str) {
    let _ = writeln!(err, "{}", kapsel_diag::escape_controls(line));
}

/// `strerror`-style text for an io error, as GNU tools print it.
pub fn errstr(e: &io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// fnmatch over (char, active) pattern elements; `active` marks unquoted glob chars.
pub fn fnmatch(p: &[(char, bool)], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(&('*', true)) => (0..=s.len()).any(|i| fnmatch(&p[1..], &s[i..])),
        Some(&('?', true)) => !s.is_empty() && fnmatch(&p[1..], &s[1..]),
        Some(&('[', true)) => {
            if s.is_empty() {
                return false;
            }
            // find the end of the bracket expression
            let mut i = 1;
            let neg = matches!(p.get(i), Some(&('!', _)) | Some(&('^', _)));
            if neg {
                i += 1;
            }
            let start = i;
            let mut end = None;
            let mut j = i;
            while j < p.len() {
                if p[j].0 == '[' && p.get(j + 1).map(|x| x.0) == Some(':') {
                    // skip a class
                    let mut k = j + 2;
                    while k + 1 < p.len() && !(p[k].0 == ':' && p[k + 1].0 == ']') {
                        k += 1;
                    }
                    j = k + 2;
                    continue;
                }
                if p[j].0 == ']' && j > start {
                    end = Some(j);
                    break;
                }
                j += 1;
            }
            let Some(end) = end else {
                return s[0] == '[' && fnmatch(&p[1..], &s[1..]);
            };
            let set: Vec<char> = p[start..end].iter().map(|x| x.0).collect();
            let c = s[0];
            let mut matched = false;
            let mut k = 0;
            while k < set.len() {
                if set[k] == '['
                    && set.get(k + 1) == Some(&':')
                    && let Some(close) = (k + 2..set.len().saturating_sub(1))
                        .find(|&m| set[m] == ':' && set[m + 1] == ']')
                {
                    let name: String = set[k + 2..close].iter().collect();
                    matched |= class_match(&name, c);
                    k = close + 2;
                    continue;
                }
                if k + 2 < set.len() && set[k + 1] == '-' {
                    if set[k] <= c && c <= set[k + 2] {
                        matched = true;
                    }
                    k += 3;
                } else {
                    if set[k] == c {
                        matched = true;
                    }
                    k += 1;
                }
            }
            matched != neg && fnmatch(&p[end + 1..], &s[1..])
        }
        Some(&(c, _)) => !s.is_empty() && s[0] == c && fnmatch(&p[1..], &s[1..]),
    }
}

pub fn class_match(name: &str, c: char) -> bool {
    match name {
        "alpha" => c.is_alphabetic(),
        "digit" => c.is_ascii_digit(),
        "alnum" => c.is_alphanumeric(),
        "upper" => c.is_uppercase(),
        "lower" => c.is_lowercase(),
        "space" => c.is_whitespace(),
        "blank" => c == ' ' || c == '\t',
        "punct" => c.is_ascii_punctuation(),
        "xdigit" => c.is_ascii_hexdigit(),
        "print" => !c.is_control(),
        "graph" => !c.is_control() && !c.is_whitespace(),
        "cntrl" => c.is_control(),
        _ => false,
    }
}
