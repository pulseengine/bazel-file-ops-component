//! `field`, `lines`, `base64`, `sha256sum` (REQ-KS-063): what awk, python
//! and perl were measured to be used for when no structure was involved.

use regex::bytes::Regex;
use sha2::{Digest, Sha256};

use super::{Ctx, xgetopt};

/// The inputs: each FILE operand, or stdin when there is none.
fn inputs(ctx: &mut Ctx, files: &[String]) -> (Vec<(String, Vec<u8>)>, i32) {
    if files.is_empty() {
        return (vec![("-".to_string(), ctx.stdin_all())], 0);
    }
    let mut out = Vec::new();
    let mut status = 0;
    for f in files {
        match ctx.read_input(f) {
            Some(d) => out.push((f.clone(), d)),
            None => status = 1,
        }
    }
    (out, status)
}

// ---- field -----------------------------------------------------------------

/// A field selector: 0 is the whole line, 1.. counts from the start, -1 (or
/// `NF`) is the last field, -2 the one before.
fn parse_fields(spec: &str) -> Result<Vec<i64>, String> {
    spec.split(',')
        .map(|s| match s {
            "NF" => Ok(-1),
            _ => s
                .parse::<i64>()
                .map_err(|_| format!("'{s}' is not a field number (N, -N or NF)")),
        })
        .collect()
}

/// `field N[,M…] [-d DELIM] [-o SEP] [FILE…]`: prints the selected fields of
/// each line, joined by SEP (default one space), like awk's `{print $N, $M}`.
/// Without -d, fields are split at runs of blanks and leading blanks are
/// skipped (awk, not cut). A field past the end is empty.
pub fn field(ctx: &mut Ctx, a: &[String]) -> i32 {
    let Ok(p) = xgetopt(ctx, a) else {
        return 2;
    };
    let Some((spec, files)) = p.operands.split_first() else {
        ctx.msg("missing field list; usage: field N[,M...] [-d DELIM] [-o SEP] [FILE...]");
        return 2;
    };
    let sel = match parse_fields(spec) {
        Ok(s) => s,
        Err(e) => {
            ctx.msg(&e);
            return 2;
        }
    };
    let delim = p
        .val("-d")
        .map(|v| v[0].clone().into_bytes())
        .filter(|d| d != b" ");
    if delim.as_deref() == Some(b"") {
        ctx.msg("-d needs a non-empty delimiter");
        return 2;
    }
    let sep = p
        .val("-o")
        .map(|v| v[0].clone().into_bytes())
        .unwrap_or_else(|| b" ".to_vec());
    let (ins, status) = inputs(ctx, files);
    let mut out = Vec::new();
    for (_, data) in ins {
        for line in data.split_inclusive(|&b| b == b'\n') {
            let body = line.strip_suffix(b"\n").unwrap_or(line);
            let fields: Vec<&[u8]> = match &delim {
                None => body
                    .split(|b| *b == b' ' || *b == b'\t')
                    .filter(|f| !f.is_empty())
                    .collect(),
                Some(d) => split_on(body, d),
            };
            for (i, &k) in sel.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(&sep);
                }
                let n = fields.len() as i64;
                let idx = if k < 0 { n + k + 1 } else { k };
                if k == 0 {
                    out.extend_from_slice(body);
                } else if idx >= 1 && idx <= n {
                    out.extend_from_slice(fields[(idx - 1) as usize]);
                }
            }
            out.push(b'\n');
        }
    }
    ctx.put(&out);
    status
}

fn split_on<'a>(s: &'a [u8], d: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i + d.len() <= s.len() {
        if &s[i..i + d.len()] == d {
            out.push(&s[last..i]);
            i += d.len();
            last = i;
        } else {
            i += 1;
        }
    }
    out.push(&s[last..]);
    out
}

// ---- lines -----------------------------------------------------------------

enum Sel {
    /// 1-based, inclusive; `None` end means the last line.
    Range(usize, Option<usize>),
    Between(Regex, Regex),
}

fn parse_sel(spec: &str) -> Result<Sel, String> {
    if let Some(rest) = spec.strip_prefix('/') {
        // /RE1/,/RE2/
        let Some((r1, r2)) = rest.split_once("/,/") else {
            return Err(format!("'{spec}': expected /RE1/,/RE2/"));
        };
        let Some(r2) = r2.strip_suffix('/') else {
            return Err(format!("'{spec}': expected /RE1/,/RE2/"));
        };
        let c = |r: &str| Regex::new(r).map_err(|e| format!("invalid regex '{r}': {e}"));
        return Ok(Sel::Between(c(r1)?, c(r2)?));
    }
    let num = |s: &str| -> Result<Option<usize>, String> {
        match s {
            "$" | "" => Ok(None),
            _ => match s.parse::<usize>() {
                Ok(0) => Err("line numbers start at 1".to_string()),
                Ok(n) => Ok(Some(n)),
                Err(_) => Err(format!("'{s}' is not a line number or `$`")),
            },
        }
    };
    match spec.split_once(':') {
        Some((a, b)) => {
            let a = num(a)?.unwrap_or(1);
            Ok(Sel::Range(a, num(b)?))
        }
        None => {
            let a = num(spec)?;
            match a {
                Some(n) => Ok(Sel::Range(n, Some(n))),
                None => Ok(Sel::Range(usize::MAX, None)),
            }
        }
    }
}

/// `lines A:B [FILE…]` prints lines A to B (1-based, inclusive; `$` is the
/// last line, `A:` runs to the end). `lines /RE1/,/RE2/` prints each block
/// from a line matching RE1 through the next line matching RE2, like awk's
/// `/a/,/b/`. `lines $` is the last line.
pub fn lines(ctx: &mut Ctx, a: &[String]) -> i32 {
    let Ok(p) = xgetopt(ctx, a) else {
        return 2;
    };
    let Some((spec, files)) = p.operands.split_first() else {
        ctx.msg("missing range; usage: lines A:B [FILE...] or lines /RE1/,/RE2/ [FILE...]");
        return 2;
    };
    let sel = match parse_sel(spec) {
        Ok(s) => s,
        Err(e) => {
            ctx.msg(&e);
            return 2;
        }
    };
    let (ins, status) = inputs(ctx, files);
    let mut data = Vec::new();
    for (_, d) in ins {
        if !data.is_empty() && !data.ends_with(b"\n") {
            data.push(b'\n');
        }
        data.extend(d);
    }
    let all: Vec<&[u8]> = data.split_inclusive(|&b| b == b'\n').collect();
    let mut out = Vec::new();
    match sel {
        Sel::Range(a, b) => {
            let n = all.len();
            let a = if a == usize::MAX { n } else { a };
            let b = b.unwrap_or(n).min(n);
            if a >= 1 && a <= b {
                for l in &all[a - 1..b] {
                    out.extend_from_slice(l);
                }
            }
        }
        Sel::Between(r1, r2) => {
            let mut inside = false;
            for l in &all {
                let body = l.strip_suffix(b"\n").unwrap_or(l);
                if !inside {
                    if r1.is_match(body) {
                        inside = true;
                        out.extend_from_slice(l);
                        // awk: a line matching both starts and ends the range.
                        if r2.is_match(body) {
                            inside = false;
                        }
                    }
                } else {
                    out.extend_from_slice(l);
                    if r2.is_match(body) {
                        inside = false;
                    }
                }
            }
        }
    }
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    ctx.put(&out);
    status
}

// ---- base64 ----------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63]);
        out.push(B64[(n >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63]
        } else {
            b'='
        });
    }
    out
}

fn b64_decode(data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    let mut pad = 0;
    for &c in data {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return Err(());
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(()),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding, and at most two `=`.
    if pad > 2 || bits >= 6 || acc != 0 {
        return Err(());
    }
    Ok(out)
}

/// `base64 [-d] [-w COLS] [FILE]`: GNU output (76 columns; `-w 0` is one line).
pub fn base64(ctx: &mut Ctx, a: &[String]) -> i32 {
    let Ok(p) = xgetopt(ctx, a) else {
        return 2;
    };
    if p.operands.len() > 1 {
        ctx.msg(&format!("extra operand '{}'", p.operands[1]));
        return 2;
    }
    let wrap = match p.val("-w") {
        Some(v) => match v[0].parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                ctx.msg(&format!("invalid wrap size: '{}'", v[0]));
                return 2;
            }
        },
        None => 76,
    };
    let (ins, status) = inputs(ctx, &p.operands);
    if status != 0 {
        return status;
    }
    let data: Vec<u8> = ins.into_iter().flat_map(|(_, d)| d).collect();
    if p.has("-d") || p.has("--decode") {
        match b64_decode(&data) {
            Ok(d) => {
                ctx.put(&d);
                0
            }
            Err(()) => {
                ctx.msg("invalid input");
                1
            }
        }
    } else {
        let enc = b64_encode(&data);
        let mut out = Vec::with_capacity(enc.len() + enc.len() / 76 + 1);
        if wrap == 0 {
            out.extend_from_slice(&enc);
            out.push(b'\n');
        } else {
            for line in enc.chunks(wrap) {
                out.extend_from_slice(line);
                out.push(b'\n');
            }
        }
        if enc.is_empty() {
            out.clear();
        }
        ctx.put(&out);
        0
    }
}

// ---- sha256sum -------------------------------------------------------------

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// `sha256sum [FILE…]` prints `HASH  NAME` (GNU format); `-c LIST` checks
/// each `HASH  NAME` line of LIST and prints `NAME: OK` or `NAME: FAILED`.
pub fn sha256sum(ctx: &mut Ctx, a: &[String]) -> i32 {
    let Ok(p) = xgetopt(ctx, a) else {
        return 2;
    };
    if p.has("-c") || p.has("--check") {
        return sha_check(ctx, &p.operands);
    }
    let mut status = 0;
    let files = if p.operands.is_empty() {
        vec!["-".to_string()]
    } else {
        p.operands.clone()
    };
    for f in &files {
        let Some(d) = ctx.read_input(f) else {
            status = 1;
            continue;
        };
        let line = format!("{}  {f}\n", hex(&Sha256::digest(&d)));
        ctx.puts(&line);
    }
    status
}

fn sha_check(ctx: &mut Ctx, lists: &[String]) -> i32 {
    let (ins, mut status) = inputs(ctx, lists);
    let (mut failed, mut unread, mut bad_lines) = (0, 0, 0);
    for (_, data) in ins {
        for line in String::from_utf8_lossy(&data).lines() {
            if line.is_empty() {
                continue;
            }
            let parsed = line.split_once(' ').and_then(|(h, rest)| {
                let name = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('*'))?;
                (h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
                    .then(|| (h.to_ascii_lowercase(), name.to_string()))
            });
            let Some((want, name)) = parsed else {
                bad_lines += 1;
                continue;
            };
            let got = match ctx.read_input(&name) {
                Some(d) => hex(&Sha256::digest(&d)),
                None => {
                    ctx.puts(&format!("{name}: FAILED open or read\n"));
                    unread += 1;
                    continue;
                }
            };
            if got == want {
                ctx.puts(&format!("{name}: OK\n"));
            } else {
                ctx.puts(&format!("{name}: FAILED\n"));
                failed += 1;
            }
        }
    }
    let plural = |n: i32, one: &str, many: &str| {
        if n == 1 {
            one.to_string()
        } else {
            many.to_string()
        }
    };
    if bad_lines > 0 {
        let m = format!(
            "WARNING: {bad_lines} {} improperly formatted",
            plural(bad_lines, "line is", "lines are")
        );
        ctx.msg(&m);
        status = 1;
    }
    if unread > 0 {
        let m = format!(
            "WARNING: {unread} listed {} could not be read",
            plural(unread, "file", "files")
        );
        ctx.msg(&m);
        status = 1;
    }
    if failed > 0 {
        let m = format!(
            "WARNING: {failed} computed {} did NOT match",
            plural(failed, "checksum", "checksums")
        );
        ctx.msg(&m);
        status = 1;
    }
    status
}
