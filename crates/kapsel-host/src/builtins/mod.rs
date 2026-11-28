//! The built-ins (REQ-KS-025). Each returns 0, 1 or 2, except `exit n`, which
//! returns 256 + n. Messages follow GNU: `name: operand: reason`.

use std::io::{Read, Write};
use std::path::PathBuf;

use crate::grants;
use crate::runerr::{ErrorClass, RunError, help_for};
use crate::shell::{Shell, Sink, err_line, errstr};

mod basic;
mod data;
mod edit;
mod files;
mod jsonedit;
mod regex_conv;
mod sed;
mod text;
mod text2;
mod text3;
mod timefmt;

pub struct Ctx<'a> {
    pub name: String,
    pub sh: &'a mut Shell,
    pub stdin: Box<dyn Read>,
    pub out: Sink,
    pub err: Sink,
    /// The built-in's supported flags, once its option parser has run: the
    /// help line of a usage error.
    pub supported: Option<String>,
}

impl Ctx<'_> {
    /// Records a run error (REQ-KS-060) for the current site. The host keeps
    /// it only if the call fails.
    pub fn record(&mut self, class: ErrorClass, operand: Option<&str>, message: String) {
        let help = help_for(class, &self.name, operand, self.supported.as_deref());
        self.record_with_help(class, operand, message, help);
    }

    pub fn record_with_help(
        &mut self,
        class: ErrorClass,
        operand: Option<&str>,
        message: String,
        help: String,
    ) {
        let e = RunError {
            site: self.sh.site,
            builtin: self.name.clone(),
            class,
            message,
            operand: operand.map(str::to_string),
            help,
        };
        self.sh.pending_errors.push(e);
    }

    /// `name: text` on stderr, recorded with the class [`classify`] gives it.
    pub fn msg(&mut self, text: &str) {
        self.fail(classify(text), None, text);
    }

    /// `name: text` on stderr, recorded as a run error of `class`.
    pub fn fail(&mut self, class: ErrorClass, operand: Option<&str>, text: &str) {
        let line = format!("{}: {}", self.name, text);
        err_line(&mut self.err, &line);
        self.record(class, operand, line);
    }

    /// `name: text` on stderr for an I/O error `e` on `operand`.
    pub fn io_fail(&mut self, operand: &str, text: &str, e: &std::io::Error) {
        self.fail(ErrorClass::of_io(e), Some(operand), text);
    }

    pub fn io_msg(&mut self, operand: &str, e: &std::io::Error) {
        let m = format!("{}: {}", operand, errstr(e));
        self.io_fail(operand, &m, e);
    }

    /// `kapsel: operand: reason` on stderr, recorded as `class`.
    pub fn deny_msg_text(&mut self, operand: &str, class: ErrorClass, reason: &str) {
        let line = format!("kapsel: {operand}: {reason}");
        err_line(&mut self.err, &line);
        self.record(class, Some(operand), line);
    }

    /// A ready `kapsel: path: reason` grant refusal line (from a tree copy):
    /// printed, and recorded by its reason.
    pub fn kapsel_line(&mut self, line: &str) {
        err_line(&mut self.err, line);
        let rest = line.strip_prefix("kapsel: ").unwrap_or(line);
        let (operand, reason) = rest.rsplit_once(": ").unwrap_or((rest, ""));
        let class = if reason == grants::GrantErr::Outside.reason() {
            ErrorClass::Grants
        } else {
            ErrorClass::Symlink
        };
        self.record(class, Some(operand), line.to_string());
    }

    pub fn deny_msg(&mut self, operand: &str, why: grants::GrantErr) {
        let line = format!("kapsel: {operand}: {}", why.reason());
        err_line(&mut self.err, &line);
        let class = match why {
            grants::GrantErr::Outside => ErrorClass::Grants,
            grants::GrantErr::Symlink | grants::GrantErr::Loop => ErrorClass::Symlink,
        };
        self.record(class, Some(operand), line);
    }

    /// Resolves a path for reading; prints the refusal and returns None when
    /// it is outside every grant (or has `..`, or loops).
    pub fn rpath(&mut self, arg: &str) -> Option<PathBuf> {
        match self.sh.grants.check_read(&self.sh.cwd, arg) {
            Ok(r) => Some(r),
            Err(w) => {
                self.deny_msg(arg, w);
                None
            }
        }
    }

    /// Resolves a path for writing. `follow_last` false keeps a final symlink
    /// itself (rm, mv source); true refuses to write through one.
    pub fn wpath(&mut self, arg: &str, follow_last: bool) -> Option<PathBuf> {
        match self.sh.grants.check_write(&self.sh.cwd, arg, follow_last) {
            Ok(r) => Some(r),
            Err(w) => {
                self.deny_msg(arg, w);
                None
            }
        }
    }

    pub fn stdin_all(&mut self) -> Vec<u8> {
        let mut v = Vec::new();
        let _ = self.stdin.read_to_end(&mut v);
        v
    }

    /// Reads a whole input operand (`-` is stdin). Prints the error and
    /// returns None on failure.
    pub fn read_input(&mut self, arg: &str) -> Option<Vec<u8>> {
        if arg == "-" {
            return Some(self.stdin_all());
        }
        let p = self.rpath(arg)?;
        match std::fs::read(&p) {
            Ok(v) => Some(v),
            Err(e) => {
                self.io_msg(arg, &e);
                None
            }
        }
    }

    pub fn put(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }

    pub fn puts(&mut self, s: &str) {
        self.put(s.as_bytes());
    }
}

/// The class of a plain `msg` line. Usage errors share a few stable openings
/// (GNU wording); everything else is [`ErrorClass::Failed`] unless the caller
/// names a class with [`Ctx::fail`].
pub fn classify(text: &str) -> ErrorClass {
    const USAGE: &[&str] = &[
        "unknown flag ",
        "missing ",
        "extra operand",
        "option ",
        "too many arguments",
        "invalid number of ",
        "only one type of list",
        "you must specify a list",
        "an input delimiter may be specified only",
        "no script specified",
        "no input files",
    ];
    const PARSE: &[&str] = &[
        ": invalid JSON: ",
        ": invalid YAML: ",
        ": invalid TOML: ",
        ": not UTF-8 text",
    ];
    if USAGE.iter().any(|p| text.starts_with(p)) || text.contains("; usage: ") {
        ErrorClass::Usage
    } else if text.starts_with("VALUE '") || PARSE.iter().any(|p| text.contains(p)) {
        ErrorClass::Parse
    } else if text.starts_with("invalid regex ") {
        ErrorClass::Regex
    } else {
        ErrorClass::Failed
    }
}

pub struct Parsed {
    pub flags: Vec<(char, Option<String>)>,
    pub operands: Vec<String>,
}

impl Parsed {
    pub fn has(&self, c: char) -> bool {
        self.flags.iter().any(|(f, _)| *f == c)
    }
    pub fn val(&self, c: char) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(f, _)| *f == c)
            .and_then(|(_, v)| v.as_deref())
    }
    pub fn vals(&self, c: char) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(f, _)| *f == c)
            .filter_map(|(_, v)| v.as_deref())
            .collect()
    }
}

pub struct Spec<'a> {
    /// Short flags; a trailing `:` takes an argument, a trailing `?` takes an
    /// optional attached argument.
    pub short: &'a str,
    pub long: &'a [(&'a str, char)],
    /// `-5` means `-n 5`.
    pub numeric: bool,
    /// Human-readable list of supported flags, shown on an unknown flag.
    pub supported: &'a str,
}

/// GNU-style option parsing (options may follow operands). An unknown flag
/// is an error naming the flag and the supported set; it is never ignored.
pub fn getopt(ctx: &mut Ctx, args: &[String], spec: &Spec) -> Result<Parsed, ()> {
    ctx.supported = Some(spec.supported.to_string());
    let mut flags = Vec::new();
    let mut operands = Vec::new();
    let takes = |c: char| -> Option<char> {
        let b: Vec<char> = spec.short.chars().collect();
        let i = b.iter().position(|&x| x == c && x != ':' && x != '?')?;
        Some(match b.get(i + 1) {
            Some(':') => ':',
            Some('?') => '?',
            _ => ' ',
        })
    };
    let unknown = |ctx: &mut Ctx, what: &str| {
        let n = ctx.name.clone();
        ctx.fail(
            ErrorClass::Usage,
            Some(what),
            &format!(
                "unknown flag {what}; kapsel's {n} supports: {}",
                spec.supported
            ),
        );
    };
    let mut i = 0;
    let mut only_operands = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if only_operands || a == "-" || !a.starts_with('-') {
            operands.push(a.clone());
            continue;
        }
        if a == "--" {
            only_operands = true;
            continue;
        }
        if let Some(rest) = a.strip_prefix("--") {
            let (name, val) = match rest.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (rest, None),
            };
            let Some(&(_, c)) = spec.long.iter().find(|(n, _)| *n == name) else {
                unknown(ctx, a);
                return Err(());
            };
            match takes(c) {
                Some(':') => {
                    let v = match val {
                        Some(v) => v,
                        None => {
                            if i < args.len() {
                                i += 1;
                                args[i - 1].clone()
                            } else {
                                ctx.msg(&format!("option '--{name}' requires an argument"));
                                return Err(());
                            }
                        }
                    };
                    flags.push((c, Some(v)));
                }
                _ => flags.push((c, val)),
            }
            continue;
        }
        let body: Vec<char> = a[1..].chars().collect();
        if spec.numeric && body.iter().all(|c| c.is_ascii_digit()) {
            flags.push(('n', Some(body.iter().collect())));
            continue;
        }
        let mut k = 0;
        while k < body.len() {
            let c = body[k];
            k += 1;
            match takes(c) {
                None => {
                    unknown(ctx, &format!("-{c}"));
                    return Err(());
                }
                Some(':') => {
                    let v: String = if k < body.len() {
                        let s = body[k..].iter().collect();
                        k = body.len();
                        s
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        ctx.msg(&format!("option requires an argument -- '{c}'"));
                        return Err(());
                    };
                    flags.push((c, Some(v)));
                }
                Some('?') => {
                    let v: String = body[k..].iter().collect();
                    k = body.len();
                    flags.push((c, if v.is_empty() { None } else { Some(v) }));
                }
                _ => flags.push((c, None)),
            }
        }
    }
    Ok(Parsed { flags, operands })
}

/// The exact flags of the REQ-KS-063 built-ins, as (flag, number of values).
/// kapsel-check's `xflags_for` has the same table (with which values are
/// paths); `xflags_tables_agree` keeps the two equal.
pub fn xflags_for(tool: &str) -> Option<&'static [(&'static str, usize)]> {
    Some(match tool {
        "edit" => &[
            ("-s", 2),
            ("-e", 2),
            ("-m", 0),
            ("-c", 1),
            ("--allow-none", 0),
            ("--lines", 1),
            ("--with", 1),
            ("--with-text", 1),
        ],
        "json" => &[("-c", 0), ("-r", 0), ("-cr", 0), ("-rc", 0), ("--str", 0)],
        "yaml" => &[("-j", 0), ("--str", 0)],
        "toml" => &[("--str", 0)],
        "field" => &[("-d", 1), ("-o", 1)],
        "lines" => &[],
        "base64" => &[("-d", 0), ("--decode", 0), ("-w", 1)],
        "sha256sum" => &[("-c", 0), ("--check", 0)],
        _ => return None,
    })
}

/// Flags (with their values) and operands, parsed by an exact-flag table.
pub struct XParsed {
    pub flags: Vec<(&'static str, Vec<String>)>,
    pub operands: Vec<String>,
}

impl XParsed {
    pub fn has(&self, f: &str) -> bool {
        self.flags.iter().any(|(n, _)| *n == f)
    }
    pub fn val(&self, f: &str) -> Option<&[String]> {
        self.flags
            .iter()
            .rev()
            .find(|(n, _)| *n == f)
            .map(|(_, v)| v.as_slice())
    }
}

/// True for `-1`, `-2.5`: an operand, not a flag.
pub fn is_negative_number(s: &str) -> bool {
    s.strip_prefix('-').is_some_and(|r| {
        r.chars().next().is_some_and(|c| c.is_ascii_digit())
            && r.chars().all(|c| c.is_ascii_digit() || c == '.')
    })
}

/// `-s X Y -m ...`: an exact-flag table as text, `no flags` when empty.
fn supported_text(table: &[(&str, usize)]) -> String {
    let v: Vec<String> = table
        .iter()
        .map(|(f, n)| match n {
            0 => f.to_string(),
            1 => format!("{f} X"),
            _ => format!("{f} X Y"),
        })
        .collect();
    if v.is_empty() {
        "no flags".to_string()
    } else {
        v.join(" ")
    }
}

/// Parses the arguments of a REQ-KS-063 built-in. Flags are whole words
/// (no clusters), anywhere on the line; `--` ends them; `-` and negative
/// numbers are operands. An unknown flag is an error naming the supported set.
pub fn xgetopt(ctx: &mut Ctx, args: &[String]) -> Result<XParsed, ()> {
    let table = xflags_for(&ctx.name).unwrap_or(&[]);
    ctx.supported = Some(supported_text(table));
    let mut p = XParsed {
        flags: Vec::new(),
        operands: Vec::new(),
    };
    let mut i = 0;
    let mut only_operands = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if only_operands || !a.starts_with('-') || a == "-" || is_negative_number(a) {
            p.operands.push(a.clone());
            continue;
        }
        if a == "--" {
            only_operands = true;
            continue;
        }
        let Some(&(flag, n)) = table.iter().find(|(f, _)| f == a) else {
            let name = ctx.name.clone();
            ctx.fail(
                ErrorClass::Usage,
                Some(a),
                &format!(
                    "unknown flag {a}; kapsel's {name} supports: {}",
                    supported_text(table)
                ),
            );
            return Err(());
        };
        if i + n > args.len() {
            ctx.msg(&format!(
                "option '{flag}' requires {n} argument{}",
                if n == 1 { "" } else { "s" }
            ));
            return Err(());
        }
        p.flags.push((flag, args[i..i + n].to_vec()));
        i += n;
    }
    Ok(p)
}

/// Replaces `path` with `data` atomically: a temporary file in the same
/// directory, then a rename over the original, so a reader sees the old
/// file or the new one and never a partial write. The original's permission
/// bits are kept. The caller has already passed `path` through
/// [`Ctx::wpath`] (the write grant, and no existing symlink).
pub fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let perms = std::fs::metadata(path).ok().map(|m| m.permissions());
    let mut attempt = 0;
    loop {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".{base}.kapsel-{}-{n}.tmp", std::process::id()));
        let fh = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp);
        let mut fh = match fh {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 8 => {
                attempt += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        let res = fh
            .write_all(data)
            .and_then(|()| fh.sync_all())
            .and_then(|()| match &perms {
                Some(p) => std::fs::set_permissions(&tmp, p.clone()),
                None => Ok(()),
            })
            .and_then(|()| std::fs::rename(&tmp, path));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        return res;
    }
}

impl Ctx<'_> {
    /// Resolves FILE for an in-place edit (write grant, no symlink) and reads
    /// it. Prints the reason and returns None on failure.
    pub fn edit_target(&mut self, arg: &str) -> Option<(PathBuf, Vec<u8>)> {
        let path = self.wpath(arg, true)?;
        match std::fs::read(&path) {
            Ok(d) => Some((path, d)),
            Err(e) => {
                self.io_msg(arg, &e);
                None
            }
        }
    }

    /// Writes an edited FILE back atomically. Returns the status.
    pub fn commit(&mut self, arg: &str, path: &std::path::Path, data: &[u8]) -> i32 {
        match atomic_write(path, data) {
            Ok(()) => 0,
            Err(e) => {
                self.io_msg(arg, &e);
                1
            }
        }
    }
}

/// Runs the built-in `args[0]`. Returns the status (256 + n for `exit n`).
pub fn run(sh: &mut Shell, args: &[String], stdin: Box<dyn Read>, out: Sink, err: Sink) -> i32 {
    let mut ctx = Ctx {
        name: args[0].clone(),
        sh,
        stdin,
        out,
        err,
        supported: None,
    };
    let a = &args[1..];
    match args[0].as_str() {
        ":" | "true" => 0,
        "false" => 1,
        "echo" => basic::echo(&mut ctx, a),
        "printf" => basic::printf(&mut ctx, a),
        "pwd" => basic::pwd(&mut ctx, a),
        "cd" => basic::cd(&mut ctx, a),
        "export" => basic::export(&mut ctx, a),
        "exit" => basic::exit(&mut ctx, a),
        "test" => basic::test(&mut ctx, a, false),
        "[" => basic::test(&mut ctx, a, true),
        "cat" => text::cat(&mut ctx, a),
        "wc" => text::wc(&mut ctx, a),
        "head" => text::head(&mut ctx, a),
        "tail" => text::tail(&mut ctx, a),
        "grep" => text::grep(&mut ctx, a),
        "sort" => text2::sort(&mut ctx, a),
        "uniq" => text2::uniq(&mut ctx, a),
        "tee" => text2::tee(&mut ctx, a),
        "cut" => text2::cut(&mut ctx, a),
        "tr" => text2::tr(&mut ctx, a),
        "seq" => text2::seq(&mut ctx, a),
        "sed" => sed::sed(&mut ctx, a),
        "mkdir" => files::mkdir(&mut ctx, a),
        "rm" => files::rm(&mut ctx, a),
        "cp" => files::cp(&mut ctx, a),
        "mv" => files::mv(&mut ctx, a),
        "ls" => files::ls(&mut ctx, a),
        "date" => timefmt::date(&mut ctx, a),
        "sleep" => timefmt::sleep(&mut ctx, a),
        "edit" => edit::edit(&mut ctx, a),
        "json" => jsonedit::json(&mut ctx, a),
        "yaml" => data::yaml(&mut ctx, a),
        "toml" => data::toml(&mut ctx, a),
        "field" => text3::field(&mut ctx, a),
        "lines" => text3::lines(&mut ctx, a),
        "base64" => text3::base64(&mut ctx, a),
        "sha256sum" => text3::sha256sum(&mut ctx, a),
        other => {
            let o = other.to_string();
            ctx.msg(&format!("{o}: not a kapsel built-in"));
            2
        }
    }
}

impl Ctx<'_> {
    /// `name: text` on stderr for a pattern that does not compile. `full` is
    /// the regex crate's whole error for the pattern as compiled; the help
    /// names the position it marks.
    pub fn regex_fail(&mut self, pattern: &str, text: &str, full: Option<&str>) {
        let line = format!("{}: {}", self.name, text);
        err_line(&mut self.err, &line);
        let help = match full.and_then(crate::runerr::regex_position) {
            Some(at) => format!(
                "the pattern `{pattern}` fails at {at} (counted in the pattern as kapsel's \
                 regex engine reads it); escape a literal metacharacter with a backslash"
            ),
            None => help_for(ErrorClass::Regex, &self.name, Some(pattern), None),
        };
        self.record_with_help(ErrorClass::Regex, Some(pattern), line, help);
    }
}

/// The regex crate's full error text for `pattern`, if it does not compile.
pub fn regex_error(pattern: &str) -> Option<String> {
    regex::Regex::new(pattern).err().map(|e| e.to_string())
}

// Test-only fault injection (DD-KS-020): with this set, the read-back of a
// structured edit sees an empty document, which no intended document equals.
#[cfg(test)]
thread_local! {
    pub(crate) static BREAK_READBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The text a structured edit's read-back parses: the edit's output, except
/// in test builds that break it on purpose.
#[cfg(test)]
pub(crate) fn readback_text(out: &str) -> &str {
    if BREAK_READBACK.get() { "" } else { out }
}

#[cfg(not(test))]
#[inline]
pub(crate) fn readback_text(out: &str) -> &str {
    out
}

#[cfg(test)]
mod readback_tests;
