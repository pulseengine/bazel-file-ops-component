//! sort uniq tee cut tr seq

use std::cmp::Ordering;
use std::io::Write;

use super::text::lines;
use super::{Ctx, Spec, getopt};

// ---- sort ---------------------------------------------------------------

#[derive(Clone, Default, Debug)]
struct Key {
    sf: usize,
    sc: usize,
    ef: Option<usize>,
    ec: usize,
    numeric: bool,
    reverse: bool,
    blank: bool,
    has_opts: bool,
}

fn parse_key(s: &str) -> Result<Key, String> {
    let (a, b) = match s.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let mut k = Key::default();
    let parse_pos = |t: &str, k: &mut Key, start: bool| -> Result<(usize, usize), String> {
        let digits_end = t
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(t.len());
        let (pos, opts) = t.split_at(digits_end);
        let (f, c) = match pos.split_once('.') {
            Some((f, c)) => (f, c),
            None => (pos, ""),
        };
        let f: usize = f.parse().map_err(|_| {
            format!("invalid number at field start: invalid count at start of '{s}'")
        })?;
        let c: usize = if c.is_empty() {
            0
        } else {
            c.parse()
                .map_err(|_| format!("invalid character position in '{s}'"))?
        };
        for o in opts.chars() {
            match o {
                'n' => k.numeric = true,
                'r' => k.reverse = true,
                'b' => k.blank = true,
                _ => {
                    return Err(format!(
                        "unsupported key option '{o}' in '{s}'; supported: n r b"
                    ));
                }
            }
            k.has_opts = true;
        }
        if f == 0 && start {
            return Err(format!(
                "field number is zero: invalid field specification '{s}'"
            ));
        }
        Ok((f, c))
    };
    let (sf, sc) = parse_pos(a, &mut k, true)?;
    k.sf = sf;
    k.sc = sc;
    if let Some(b) = b {
        let (ef, ec) = parse_pos(b, &mut k, false)?;
        k.ef = Some(ef);
        k.ec = ec;
    }
    Ok(k)
}

fn field_ranges(line: &[u8], sep: Option<u8>) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    match sep {
        Some(s) => {
            let mut st = 0;
            for (i, &b) in line.iter().enumerate() {
                if b == s {
                    v.push((st, i));
                    st = i + 1;
                }
            }
            v.push((st, line.len()));
        }
        None => {
            let mut pos = 0;
            let blank = |b: u8| b == b' ' || b == b'\t';
            while pos < line.len() {
                let st = pos;
                while pos < line.len() && blank(line[pos]) {
                    pos += 1;
                }
                while pos < line.len() && !blank(line[pos]) {
                    pos += 1;
                }
                v.push((st, pos));
            }
        }
    }
    v
}

fn key_bytes<'a>(line: &'a [u8], k: &Key, sep: Option<u8>) -> &'a [u8] {
    let r = field_ranges(line, sep);
    let mut start = match r.get(k.sf - 1) {
        Some(x) => x.0,
        None => return &line[line.len()..],
    };
    if k.blank {
        while start < line.len() && (line[start] == b' ' || line[start] == b'\t') {
            start += 1;
        }
    }
    if k.sc > 0 {
        start = (start + k.sc - 1).min(line.len());
    }
    let end = match k.ef {
        None => line.len(),
        Some(ef) => match r.get(ef - 1) {
            Some(x) => {
                if k.ec == 0 {
                    x.1
                } else {
                    (x.0 + k.ec).min(x.1)
                }
            }
            None => line.len(),
        },
    };
    if start >= end {
        &line[start.min(line.len())..start.min(line.len())]
    } else {
        &line[start..end]
    }
}

fn num_value(b: &[u8]) -> f64 {
    let s = String::from_utf8_lossy(b);
    let t = s.trim_start();
    let mut end = 0;
    let cs: Vec<char> = t.chars().collect();
    let mut i = 0;
    if i < cs.len() && cs[i] == '-' {
        i += 1;
    }
    let ds = i;
    while i < cs.len() && cs[i].is_ascii_digit() {
        i += 1;
    }
    if i < cs.len() && cs[i] == '.' {
        i += 1;
        while i < cs.len() && cs[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i > ds {
        end = i;
    }
    let num: String = cs[..end].iter().collect();
    num.parse().unwrap_or(0.0)
}

pub fn sort(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "urnsbk:t:o:",
        long: &[
            ("unique", 'u'),
            ("reverse", 'r'),
            ("numeric-sort", 'n'),
            ("stable", 's'),
            ("ignore-leading-blanks", 'b'),
            ("key", 'k'),
            ("field-separator", 't'),
            ("output", 'o'),
        ],
        numeric: false,
        supported: "-u -r -n -s -b -k KEYDEF -t SEP -o FILE",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let sep = match p.val('t') {
        None => None,
        Some(t) if t.len() == 1 => Some(t.as_bytes()[0]),
        Some(t) => {
            ctx.msg(&format!("multi-character tab '{t}'"));
            return 2;
        }
    };
    let mut keys = Vec::new();
    for k in p.vals('k') {
        match parse_key(k) {
            Ok(k) => keys.push(k),
            Err(e) => {
                ctx.msg(&e);
                return 2;
            }
        }
    }
    let (gu, gr, gn, gs, gb) = (p.has('u'), p.has('r'), p.has('n'), p.has('s'), p.has('b'));
    if keys.is_empty() {
        keys.push(Key {
            sf: 1,
            numeric: gn,
            reverse: gr,
            blank: gb,
            has_opts: true,
            ..Default::default()
        });
    } else {
        for k in keys.iter_mut() {
            if !k.has_opts {
                k.numeric = gn;
                k.reverse = gr;
                k.blank = gb;
            }
        }
    }
    let whole_line_default = p.vals('k').is_empty();
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    let mut data = Vec::new();
    let mut status = 0;
    for f in &files {
        match ctx.read_input(f) {
            Some(d) => {
                data.extend_from_slice(&d);
                if !d.is_empty() && !d.ends_with(b"\n") {
                    data.push(b'\n');
                }
            }
            None => status = 2,
        }
    }
    if status != 0 {
        return 2;
    }
    let mut ls: Vec<&[u8]> = lines(&data);
    let cmp_keys = |x: &[u8], y: &[u8]| -> Ordering {
        for k in &keys {
            let (kx, ky) = if whole_line_default && !k.blank {
                (x, y)
            } else {
                (key_bytes(x, k, sep), key_bytes(y, k, sep))
            };
            let mut o = if k.numeric {
                num_value(kx)
                    .partial_cmp(&num_value(ky))
                    .unwrap_or(Ordering::Equal)
            } else {
                kx.cmp(ky)
            };
            if k.reverse {
                o = o.reverse();
            }
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    };
    ls.sort_by(|x, y| {
        let o = cmp_keys(x, y);
        if o != Ordering::Equal || gu || gs {
            return o;
        }
        let o = x.cmp(y);
        if gr { o.reverse() } else { o }
    });
    let mut out = Vec::new();
    let mut prev: Option<&[u8]> = None;
    for l in &ls {
        if gu
            && let Some(pv) = prev
            && cmp_keys(pv, l) == Ordering::Equal
        {
            continue;
        }
        out.extend_from_slice(l);
        out.push(b'\n');
        prev = Some(l);
    }
    if let Some(o) = p.val('o') {
        let Some(path) = ctx.wpath(o, true) else {
            return 2;
        };
        if let Err(e) = std::fs::write(&path, &out) {
            ctx.io_msg(o, &e);
            return 2;
        }
    } else {
        ctx.put(&out);
    }
    0
}

// ---- uniq -----------------------------------------------------------------

pub fn uniq(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "cdui",
        long: &[
            ("count", 'c'),
            ("repeated", 'd'),
            ("unique", 'u'),
            ("ignore-case", 'i'),
        ],
        numeric: false,
        supported: "-c -d -u -i",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    if p.operands.len() > 1 {
        let extra = p.operands[1].clone();
        ctx.msg(&format!(
            "extra operand '{extra}' (an output file is not supported; use a redirect)"
        ));
        return 2;
    }
    let f = p.operands.first().cloned().unwrap_or_else(|| "-".into());
    let Some(data) = ctx.read_input(&f) else {
        return 1;
    };
    let (c, d, u, i) = (p.has('c'), p.has('d'), p.has('u'), p.has('i'));
    let key = |l: &[u8]| -> Vec<u8> {
        if i {
            l.to_ascii_lowercase()
        } else {
            l.to_vec()
        }
    };
    let ls = lines(&data);
    let mut groups: Vec<(&[u8], usize)> = Vec::new();
    for l in ls {
        match groups.last_mut() {
            Some((g, n)) if key(g) == key(l) => *n += 1,
            _ => groups.push((l, 1)),
        }
    }
    let mut out = Vec::new();
    for (l, n) in groups {
        if d && n < 2 {
            continue;
        }
        if u && n > 1 {
            continue;
        }
        if c {
            out.extend_from_slice(format!("{n:>7} ").as_bytes());
        }
        out.extend_from_slice(l);
        out.push(b'\n');
    }
    ctx.put(&out);
    0
}

// ---- tee ------------------------------------------------------------------

pub fn tee(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "a",
        long: &[("append", 'a')],
        numeric: false,
        supported: "-a",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let data = ctx.stdin_all();
    let mut status = 0;
    for f in &p.operands {
        if f == "-" {
            continue;
        }
        let Some(path) = ctx.wpath(f, true) else {
            status = 1;
            continue;
        };
        let mut oo = std::fs::OpenOptions::new();
        oo.create(true).write(true);
        if p.has('a') {
            oo.append(true);
        } else {
            oo.truncate(true);
        }
        match oo.open(&path).and_then(|mut fh| fh.write_all(&data)) {
            Ok(()) => {}
            Err(e) => {
                ctx.io_msg(f, &e);
                status = 1;
            }
        }
    }
    ctx.put(&data);
    status
}

// ---- cut ------------------------------------------------------------------

fn parse_list(s: &str) -> Result<Vec<(usize, usize)>, String> {
    let mut v = Vec::new();
    for part in s.split(',') {
        let bad = || format!("invalid byte, character or field list '{s}'");
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a = if a.is_empty() {
                    1
                } else {
                    a.parse::<usize>().map_err(|_| bad())?
                };
                let b = if b.is_empty() {
                    usize::MAX
                } else {
                    b.parse::<usize>().map_err(|_| bad())?
                };
                (a, b)
            }
            None => {
                let n = part.parse::<usize>().map_err(|_| bad())?;
                (n, n)
            }
        };
        if a == 0 {
            return Err("fields and positions are numbered from 1".into());
        }
        if a > b {
            return Err("invalid decreasing range".into());
        }
        v.push((a, b));
    }
    Ok(v)
}

pub fn cut(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "d:f:c:b:s",
        long: &[
            ("delimiter", 'd'),
            ("fields", 'f'),
            ("characters", 'c'),
            ("bytes", 'b'),
            ("only-delimited", 's'),
        ],
        numeric: false,
        supported: "-d DELIM -f LIST -c LIST -b LIST -s",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let modes = ['f', 'c', 'b'].iter().filter(|m| p.has(**m)).count();
    if modes == 0 {
        ctx.msg("you must specify a list of bytes, characters, or fields");
        return 2;
    }
    if modes > 1 {
        ctx.msg("only one type of list may be specified");
        return 2;
    }
    let mode = ['f', 'c', 'b'].into_iter().find(|m| p.has(*m)).unwrap();
    let list = match parse_list(p.val(mode).unwrap()) {
        Ok(l) => l,
        Err(e) => {
            ctx.msg(&e);
            return 2;
        }
    };
    let delim = match p.val('d') {
        None => b'\t',
        Some(d) if d.len() == 1 => d.as_bytes()[0],
        Some(_) => {
            ctx.msg("the delimiter must be a single character");
            return 2;
        }
    };
    if p.has('d') && mode != 'f' {
        ctx.msg("an input delimiter may be specified only when operating on fields");
        return 2;
    }
    let sel = |i: usize| list.iter().any(|&(a, b)| a <= i && i <= b);
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    let mut status = 0;
    for f in &files {
        let Some(data) = ctx.read_input(f) else {
            status = 1;
            continue;
        };
        let mut out = Vec::new();
        for l in lines(&data) {
            if mode == 'f' {
                if !l.contains(&delim) {
                    if !p.has('s') {
                        out.extend_from_slice(l);
                        out.push(b'\n');
                    }
                    continue;
                }
                let picked: Vec<&[u8]> = l
                    .split(|&b| b == delim)
                    .enumerate()
                    .filter(|(i, _)| sel(i + 1))
                    .map(|(_, x)| x)
                    .collect();
                out.extend_from_slice(&picked.join(&delim));
                out.push(b'\n');
            } else if mode == 'b' {
                for (i, &b) in l.iter().enumerate() {
                    if sel(i + 1) {
                        out.push(b);
                    }
                }
                out.push(b'\n');
            } else if let Ok(s) = std::str::from_utf8(l) {
                for (i, ch) in s.chars().enumerate() {
                    if sel(i + 1) {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                }
                out.push(b'\n');
            } else {
                for (i, &b) in l.iter().enumerate() {
                    if sel(i + 1) {
                        out.push(b);
                    }
                }
                out.push(b'\n');
            }
        }
        ctx.put(&out);
    }
    status
}

// ---- tr -------------------------------------------------------------------

fn expand_set(s: &str) -> Result<Vec<u8>, String> {
    let b = s.as_bytes();
    // first pass: tokens (byte, is_literal_dash_ok)
    let mut toks: Vec<(u8, bool)> = Vec::new(); // (byte, escaped)
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        if b[i] == b'['
            && b.get(i + 1) == Some(&b':')
            && let Some(end) = s[i + 2..].find(":]")
        {
            let name = &s[i + 2..i + 2 + end];
            let class: Vec<u8> = (0u8..=255)
                .filter(|&c| c < 128 && super::super::shell::class_match(name, c as char))
                .collect();
            if class.is_empty() {
                return Err(format!("invalid character class '{name}'"));
            }
            toks.push((0, false));
            // flush pending as ranges later: use marker by expanding directly
            toks.pop();
            flush(&mut toks, &mut out);
            out.extend(class);
            i += end + 4;
            continue;
        }
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            let c = b[i];
            i += 1;
            let v = match c {
                b'n' => b'\n',
                b't' => b'\t',
                b'r' => b'\r',
                b'a' => 7,
                b'b' => 8,
                b'f' => 12,
                b'v' => 11,
                b'\\' => b'\\',
                b'0'..=b'7' => {
                    let mut v = (c - b'0') as u32;
                    let mut n = 1;
                    while n < 3 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                        v = v * 8 + (b[i] - b'0') as u32;
                        i += 1;
                        n += 1;
                    }
                    v as u8
                }
                other => other,
            };
            toks.push((v, true));
            continue;
        }
        toks.push((b[i], false));
        i += 1;
    }
    flush(&mut toks, &mut out);
    Ok(out)
}

fn flush(toks: &mut Vec<(u8, bool)>, out: &mut Vec<u8>) {
    let mut i = 0;
    while i < toks.len() {
        if i + 2 < toks.len() && toks[i + 1] == (b'-', false) {
            let (a, b) = (toks[i].0, toks[i + 2].0);
            if a <= b {
                out.extend(a..=b);
            }
            i += 3;
        } else {
            out.push(toks[i].0);
            i += 1;
        }
    }
    toks.clear();
}

pub fn tr(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "dscC",
        long: &[
            ("delete", 'd'),
            ("squeeze-repeats", 's'),
            ("complement", 'c'),
        ],
        numeric: false,
        supported: "-d -s -c (sets: ranges a-z, escapes \\n \\t \\NNN, classes [:alpha:] ...)",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 2;
    };
    let (del, sq, comp) = (p.has('d'), p.has('s'), p.has('c') || p.has('C'));
    let ops = &p.operands;
    if ops.is_empty() {
        ctx.msg("missing operand");
        return 2;
    }
    let need2 = !del && !sq || del && sq;
    if need2 && ops.len() < 2 {
        let first = ops[0].clone();
        ctx.msg(&format!("missing operand after '{first}'"));
        return 2;
    }
    if ops.len() > 2 || (del && !sq && ops.len() > 1) {
        let extra = ops[if del && !sq { 1 } else { 2 }].clone();
        ctx.msg(&format!("extra operand '{extra}'"));
        return 2;
    }
    let mut s1 = match expand_set(&ops[0]) {
        Ok(s) => s,
        Err(e) => {
            ctx.msg(&e);
            return 2;
        }
    };
    if comp {
        let inset: Vec<bool> = (0..=255u8).map(|c| s1.contains(&c)).collect();
        s1 = (0..=255u8).filter(|&c| !inset[c as usize]).collect();
    }
    let s2 = match ops.get(1) {
        Some(o) => match expand_set(o) {
            Ok(s) => s,
            Err(e) => {
                ctx.msg(&e);
                return 2;
            }
        },
        None => Vec::new(),
    };
    let data = ctx.stdin_all();
    let mut map: [Option<u8>; 256] = [None; 256];
    let mut delete = [false; 256];
    let mut squeeze = [false; 256];
    if del {
        for &c in &s1 {
            delete[c as usize] = true;
        }
        if sq {
            for &c in &s2 {
                squeeze[c as usize] = true;
            }
        }
    } else if ops.len() == 2 {
        if s2.is_empty() {
            ctx.msg("when not truncating set1, string2 must be non-empty");
            return 2;
        }
        for (i, &c) in s1.iter().enumerate() {
            let t = s2[i.min(s2.len() - 1)];
            if map[c as usize].is_none() {
                map[c as usize] = Some(t);
            }
        }
        if sq {
            for &c in &s2 {
                squeeze[c as usize] = true;
            }
        }
    } else if sq {
        for &c in &s1 {
            squeeze[c as usize] = true;
        }
    }
    let mut out = Vec::with_capacity(data.len());
    let mut last: Option<u8> = None;
    for &b in &data {
        if delete[b as usize] {
            continue;
        }
        let t = map[b as usize].unwrap_or(b);
        if squeeze[t as usize] && last == Some(t) {
            continue;
        }
        out.push(t);
        last = Some(t);
    }
    ctx.put(&out);
    0
}

// ---- seq ------------------------------------------------------------------

fn decimals(s: &str) -> usize {
    match s.split_once('.') {
        Some((_, d)) => d.trim_end_matches(|c: char| !c.is_ascii_digit()).len(),
        None => 0,
    }
}

pub fn seq(ctx: &mut Ctx, a: &[String]) -> i32 {
    let mut sep = "\n".to_string();
    let mut equal = false;
    let mut nums: Vec<String> = Vec::new();
    let mut i = 0;
    while i < a.len() {
        let x = &a[i];
        i += 1;
        let numlike = x.len() > 1
            && x.starts_with('-')
            && x[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.');
        if x == "-s" || x == "--separator" {
            if i >= a.len() {
                ctx.msg("option requires an argument -- 's'");
                return 2;
            }
            sep = a[i].clone();
            i += 1;
        } else if let Some(s) = x.strip_prefix("-s").filter(|_| !numlike) {
            sep = s.to_string();
        } else if x == "-w" || x == "--equal-width" {
            equal = true;
        } else if x == "--" {
            nums.extend(a[i..].iter().cloned());
            break;
        } else if x.starts_with('-') && x.len() > 1 && !numlike {
            ctx.msg(&format!("unknown flag {x}; kapsel's seq supports: -s SEP -w, and LAST | FIRST LAST | FIRST INCREMENT LAST"));
            return 2;
        } else {
            nums.push(x.clone());
        }
    }
    if nums.is_empty() {
        ctx.msg("missing operand");
        return 2;
    }
    if nums.len() > 3 {
        let extra = nums[3].clone();
        ctx.msg(&format!("extra operand '{extra}'"));
        return 2;
    }
    let mut vals = Vec::new();
    for n in &nums {
        match n.trim().parse::<f64>() {
            Ok(v) if v.is_finite() => vals.push(v),
            _ => {
                ctx.msg(&format!("invalid floating point argument: '{n}'"));
                return 2;
            }
        }
    }
    let (first, inc, last) = match vals.len() {
        1 => (1.0, 1.0, vals[0]),
        2 => (vals[0], 1.0, vals[1]),
        _ => (vals[0], vals[1], vals[2]),
    };
    if inc == 0.0 {
        ctx.msg(&format!("invalid Zero increment value: '{}'", nums[1]));
        return 2;
    }
    let prec = match nums.len() {
        1 => decimals(&nums[0]),
        2 => decimals(&nums[0]),
        _ => decimals(&nums[0]).max(decimals(&nums[1])),
    };
    let mut items = Vec::new();
    let mut k = 0u64;
    loop {
        let v = first + (k as f64) * inc;
        if (inc > 0.0 && v > last + 1e-9) || (inc < 0.0 && v < last - 1e-9) {
            break;
        }
        items.push(format!("{v:.prec$}"));
        k += 1;
        if k > 100_000_000 {
            break;
        }
    }
    if equal {
        let w = items
            .iter()
            .map(|s| s.trim_start_matches('-').len())
            .max()
            .unwrap_or(0);
        for s in items.iter_mut() {
            let neg = s.starts_with('-');
            let body = s.trim_start_matches('-').to_string();
            *s = format!("{}{:0>w$}", if neg { "-" } else { "" }, body);
        }
    }
    if !items.is_empty() {
        let mut o = items.join(&sep);
        o.push('\n');
        ctx.puts(&o);
    }
    0
}
