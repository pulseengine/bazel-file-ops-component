//! POSIX BRE/ERE to the `regex` crate's syntax.

use regex::bytes::{Regex, RegexBuilder};

fn esc(c: char) -> String {
    regex::escape(&c.to_string())
}

fn bracket(chars: &[char], i: &mut usize, out: &mut String) -> Result<(), String> {
    // chars[*i] == '['
    let mut j = *i + 1;
    let mut s = String::from("[");
    if chars.get(j) == Some(&'^') {
        s.push('^');
        j += 1;
    }
    let first = j;
    loop {
        let Some(&c) = chars.get(j) else {
            return Err("unmatched [ (unterminated bracket expression)".into());
        };
        if c == ']' && j > first {
            s.push(']');
            *i = j + 1;
            out.push_str(&s);
            return Ok(());
        }
        if c == '[' && matches!(chars.get(j + 1), Some(':')) {
            let mut k = j + 2;
            while k + 1 < chars.len() && !(chars[k] == ':' && chars[k + 1] == ']') {
                k += 1;
            }
            if k + 1 >= chars.len() {
                return Err("unmatched [ (unterminated character class)".into());
            }
            s.extend(&chars[j..k + 2]);
            j = k + 2;
            continue;
        }
        if c == '[' && matches!(chars.get(j + 1), Some('.') | Some('=')) {
            return Err("collating symbols and equivalence classes are not supported".into());
        }
        match c {
            '\\' | '[' | ']' | '&' | '~' => {
                s.push('\\');
                s.push(c);
            }
            _ => s.push(c),
        }
        j += 1;
    }
}

/// Translates a POSIX pattern. `ere` selects extended syntax.
pub fn translate_regex(pat: &str, ere: bool) -> Result<String, String> {
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut at_start = true;
    while i < chars.len() {
        let c = chars[i];
        let was_start = at_start;
        at_start = false;
        match c {
            '\\' => {
                i += 1;
                let Some(&n) = chars.get(i) else {
                    return Err("trailing backslash (\\)".into());
                };
                i += 1;
                match n {
                    '(' | '|' if !ere => {
                        out.push(n);
                        at_start = true;
                    }
                    ')' | '{' | '}' | '+' | '?' if !ere => out.push(n),
                    '<' | '>' => out.push_str("\\b"),
                    'w' | 'W' | 's' | 'S' | 'b' | 'B' => {
                        out.push('\\');
                        out.push(n);
                    }
                    '1'..='9' => return Err("back-references are not supported".into()),
                    'n' => out.push_str("\\n"),
                    _ => out.push_str(&esc(n)),
                }
                continue;
            }
            '[' => {
                bracket(&chars, &mut i, &mut out)?;
                continue;
            }
            '*' if was_start => out.push_str("\\*"),
            '*' => out.push('*'),
            '.' => out.push('.'),
            '^' => {
                if ere || was_start {
                    out.push('^');
                    at_start = !ere;
                } else {
                    out.push_str("\\^");
                }
            }
            '$' => {
                let end = i + 1 == chars.len()
                    || (!ere && matches!(chars.get(i + 1), Some('\\')) && matches!(chars.get(i + 2), Some(')') | Some('|')));
                if ere || end {
                    out.push('$');
                } else {
                    out.push_str("\\$");
                }
            }
            '(' | ')' | '|' | '+' | '?' if ere => {
                out.push(c);
                if c == '(' || c == '|' {
                    at_start = true;
                }
            }
            '{' if ere => {
                // a valid interval is copied; otherwise a literal brace
                let rest: String = chars[i..].iter().collect();
                let re = regex::Regex::new(r"^\{[0-9]+(,[0-9]*)?\}").unwrap();
                if let Some(m) = re.find(&rest) {
                    out.push_str(m.as_str());
                    i += m.as_str().chars().count();
                    continue;
                }
                out.push_str("\\{");
            }
            _ => out.push_str(&esc(c)),
        }
        i += 1;
    }
    Ok(out)
}

/// Compiles a POSIX pattern; the error is a short reason.
pub fn compile(pat: &str, ere: bool, icase: bool) -> Result<Regex, String> {
    let t = translate_regex(pat, ere)?;
    build(&t, icase)
}

pub fn build(translated: &str, icase: bool) -> Result<Regex, String> {
    RegexBuilder::new(translated)
        .case_insensitive(icase)
        .multi_line(false)
        .build()
        .map_err(|e| short_error(&e.to_string()))
}

fn short_error(s: &str) -> String {
    for l in s.lines().rev() {
        if let Some(r) = l.trim().strip_prefix("error: ") {
            return r.to_string();
        }
    }
    s.lines().last().unwrap_or(s).trim().to_string()
}
