//! The built-ins (REQ-KS-025). Each returns 0, 1 or 2, except `exit n`, which
//! returns 256 + n. Messages follow GNU: `name: operand: reason`.

use std::io::{Read, Write};
use std::path::PathBuf;

use crate::grants;
use crate::shell::{Shell, Sink, errstr};

mod basic;
mod files;
mod regex_conv;
mod sed;
mod text;
mod text2;
mod timefmt;

pub struct Ctx<'a> {
    pub name: String,
    pub sh: &'a mut Shell,
    pub stdin: Box<dyn Read>,
    pub out: Sink,
    pub err: Sink,
}

impl Ctx<'_> {
    /// `name: text` on stderr.
    pub fn msg(&mut self, text: &str) {
        let _ = writeln!(self.err, "{}: {}", self.name, text);
    }

    pub fn io_msg(&mut self, operand: &str, e: &std::io::Error) {
        let m = format!("{}: {}", operand, errstr(e));
        self.msg(&m);
    }

    pub fn deny_msg(&mut self, operand: &str, why: grants::GrantErr) {
        let _ = writeln!(self.err, "kapsel: {operand}: {}", why.reason());
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
        ctx.msg(&format!(
            "unknown flag {what}; kapsel's {n} supports: {}",
            spec.supported
        ));
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

/// Runs the built-in `args[0]`. Returns the status (256 + n for `exit n`).
pub fn run(sh: &mut Shell, args: &[String], stdin: Box<dyn Read>, out: Sink, err: Sink) -> i32 {
    let mut ctx = Ctx {
        name: args[0].clone(),
        sh,
        stdin,
        out,
        err,
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
        other => {
            let o = other.to_string();
            ctx.msg(&format!("{o}: not a kapsel built-in"));
            2
        }
    }
}
