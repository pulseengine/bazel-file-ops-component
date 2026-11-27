//! echo printf pwd cd export exit test [

use super::{Ctx, Spec, getopt};

/// Interprets backslash escapes as `echo -e` / `printf` do. Returns false if
/// `\c` stopped the output.
pub fn unescape(s: &str, out: &mut Vec<u8>, printf_style: bool) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = b[i];
        i += 1;
        match c {
            b'\\' => out.push(b'\\'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'c' => return false,
            b'e' if !printf_style => out.push(27),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'"' if printf_style => out.push(b'"'),
            b'0'..=b'7' if printf_style || c == b'0' => {
                // echo -e: \0NNN (up to 3 digits after the 0); printf: \NNN
                let mut v = 0u32;
                let mut n = 0;
                let mut j = if printf_style { i - 1 } else { i };
                while j < b.len() && n < 3 && (b'0'..=b'7').contains(&b[j]) {
                    v = v * 8 + (b[j] - b'0') as u32;
                    j += 1;
                    n += 1;
                }
                i = j;
                out.push(v as u8);
            }
            b'x' => {
                let mut v = 0u32;
                let mut n = 0;
                while i < b.len() && n < 2 && b[i].is_ascii_hexdigit() {
                    v = v * 16 + (b[i] as char).to_digit(16).unwrap();
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    out.extend_from_slice(b"\\x");
                } else {
                    out.push(v as u8);
                }
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    true
}

pub fn echo(ctx: &mut Ctx, a: &[String]) -> i32 {
    // GNU echo: leading args made only of -n -e -E letters are options;
    // anything else (including unknown dash words) is text.
    let mut nl = true;
    let mut esc = false;
    let mut i = 0;
    while i < a.len() {
        let w = &a[i];
        if w.len() > 1 && w.starts_with('-') && w[1..].chars().all(|c| matches!(c, 'n' | 'e' | 'E')) {
            for c in w[1..].chars() {
                match c {
                    'n' => nl = false,
                    'e' => esc = true,
                    _ => esc = false,
                }
            }
            i += 1;
        } else {
            break;
        }
    }
    let mut out = Vec::new();
    for (k, w) in a[i..].iter().enumerate() {
        if k > 0 {
            out.push(b' ');
        }
        if esc {
            if !unescape(w, &mut out, false) {
                nl = false;
                break;
            }
        } else {
            out.extend_from_slice(w.as_bytes());
        }
    }
    if nl {
        out.push(b'\n');
    }
    ctx.put(&out);
    0
}

// ---- printf -------------------------------------------------------------

struct Fmt {
    minus: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alt: bool,
    width: Option<usize>,
    prec: Option<usize>,
}

fn pad(s: String, f: &Fmt, numeric: bool) -> String {
    let w = f.width.unwrap_or(0);
    let n = s.chars().count();
    if n >= w {
        return s;
    }
    let fill = w - n;
    if f.minus {
        format!("{s}{}", " ".repeat(fill))
    } else if f.zero && numeric {
        let (sign, rest) = match s.chars().next() {
            Some(c @ ('-' | '+' | ' ')) => (c.to_string(), s[1..].to_string()),
            _ => (String::new(), s.clone()),
        };
        let (pre, rest) = if rest.starts_with("0x") || rest.starts_with("0X") {
            (rest[..2].to_string(), rest[2..].to_string())
        } else {
            (String::new(), rest)
        };
        format!("{sign}{pre}{}{rest}", "0".repeat(fill))
    } else {
        format!("{}{s}", " ".repeat(fill))
    }
}

fn parse_int(ctx: &mut Ctx, s: &str, status: &mut i32) -> i128 {
    let t = s.trim();
    if t.is_empty() {
        return 0;
    }
    let b = t.as_bytes();
    if (b[0] == b'\'' || b[0] == b'"') && t.chars().count() >= 2 {
        return t.chars().nth(1).unwrap() as i128;
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (radix, digits) = if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        (16, h)
    } else if body.len() > 1 && body.starts_with('0') {
        (8, &body[1..])
    } else {
        (10, body)
    };
    let end = digits.find(|c: char| !c.is_digit(radix)).unwrap_or(digits.len());
    if end == 0 {
        ctx.msg(&format!("'{s}': expected a numeric value"));
        *status = 1;
        return 0;
    }
    let v = i128::from_str_radix(&digits[..end], radix).unwrap_or(0);
    if end < digits.len() {
        ctx.msg(&format!("'{s}': value not completely converted"));
        *status = 1;
    }
    if neg { -v } else { v }
}

fn parse_float(ctx: &mut Ctx, s: &str, status: &mut i32) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    match t.parse::<f64>() {
        Ok(v) => v,
        Err(_) => {
            ctx.msg(&format!("'{s}': expected a numeric value"));
            *status = 1;
            0.0
        }
    }
}

fn fmt_exp(v: f64, prec: usize, upper: bool) -> String {
    let s = format!("{:.*e}", prec, v);
    let (m, e) = s.split_once('e').unwrap();
    let e: i32 = e.parse().unwrap();
    let r = format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    if upper { r.to_uppercase() } else { r }
}

fn fmt_g(v: f64, prec: Option<usize>, alt: bool, upper: bool) -> String {
    let p = match prec {
        Some(0) => 1,
        Some(p) => p,
        None => 6,
    };
    if v == 0.0 {
        return if alt { format!("{:.*}", p - 1, 0.0) } else { "0".into() };
    }
    let x: i32 = format!("{:.*e}", p - 1, v).split_once('e').unwrap().1.parse().unwrap();
    let mut s = if x >= -4 && (x as i64) < p as i64 {
        format!("{:.*}", (p as i32 - 1 - x).max(0) as usize, v)
    } else {
        fmt_exp(v, p - 1, false)
    };
    if !alt {
        let (mant, exp) = match s.split_once('e') {
            Some((m, e)) => (m.to_string(), format!("e{e}")),
            None => (s.clone(), String::new()),
        };
        let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.').to_string() } else { mant };
        s = format!("{mant}{exp}");
    }
    if upper { s.to_uppercase() } else { s }
}

/// One pass over the format. Returns whether any argument was consumed.
fn format_once(ctx: &mut Ctx, fmt: &str, args: &[String], pos: &mut usize, out: &mut Vec<u8>, status: &mut i32) -> Result<(bool, bool), ()> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut used = false;
    let next_arg = |pos: &mut usize, used: &mut bool| -> Option<String> {
        if *pos < args.len() {
            *pos += 1;
            *used = true;
            Some(args[*pos - 1].clone())
        } else {
            None
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            // collect a run of escape text
            let mut j = i + 1;
            if j < chars.len() {
                j += 1;
            }
            let seg: String = chars[i..j].iter().collect();
            // octal/hex escapes may be longer
            let mut k = j;
            if matches!(chars.get(i + 1), Some('0'..='7')) {
                while k < chars.len() && k < i + 4 && ('0'..='7').contains(&chars[k]) {
                    k += 1;
                }
            } else if chars.get(i + 1) == Some(&'x') {
                while k < chars.len() && k < i + 4 && chars[k].is_ascii_hexdigit() {
                    k += 1;
                }
            }
            let seg: String = if k > j { chars[i..k].iter().collect() } else { seg };
            let cont = unescape(&seg, out, true);
            i = k;
            if !cont {
                return Ok((used, true));
            }
            continue;
        }
        if c != '%' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            ctx.msg("%: invalid conversion specification");
            *status = 1;
            return Err(());
        }
        let mut f = Fmt { minus: false, plus: false, space: false, zero: false, alt: false, width: None, prec: None };
        while i < chars.len() {
            match chars[i] {
                '-' => f.minus = true,
                '+' => f.plus = true,
                ' ' => f.space = true,
                '0' => f.zero = true,
                '#' => f.alt = true,
                _ => break,
            }
            i += 1;
        }
        if chars.get(i) == Some(&'*') {
            i += 1;
            let a = next_arg(pos, &mut used).unwrap_or_default();
            let v = parse_int(ctx, &a, status);
            if v < 0 {
                f.minus = true;
            }
            f.width = Some(v.unsigned_abs() as usize);
        } else {
            let st = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i > st {
                f.width = chars[st..i].iter().collect::<String>().parse().ok();
            }
        }
        if chars.get(i) == Some(&'.') {
            i += 1;
            if chars.get(i) == Some(&'*') {
                i += 1;
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let v = parse_int(ctx, &a, status);
                f.prec = if v >= 0 { Some(v as usize) } else { None };
            } else {
                let st = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                f.prec = Some(chars[st..i].iter().collect::<String>().parse().unwrap_or(0));
            }
        }
        while i < chars.len() && matches!(chars[i], 'l' | 'h' | 'L' | 'q' | 'j' | 'z' | 't') {
            i += 1;
        }
        let Some(&conv) = chars.get(i) else {
            ctx.msg("invalid conversion specification: missing conversion character");
            *status = 1;
            return Err(());
        };
        i += 1;
        match conv {
            '%' => out.push(b'%'),
            's' => {
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let a = match f.prec {
                    Some(p) => a.chars().take(p).collect(),
                    None => a,
                };
                out.extend_from_slice(pad(a, &f, false).as_bytes());
            }
            'b' => {
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let mut tmp = Vec::new();
                let cont = unescape(&a, &mut tmp, false);
                let s = String::from_utf8_lossy(&tmp).into_owned();
                out.extend_from_slice(pad(s, &f, false).as_bytes());
                if !cont {
                    return Ok((used, true));
                }
            }
            'c' => {
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let s: String = a.chars().take(1).collect();
                out.extend_from_slice(pad(s, &f, false).as_bytes());
            }
            'd' | 'i' => {
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let v = parse_int(ctx, &a, status);
                let mut digits = v.unsigned_abs().to_string();
                if let Some(p) = f.prec {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                let sign = if v < 0 { "-" } else if f.plus { "+" } else if f.space { " " } else { "" };
                let ff = Fmt { zero: f.zero && f.prec.is_none(), ..f };
                out.extend_from_slice(pad(format!("{sign}{digits}"), &ff, true).as_bytes());
            }
            'u' | 'o' | 'x' | 'X' => {
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let v = parse_int(ctx, &a, status) as u64;
                let mut digits = match conv {
                    'u' => v.to_string(),
                    'o' => format!("{v:o}"),
                    'x' => format!("{v:x}"),
                    _ => format!("{v:X}"),
                };
                if let Some(p) = f.prec {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                if f.alt && v != 0 {
                    match conv {
                        'o' => digits.insert(0, '0'),
                        'x' => digits.insert_str(0, "0x"),
                        'X' => digits.insert_str(0, "0X"),
                        _ => {}
                    }
                }
                let ff = Fmt { zero: f.zero && f.prec.is_none(), ..f };
                out.extend_from_slice(pad(digits, &ff, true).as_bytes());
            }
            'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                let a = next_arg(pos, &mut used).unwrap_or_default();
                let v = parse_float(ctx, &a, status);
                let mut s = match conv {
                    'f' | 'F' => format!("{:.*}", f.prec.unwrap_or(6), v.abs()),
                    'e' | 'E' => fmt_exp(v.abs(), f.prec.unwrap_or(6), conv == 'E'),
                    _ => fmt_g(v.abs(), f.prec, f.alt, conv == 'G'),
                };
                let neg = v.is_sign_negative() && !(v == 0.0 && false);
                let sign = if neg { "-" } else if f.plus { "+" } else if f.space { " " } else { "" };
                s = format!("{sign}{s}");
                out.extend_from_slice(pad(s, &f, true).as_bytes());
            }
            other => {
                ctx.msg(&format!("%{other}: invalid conversion specification"));
                *status = 1;
                return Err(());
            }
        }
    }
    Ok((used, false))
}

pub fn printf(ctx: &mut Ctx, a: &[String]) -> i32 {
    let a: Vec<String> = if a.first().map(|s| s.as_str()) == Some("--") { a[1..].to_vec() } else { a.to_vec() };
    if a.is_empty() {
        ctx.msg("missing operand");
        return 2;
    }
    if a[0].starts_with('-') && a[0].len() > 1 {
        ctx.msg(&format!("unknown flag {}; kapsel's printf takes FORMAT [ARGUMENT]... (use -- before a format starting with -)", a[0]));
        return 2;
    }
    let fmt = a[0].clone();
    let args = &a[1..];
    let mut pos = 0;
    let mut status = 0;
    let mut out = Vec::new();
    loop {
        match format_once(ctx, &fmt, args, &mut pos, &mut out, &mut status) {
            Err(()) => {
                ctx.put(&out);
                return 1;
            }
            Ok((_, true)) => break,
            Ok((used, false)) => {
                if !used || pos >= args.len() {
                    break;
                }
            }
        }
    }
    ctx.put(&out);
    status
}

// ---- pwd cd export exit ----------------------------------------------------

pub fn pwd(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "LP", long: &[], numeric: false, supported: "-L -P (the working directory is always physical)" };
    let Ok(_p) = getopt(ctx, a, &spec) else { return 2 };
    let s = format!("{}\n", ctx.sh.cwd.display());
    ctx.puts(&s);
    0
}

pub fn cd(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "", long: &[], numeric: false, supported: "no flags; usage: cd [DIR | -]" };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    if p.operands.len() > 1 {
        ctx.msg("too many arguments");
        return 1;
    }
    let mut print = false;
    let target = match p.operands.first().map(|s| s.as_str()) {
        None => match ctx.sh.vars.get("HOME") {
            Some(h) if !h.is_empty() => h.clone(),
            _ => {
                ctx.msg("HOME not set");
                return 1;
            }
        },
        Some("-") => match ctx.sh.vars.get("OLDPWD") {
            Some(h) if !h.is_empty() => {
                print = true;
                h.clone()
            }
            _ => {
                ctx.msg("OLDPWD not set");
                return 1;
            }
        },
        Some(t) => t.to_string(),
    };
    let Some(r) = ctx.rpath(&target) else { return 1 };
    match std::fs::metadata(&r) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => {
            ctx.msg(&format!("{target}: Not a directory"));
            return 1;
        }
        Err(e) => {
            ctx.io_msg(&target, &e);
            return 1;
        }
    }
    let old = ctx.sh.cwd.display().to_string();
    ctx.sh.vars.insert("OLDPWD".into(), old);
    ctx.sh.vars.insert("PWD".into(), r.display().to_string());
    ctx.sh.cwd = r;
    if print {
        let s = format!("{}\n", ctx.sh.cwd.display());
        ctx.puts(&s);
    }
    0
}

fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic() || x == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

pub fn export(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "p", long: &[], numeric: false, supported: "-p; usage: export [NAME[=VALUE]]..." };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    if p.operands.is_empty() {
        let lines: Vec<String> = ctx
            .sh
            .exported
            .iter()
            .map(|n| format!("export {}=\"{}\"\n", n, ctx.sh.vars.get(n).cloned().unwrap_or_default()))
            .collect();
        for l in lines {
            ctx.puts(&l);
        }
        return 0;
    }
    let mut st = 0;
    for op in &p.operands {
        let (name, val) = match op.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (op.as_str(), None),
        };
        if !valid_name(name) {
            ctx.msg(&format!("'{op}': not a valid identifier"));
            st = 1;
            continue;
        }
        if let Some(v) = val {
            ctx.sh.vars.insert(name.to_string(), v.to_string());
        }
        ctx.sh.exported.insert(name.to_string());
    }
    st
}

pub fn exit(ctx: &mut Ctx, a: &[String]) -> i32 {
    if a.len() > 1 {
        ctx.msg("too many arguments");
        return 1;
    }
    match a.first() {
        None => 256 + ctx.sh.status,
        Some(s) => match s.trim().parse::<i64>() {
            Ok(n) => 256 + (n.rem_euclid(256)) as i32,
            Err(_) => {
                ctx.msg(&format!("{s}: numeric argument required"));
                256 + 2
            }
        },
    }
}

// ---- test ---------------------------------------------------------------

struct TestParser<'a, 'b> {
    ctx: &'a mut Ctx<'b>,
    t: Vec<String>,
    pos: usize,
}

const UNARY: &[&str] = &["-e", "-f", "-d", "-r", "-w", "-x", "-s", "-z", "-n", "-L", "-h"];
const BINARY: &[&str] = &["=", "==", "!=", "<", ">", "-eq", "-ne", "-lt", "-le", "-gt", "-ge"];

impl TestParser<'_, '_> {
    fn peek(&self, k: usize) -> Option<&str> {
        self.t.get(self.pos + k).map(|s| s.as_str())
    }
    fn or(&mut self) -> Result<bool, String> {
        let mut v = self.and()?;
        while self.peek(0) == Some("-o") {
            self.pos += 1;
            let r = self.and()?;
            v = v || r;
        }
        Ok(v)
    }
    fn and(&mut self) -> Result<bool, String> {
        let mut v = self.not()?;
        while self.peek(0) == Some("-a") {
            self.pos += 1;
            let r = self.not()?;
            v = v && r;
        }
        Ok(v)
    }
    fn not(&mut self) -> Result<bool, String> {
        if self.peek(0) == Some("!") && self.t.len() - self.pos > 1 {
            self.pos += 1;
            return Ok(!self.not()?);
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<bool, String> {
        let Some(tok) = self.peek(0).map(|s| s.to_string()) else {
            return Err("missing argument".into());
        };
        if tok == "(" && self.t.len() - self.pos > 1 {
            self.pos += 1;
            let v = self.or()?;
            if self.peek(0) != Some(")") {
                return Err("expected ')'".into());
            }
            self.pos += 1;
            return Ok(v);
        }
        let rem = self.t.len() - self.pos;
        if rem >= 3 && self.peek(1).is_some_and(|o| o.starts_with('-') || BINARY.contains(&o)) {
            let op = self.peek(1).unwrap().to_string();
            if BINARY.contains(&op.as_str()) {
                let rhs = self.t[self.pos + 2].clone();
                self.pos += 3;
                return self.binary(&tok, &op, &rhs);
            }
            if !matches!(op.as_str(), "-a" | "-o") {
                return Err(format!("unsupported operator '{op}'; supported: {}", supported_ops()));
            }
        }
        if rem >= 2 && tok.starts_with('-') && tok.len() == 2 && !matches!(tok.as_str(), "-a" | "-o") {
            if UNARY.contains(&tok.as_str()) {
                let arg = self.t[self.pos + 1].clone();
                self.pos += 2;
                return Ok(self.unary(&tok, &arg));
            }
            return Err(format!("unsupported operator '{tok}'; supported: {}", supported_ops()));
        }
        self.pos += 1;
        Ok(!tok.is_empty())
    }
    fn unary(&mut self, op: &str, arg: &str) -> bool {
        match op {
            "-z" => return arg.is_empty(),
            "-n" => return !arg.is_empty(),
            _ => {}
        }
        let Some(p) = self.ctx.rpath(arg) else { return false };
        let md = if op == "-L" || op == "-h" { std::fs::symlink_metadata(&p) } else { std::fs::metadata(&p) };
        let Ok(md) = md else { return false };
        use std::os::unix::fs::PermissionsExt;
        let mode = md.permissions().mode();
        match op {
            "-e" => true,
            "-f" => md.is_file(),
            "-d" => md.is_dir(),
            "-s" => md.len() > 0,
            "-L" | "-h" => md.file_type().is_symlink(),
            "-r" => mode & 0o444 != 0,
            "-w" => mode & 0o222 != 0,
            "-x" => mode & 0o111 != 0,
            _ => false,
        }
    }
    fn binary(&mut self, l: &str, op: &str, r: &str) -> Result<bool, String> {
        match op {
            "=" | "==" => Ok(l == r),
            "!=" => Ok(l != r),
            "<" => Ok(l < r),
            ">" => Ok(l > r),
            _ => {
                let a: i64 = l.trim().parse().map_err(|_| format!("{l}: integer expression expected"))?;
                let b: i64 = r.trim().parse().map_err(|_| format!("{r}: integer expression expected"))?;
                Ok(match op {
                    "-eq" => a == b,
                    "-ne" => a != b,
                    "-lt" => a < b,
                    "-le" => a <= b,
                    "-gt" => a > b,
                    _ => a >= b,
                })
            }
        }
    }
}

fn supported_ops() -> String {
    format!("{} {} ! -a -o ( )", UNARY.join(" "), BINARY.join(" "))
}

pub fn test(ctx: &mut Ctx, a: &[String], bracket: bool) -> i32 {
    let mut t = a.to_vec();
    if bracket {
        if t.last().map(|s| s.as_str()) != Some("]") {
            ctx.msg("missing ']'");
            return 2;
        }
        t.pop();
    }
    if t.is_empty() {
        return 1;
    }
    let mut p = TestParser { ctx, t, pos: 0 };
    let r = p.or();
    let done = p.pos >= p.t.len();
    let leftover = p.t.get(p.pos).cloned();
    match r {
        Ok(v) if done => {
            if v { 0 } else { 1 }
        }
        Ok(_) => {
            ctx.msg(&format!("{}: unexpected argument", leftover.unwrap_or_default()));
            2
        }
        Err(e) => {
            ctx.msg(&e);
            2
        }
    }
}
