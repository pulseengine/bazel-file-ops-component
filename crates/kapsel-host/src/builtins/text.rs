//! cat wc head tail grep

use std::path::Path;

use super::regex_conv;
use super::{Ctx, Spec, getopt};

pub fn lines(data: &[u8]) -> Vec<&[u8]> {
    if data.is_empty() {
        return Vec::new();
    }
    let body = data.strip_suffix(b"\n").unwrap_or(data);
    body.split(|&b| b == b'\n').collect()
}

pub fn cat(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "nbsE",
        long: &[
            ("number", 'n'),
            ("number-nonblank", 'b'),
            ("squeeze-blank", 's'),
            ("show-ends", 'E'),
        ],
        numeric: false,
        supported: "-n -b -s -E",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let (n, b, s, e) = (p.has('n'), p.has('b'), p.has('s'), p.has('E'));
    let plain = !(n || b || s || e);
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    let mut status = 0;
    let mut num = 0u64;
    let mut prev_blank = false;
    for f in &files {
        let Some(data) = ctx.read_input(f) else {
            status = 1;
            continue;
        };
        if plain {
            ctx.put(&data);
            continue;
        }
        let ends_nl = data.ends_with(b"\n");
        let ls = lines(&data);
        let count = ls.len();
        for (i, l) in ls.iter().enumerate() {
            let blank = l.is_empty();
            if s && blank && prev_blank {
                continue;
            }
            prev_blank = blank;
            let mut o = Vec::new();
            if b {
                if !blank {
                    num += 1;
                    o.extend_from_slice(format!("{num:6}\t").as_bytes());
                }
            } else if n {
                num += 1;
                o.extend_from_slice(format!("{num:6}\t").as_bytes());
            }
            o.extend_from_slice(l);
            if e {
                o.push(b'$');
            }
            if i + 1 < count || ends_nl {
                o.push(b'\n');
            }
            ctx.put(&o);
        }
    }
    status
}

pub fn wc(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "lwcm",
        long: &[
            ("lines", 'l'),
            ("words", 'w'),
            ("bytes", 'c'),
            ("chars", 'm'),
        ],
        numeric: false,
        supported: "-l -w -c -m",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let any = p.has('l') || p.has('w') || p.has('c') || p.has('m');
    let (l, w, c, m) = if any {
        (p.has('l'), p.has('w'), p.has('c'), p.has('m'))
    } else {
        (true, true, true, false)
    };
    let ncols = [l, w, c, m].iter().filter(|x| **x).count();
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    let mut rows: Vec<(Vec<u64>, Option<String>)> = Vec::new();
    let mut status = 0;
    let mut total_bytes = 0u64;
    let mut stdin_used = false;
    for f in &files {
        let Some(data) = ctx.read_input(f) else {
            status = 1;
            continue;
        };
        if f == "-" {
            stdin_used = true;
        }
        total_bytes += data.len() as u64;
        let mut cols = Vec::new();
        if l {
            cols.push(data.iter().filter(|&&b| b == b'\n').count() as u64);
        }
        if w {
            let mut n = 0u64;
            let mut inw = false;
            for &b in &data {
                let sp = matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
                if !sp && !inw {
                    n += 1;
                }
                inw = !sp;
            }
            cols.push(n);
        }
        if m {
            cols.push(String::from_utf8_lossy(&data).chars().count() as u64);
        }
        if c {
            cols.push(data.len() as u64);
        }
        let name = if p.operands.is_empty() {
            None
        } else {
            Some(f.clone())
        };
        rows.push((cols, name));
    }
    let width = if ncols == 1 && files.len() <= 1 {
        1
    } else if stdin_used {
        7
    } else {
        total_bytes.to_string().len().max(1)
    };
    let mut total = vec![0u64; ncols];
    let emit = |ctx: &mut Ctx, cols: &[u64], name: Option<&str>| {
        let mut s = cols
            .iter()
            .map(|v| format!("{v:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(n) = name {
            s.push(' ');
            s.push_str(n);
        }
        s.push('\n');
        ctx.puts(&s);
    };
    for (cols, name) in &rows {
        for (i, v) in cols.iter().enumerate() {
            total[i] += v;
        }
        emit(ctx, cols, name.as_deref());
    }
    if files.len() > 1 {
        emit(ctx, &total, Some("total"));
    }
    status
}

/// Parses a count like `5` or `+5` (tail). Returns (n, plus).
fn count_arg(
    ctx: &mut Ctx,
    v: &str,
    what: &str,
    allow_plus: bool,
    allow_minus: bool,
) -> Option<(i64, bool, bool)> {
    let (rest, plus, minus) = if let Some(r) = v.strip_prefix('+') {
        (r, true, false)
    } else if let Some(r) = v.strip_prefix('-') {
        (r, false, true)
    } else {
        (v, false, false)
    };
    let mut digits = rest.to_string();
    let mut mult = 1i64;
    if let Some(last) = digits.chars().last() {
        let m = match last {
            'k' | 'K' => Some(1024),
            'M' => Some(1024 * 1024),
            'b' => Some(512),
            _ => None,
        };
        if let Some(m) = m {
            mult = m;
            digits.pop();
        }
    }
    match digits.parse::<i64>() {
        Ok(n) if !(plus && !allow_plus) && !(minus && !allow_minus) => {
            Some((n * mult, plus, minus))
        }
        _ => {
            ctx.msg(&format!("invalid number of {what}: '{v}'"));
            None
        }
    }
}

pub fn head(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "n:c:qv",
        long: &[
            ("lines", 'n'),
            ("bytes", 'c'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("verbose", 'v'),
        ],
        numeric: true,
        supported: "-n N, -N, -c N, -q, -v",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let bytes_mode = p.val('c').is_some()
        && p.flags.iter().rposition(|(f, _)| *f == 'c')
            > p.flags.iter().rposition(|(f, _)| *f == 'n');
    let (n, minus) = if bytes_mode {
        let Some((n, _, m)) = count_arg(ctx, p.val('c').unwrap(), "bytes", false, true) else {
            return 1;
        };
        (n, m)
    } else {
        match p.val('n') {
            Some(v) => {
                let Some((n, _, m)) = count_arg(ctx, v, "lines", false, true) else {
                    return 1;
                };
                (n, m)
            }
            None => (10, false),
        }
    };
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    let header = (files.len() > 1 && !p.has('q')) || p.has('v');
    let mut status = 0;
    for (idx, f) in files.iter().enumerate() {
        let Some(data) = ctx.read_input(f) else {
            status = 1;
            continue;
        };
        if header {
            let name = if f == "-" { "standard input" } else { f };
            let h = format!("{}==> {} <==\n", if idx > 0 { "\n" } else { "" }, name);
            ctx.puts(&h);
        }
        if bytes_mode {
            let end = if minus {
                data.len().saturating_sub(n as usize)
            } else {
                (n as usize).min(data.len())
            };
            ctx.put(&data[..end]);
        } else {
            let mut ends = Vec::new();
            for (i, &b) in data.iter().enumerate() {
                if b == b'\n' {
                    ends.push(i + 1);
                }
            }
            let total_lines = ends.len()
                + if data.last().is_some_and(|&b| b != b'\n') {
                    1
                } else {
                    0
                };
            let keep = if minus {
                total_lines.saturating_sub(n as usize)
            } else {
                (n as usize).min(total_lines)
            };
            let end = if keep == 0 {
                0
            } else if keep <= ends.len() {
                ends[keep - 1]
            } else {
                data.len()
            };
            ctx.put(&data[..end]);
        }
    }
    status
}

pub fn tail(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "n:c:qv",
        long: &[
            ("lines", 'n'),
            ("bytes", 'c'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("verbose", 'v'),
        ],
        numeric: true,
        supported: "-n N, -n +N, -N, -c N, -q, -v",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let bytes_mode = p.val('c').is_some()
        && p.flags.iter().rposition(|(f, _)| *f == 'c')
            > p.flags.iter().rposition(|(f, _)| *f == 'n');
    let (n, from_start) = if bytes_mode {
        let Some((n, plus, _)) = count_arg(ctx, p.val('c').unwrap(), "bytes", true, false) else {
            return 1;
        };
        (n, plus)
    } else {
        match p.val('n') {
            Some(v) => {
                // GNU accepts -n -5 as -n 5
                let v2 = v.strip_prefix('-').unwrap_or(v);
                let Some((n, plus, _)) = count_arg(ctx, v2, "lines", true, false) else {
                    return 1;
                };
                (n, plus)
            }
            None => (10, false),
        }
    };
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    let header = (files.len() > 1 && !p.has('q')) || p.has('v');
    let mut status = 0;
    for (idx, f) in files.iter().enumerate() {
        let Some(data) = ctx.read_input(f) else {
            status = 1;
            continue;
        };
        if header {
            let name = if f == "-" { "standard input" } else { f };
            let h = format!("{}==> {} <==\n", if idx > 0 { "\n" } else { "" }, name);
            ctx.puts(&h);
        }
        if bytes_mode {
            let start = if from_start {
                (n as usize).saturating_sub(1).min(data.len())
            } else {
                data.len().saturating_sub(n as usize)
            };
            ctx.put(&data[start..]);
        } else {
            // start offsets of each line
            let mut starts = vec![0usize];
            for (i, &b) in data.iter().enumerate() {
                if b == b'\n' && i + 1 < data.len() {
                    starts.push(i + 1);
                }
            }
            if data.is_empty() {
                starts.clear();
            }
            let total = starts.len();
            let first = if from_start {
                (n as usize).saturating_sub(1)
            } else {
                total.saturating_sub(n as usize)
            };
            if first < total {
                ctx.put(&data[starts[first]..]);
            }
        }
    }
    status
}

// ---- grep ------------------------------------------------------------------

struct GrepOpts {
    re: regex::bytes::Regex,
    invert: bool,
    number: bool,
    only: bool,
    count: bool,
    list: bool,
    quiet: bool,
}

fn grep_data(ctx: &mut Ctx, o: &GrepOpts, name: Option<&str>, data: &[u8]) -> bool {
    let binary = data.contains(&0);
    let mut matched_lines = 0u64;
    let mut buf = Vec::new();
    let prefix = |n: usize, sep: char| -> Vec<u8> {
        let mut p = Vec::new();
        if let Some(nm) = name {
            p.extend_from_slice(nm.as_bytes());
            p.push(sep as u8);
        }
        if o.number {
            p.extend_from_slice(n.to_string().as_bytes());
            p.push(sep as u8);
        }
        p
    };
    for (i, l) in lines(data).iter().enumerate() {
        let is = o.re.is_match(l) != o.invert;
        if !is {
            continue;
        }
        matched_lines += 1;
        if o.quiet || o.list {
            break;
        }
        if o.count || binary {
            continue;
        }
        if o.only {
            if !o.invert {
                for m in o.re.find_iter(l) {
                    if m.as_bytes().is_empty() {
                        continue;
                    }
                    buf.extend_from_slice(&prefix(i + 1, ':'));
                    buf.extend_from_slice(m.as_bytes());
                    buf.push(b'\n');
                }
            }
        } else {
            buf.extend_from_slice(&prefix(i + 1, ':'));
            buf.extend_from_slice(l);
            buf.push(b'\n');
        }
    }
    if o.quiet {
        return matched_lines > 0;
    }
    if o.list {
        if matched_lines > 0 {
            let mut s = name.unwrap_or("(standard input)").as_bytes().to_vec();
            s.push(b'\n');
            ctx.put(&s);
        }
        return matched_lines > 0;
    }
    if o.count {
        let mut s = Vec::new();
        if let Some(nm) = name {
            s.extend_from_slice(nm.as_bytes());
            s.push(b':');
        }
        s.extend_from_slice(format!("{matched_lines}\n").as_bytes());
        ctx.put(&s);
    } else if binary && matched_lines > 0 {
        ctx.puts(&format!(
            "grep: {}: binary file matches\n",
            name.unwrap_or("(standard input)")
        ));
    } else {
        ctx.put(&buf);
    }
    matched_lines > 0
}

fn walk(ctx: &mut Ctx, dir: &str, follow: bool, out: &mut Vec<String>) {
    let Some(r) = ctx.rpath(dir) else { return };
    let mut names: Vec<String> = match std::fs::read_dir(&r) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(e) => {
            ctx.io_msg(dir, &e);
            return;
        }
    };
    names.sort();
    for n in names {
        let child = if dir == "." && false {
            n.clone()
        } else {
            format!("{}/{}", dir.trim_end_matches('/'), n)
        };
        let child = if dir.is_empty() { n.clone() } else { child };
        let p = r.join(&n);
        let Ok(md) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if md.file_type().is_symlink() && !follow {
            continue;
        }
        let md = if md.file_type().is_symlink() {
            std::fs::metadata(&p).ok()
        } else {
            Some(md)
        };
        match md {
            Some(m) if m.is_dir() => walk(ctx, &child, follow, out),
            Some(m) if m.is_file() => out.push(child),
            _ => {}
        }
    }
}

pub fn grep(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "EFGnivrRlcoqhHswxe:",
        long: &[
            ("extended-regexp", 'E'),
            ("fixed-strings", 'F'),
            ("basic-regexp", 'G'),
            ("line-number", 'n'),
            ("ignore-case", 'i'),
            ("invert-match", 'v'),
            ("recursive", 'r'),
            ("dereference-recursive", 'R'),
            ("files-with-matches", 'l'),
            ("count", 'c'),
            ("only-matching", 'o'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("no-filename", 'h'),
            ("with-filename", 'H'),
            ("no-messages", 's'),
            ("word-regexp", 'w'),
            ("line-regexp", 'x'),
            ("regexp", 'e'),
        ],
        numeric: false,
        supported: "-E -F -G -n -i -v -r -R -l -c -o -q -h -H -s -w -x -e PATTERN",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let mut operands = p.operands.clone();
    let mut pats: Vec<String> = p.vals('e').iter().map(|s| s.to_string()).collect();
    if pats.is_empty() {
        if operands.is_empty() {
            ctx.msg("missing PATTERN; usage: grep [FLAGS] PATTERN [FILE...]");
            return 2;
        }
        pats.push(operands.remove(0));
    }
    let fixed = p.has('F');
    let ere = p.has('E');
    let mut alts = Vec::new();
    for pat in pats.iter().flat_map(|s| s.split('\n')) {
        let t = if fixed {
            Ok(regex::escape(pat))
        } else {
            regex_conv::translate_regex(pat, ere)
        };
        match t {
            Ok(t) => alts.push(format!("(?:{t})")),
            Err(e) => {
                ctx.regex_fail(pat, &format!("invalid regex '{pat}': {e}"), None);
                return 2;
            }
        }
    }
    let mut combined = alts.join("|");
    if p.has('x') {
        combined = format!("^(?:{combined})$");
    } else if p.has('w') {
        combined = format!("\\b(?:{combined})\\b");
    }
    let re = match regex_conv::build(&combined, p.has('i')) {
        Ok(r) => r,
        Err(e) => {
            let shown = if pats.len() == 1 {
                pats[0].clone()
            } else {
                pats.join("\\n")
            };
            // The first pattern that fails on its own, with the full error
            // (it marks the position).
            let full = pats.iter().find_map(|p| {
                let t = if fixed {
                    regex::escape(p)
                } else {
                    regex_conv::translate_regex(p, ere).ok()?
                };
                super::regex_error(&t)
            });
            ctx.regex_fail(
                &shown,
                &format!("invalid regex '{shown}': {e}"),
                full.as_deref(),
            );
            return 2;
        }
    };
    let o = GrepOpts {
        re,
        invert: p.has('v'),
        number: p.has('n'),
        only: p.has('o'),
        count: p.has('c'),
        list: p.has('l'),
        quiet: p.has('q'),
    };
    let recursive = p.has('r') || p.has('R');
    let follow = p.has('R');
    let silent = p.has('s');
    let mut files: Vec<(String, bool)> = Vec::new(); // (path, is_walked)
    let mut status_err = false;
    if operands.is_empty() {
        if recursive {
            let mut v = Vec::new();
            walk(ctx, "", follow, &mut v);
            // an empty `dir` yields names without "./", as GNU does
            files.extend(v.into_iter().map(|f| (f, true)));
        } else {
            files.push(("-".into(), false));
        }
    } else {
        for f in &operands {
            if recursive && f != "-" {
                let Some(r) = ctx.rpath(f) else {
                    status_err = true;
                    continue;
                };
                if r.is_dir() {
                    let mut v = Vec::new();
                    walk(ctx, f, follow, &mut v);
                    files.extend(v.into_iter().map(|x| (x, true)));
                    continue;
                }
            }
            files.push((f.clone(), false));
        }
    }
    let show_names = if p.has('h') {
        false
    } else {
        p.has('H')
            || files.len() > 1
            || (recursive && (operands.is_empty() || files.iter().any(|f| f.1)))
    };
    let mut any = false;
    for (f, _) in &files {
        let data = if f == "-" {
            Some(ctx.stdin_all())
        } else if silent {
            let r = ctx.rpath(f);
            r.and_then(|r| std::fs::read(Path::new(&r)).ok())
        } else {
            ctx.read_input(f)
        };
        let Some(data) = data else {
            status_err = true;
            continue;
        };
        let name = if show_names {
            Some(f.as_str())
        } else if o.list {
            Some(if f == "-" {
                "(standard input)"
            } else {
                f.as_str()
            })
        } else {
            None
        };
        if grep_data(ctx, &o, name, &data) {
            any = true;
            if o.quiet {
                return 0;
            }
        }
    }
    if status_err {
        2
    } else if any {
        0
    } else {
        1
    }
}
