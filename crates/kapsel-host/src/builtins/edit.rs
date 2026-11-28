//! `edit`: in-place file edits (REQ-KS-063, DD-KS-020).
//!
//! ```text
//! edit FILE -s OLD NEW            replace the literal text OLD
//! edit FILE -e REGEX REPL         Rust regex; `$1`, `${name}` in REPL; `^`/`$` match at lines
//!   -m                            `.` also matches a newline
//!   -c N                          exactly N replacements, or exit 1 and no write
//!   --allow-none                  zero matches is not an error
//! edit FILE --lines A:B --with FILE2 | --with-text TEXT
//!                                 replace lines A..=B (1-based, `$` = last);
//!                                 `A:A-1` inserts before line A
//! ```
//!
//! An edit that matches nothing exits 1 and leaves the file as it was: a
//! silent no-op edit is the failure kapsel exists to stop. The new content is
//! written to a temporary file in the same directory and renamed over FILE,
//! so FILE is never half written.

use regex::bytes::RegexBuilder;

use super::{Ctx, xgetopt};
use crate::runerr::ErrorClass;

const USAGE: &str = "usage: edit FILE (-s OLD NEW | -e REGEX REPL [-m] | --lines A:B (--with FILE2 | --with-text TEXT)) [-c N] [--allow-none]";

fn usage(ctx: &mut Ctx, why: &str) -> i32 {
    ctx.msg(&format!("{why}; {USAGE}"));
    2
}

/// Non-overlapping occurrences of `needle` in `hay`, as start offsets.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// `A:B` with `$` for the last line. Returns (a, b), 1-based; b may be a-1.
fn parse_range(spec: &str, nlines: usize) -> Result<(usize, usize), String> {
    let num = |s: &str| -> Result<usize, String> {
        if s == "$" {
            Ok(nlines)
        } else {
            s.parse::<usize>()
                .map_err(|_| format!("'{s}' is not a line number or `$`"))
        }
    };
    let (a, b) = match spec.split_once(':') {
        Some((a, b)) => (num(a)?, num(b)?),
        None => {
            let a = num(spec)?;
            (a, a)
        }
    };
    if a == 0 {
        return Err("line numbers start at 1".to_string());
    }
    if a > nlines + 1 || b > nlines || b + 1 < a {
        return Err(format!(
            "range {spec} is outside the file's {nlines} line{}",
            if nlines == 1 { "" } else { "s" }
        ));
    }
    Ok((a, b))
}

pub fn edit(ctx: &mut Ctx, a: &[String]) -> i32 {
    let Ok(p) = xgetopt(ctx, a) else {
        return 2;
    };
    let file = match p.operands.as_slice() {
        [f] => f.clone(),
        [] => return usage(ctx, "missing FILE"),
        more => {
            return usage(
                ctx,
                &format!("one FILE per edit, got {}: {}", more.len(), more.join(" ")),
            );
        }
    };
    let modes = ["-s", "-e", "--lines"].iter().filter(|m| p.has(m)).count();
    let one_each = ["-s", "-e", "--lines", "--with", "--with-text", "-c"]
        .iter()
        .all(|f| p.flags.iter().filter(|(n, _)| n == f).count() <= 1);
    if modes != 1 || !one_each {
        return usage(ctx, "give exactly one of -s, -e or --lines, once");
    }
    if p.has("-m") && !p.has("-e") {
        return usage(ctx, "-m applies to -e");
    }
    let count = match p.val("-c") {
        Some(v) => match v[0].parse::<usize>() {
            Ok(n) => Some(n),
            Err(_) => return usage(ctx, &format!("-c needs a count, got '{}'", v[0])),
        },
        None => None,
    };
    let allow_none = p.has("--allow-none");

    if let Some(range) = p.val("--lines") {
        let range = range[0].clone();
        if count.is_some() || allow_none {
            return usage(ctx, "-c and --allow-none apply to -s and -e");
        }
        let mut with = match (p.val("--with"), p.val("--with-text")) {
            (Some(f), None) => {
                let f = f[0].clone();
                match ctx.read_input(&f) {
                    Some(d) => d,
                    None => return 1,
                }
            }
            (None, Some(t)) => t[0].clone().into_bytes(),
            _ => return usage(ctx, "--lines needs one of --with FILE2 or --with-text TEXT"),
        };
        if !with.is_empty() && !with.ends_with(b"\n") {
            with.push(b'\n');
        }
        let Some((path, data)) = ctx.edit_target(&file) else {
            return 1;
        };
        let lines: Vec<&[u8]> = data.split_inclusive(|&b| b == b'\n').collect();
        let (from, to) = match parse_range(&range, lines.len()) {
            Ok(r) => r,
            Err(e) => {
                ctx.msg(&format!("{file}: {e}; file unchanged"));
                return 1;
            }
        };
        let mut out: Vec<u8> = lines[..from - 1].concat();
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(&with);
        out.extend(lines[to..].concat());
        return ctx.commit(&file, &path, &out);
    }
    if p.has("--with") || p.has("--with-text") {
        return usage(ctx, "--with and --with-text apply to --lines");
    }

    let (out, n, what) = if let Some(v) = p.val("-s") {
        let (old, new) = (v[0].clone(), v[1].clone());
        if old.is_empty() {
            return usage(ctx, "-s needs a non-empty OLD");
        }
        let Some((path, data)) = ctx.edit_target(&file) else {
            return 1;
        };
        let at = find_all(&data, old.as_bytes());
        let mut out = Vec::with_capacity(data.len());
        let mut last = 0;
        for &i in &at {
            out.extend_from_slice(&data[last..i]);
            out.extend_from_slice(new.as_bytes());
            last = i + old.len();
        }
        out.extend_from_slice(&data[last..]);
        ((path, data, out), at.len(), format!("'{old}'"))
    } else {
        let v = p.val("-e").unwrap_or_default();
        let (re, repl) = (v[0].clone(), v[1].clone());
        let rx = match RegexBuilder::new(&re)
            .multi_line(true)
            .dot_matches_new_line(p.has("-m"))
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                let full = e.to_string();
                let first = full.lines().last().unwrap_or("").to_string();
                ctx.regex_fail(
                    &re,
                    &format!("invalid regex '{re}': {}", first.trim()),
                    Some(&full),
                );
                return 2;
            }
        };
        let Some((path, data)) = ctx.edit_target(&file) else {
            return 1;
        };
        let n = rx.find_iter(&data).count();
        let out = rx.replace_all(&data, repl.as_bytes()).into_owned();
        ((path, data, out), n, format!("/{re}/"))
    };
    let (path, data, out) = out;
    if let Some(want) = count
        && n != want
    {
        ctx.fail(
            ErrorClass::EditNoMatch,
            Some(&file),
            &format!(
                "{file}: {what} matched {n} time{}, -c requires {want}; file unchanged",
                if n == 1 { "" } else { "s" }
            ),
        );
        return 1;
    }
    if n == 0 {
        if allow_none {
            return 0;
        }
        ctx.fail(
            ErrorClass::EditNoMatch,
            Some(&file),
            &format!("{file}: {what} not found; file unchanged (--allow-none accepts no match)"),
        );
        return 1;
    }
    if out == data {
        return 0;
    }
    ctx.commit(&file, &path, &out)
}
