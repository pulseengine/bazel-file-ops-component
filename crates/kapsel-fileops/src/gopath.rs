//! Go's `path/filepath` on Unix, which the TinyGo component's messages and
//! path decisions are made of.

/// `filepath.IsAbs`.
pub fn is_abs(p: &str) -> bool {
    p.starts_with('/')
}

/// `filepath.Clean`.
pub fn clean(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let rooted = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !rooted {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    let body = out.join("/");
    match (rooted, body.is_empty()) {
        (true, _) => format!("/{body}"),
        (false, true) => ".".to_string(),
        (false, false) => body,
    }
}

/// `filepath.Join`.
pub fn join(parts: &[&str]) -> String {
    let nonempty: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    if nonempty.is_empty() {
        return String::new();
    }
    clean(&nonempty.join("/"))
}

/// `filepath.Dir`.
pub fn dir(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => clean(&p[..=i]),
        None => ".".to_string(),
    }
}

/// `filepath.Base`.
pub fn base(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    match trimmed.rfind('/') {
        Some(i) => trimmed[i + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// `filepath.Rel`.
pub fn rel(basepath: &str, targpath: &str) -> Result<String, String> {
    let fail = || Err(format!("Rel: can't make {targpath} relative to {basepath}"));
    let mut base = clean(basepath);
    let targ = clean(targpath);
    if targ == base {
        return Ok(".".to_string());
    }
    if base == "." {
        base.clear();
    }
    let base_slashed = base.starts_with('/');
    let targ_slashed = targ.starts_with('/');
    if base_slashed != targ_slashed {
        return fail();
    }
    let (b, t) = (base.as_bytes(), targ.as_bytes());
    let (bl, tl) = (b.len(), t.len());
    let (mut b0, mut bi, mut t0, mut ti) = (0, 0, 0, 0);
    loop {
        while bi < bl && b[bi] != b'/' {
            bi += 1;
        }
        while ti < tl && t[ti] != b'/' {
            ti += 1;
        }
        if t[t0..ti] != b[b0..bi] {
            break;
        }
        if bi < bl {
            bi += 1;
        }
        if ti < tl {
            ti += 1;
        }
        b0 = bi;
        t0 = ti;
    }
    if &b[b0..bi] == b".." {
        return fail();
    }
    if b0 != bl {
        let seps = b[b0..bl].iter().filter(|c| **c == b'/').count();
        let mut out = String::from("..");
        for _ in 0..seps {
            out.push_str("/..");
        }
        if t0 != tl {
            out.push('/');
            out.push_str(&targ[t0..]);
        }
        return Ok(out);
    }
    Ok(targ[t0..].to_string())
}

/// `strconv.Quote`, for the names in os/exec errors.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c if (0x80..0xa0).contains(&(c as u32)) || c == '\u{FEFF}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_matches_go() {
        for (i, o) in [
            ("", "."),
            ("/", "/"),
            ("a/b/../c", "a/c"),
            ("../../x", "../../x"),
            ("/../x", "/x"),
            ("a/../..", ".."),
            ("./a/./b/", "a/b"),
            ("//a//b", "/a/b"),
        ] {
            assert_eq!(clean(i), o, "{i}");
        }
    }

    /// Ported from TestJoinPaths, TestGetDirname, TestGetBasename.
    #[test]
    fn join_dir_base_match_go() {
        assert_eq!(join(&["a", "b", "c"]), "a/b/c");
        assert_eq!(join(&["", "b", "c"]), "b/c");
        assert_eq!(join(&["a"]), "a");
        assert_eq!(join(&[]), "");
        assert_eq!(dir("/path/to/file.txt"), "/path/to");
        assert_eq!(dir("file.txt"), ".");
        assert_eq!(dir("/path/to/"), "/path/to");
        assert_eq!(dir(""), ".");
        assert_eq!(base("/path/to/file.txt"), "file.txt");
        assert_eq!(base("file.txt"), "file.txt");
        assert_eq!(base("/path/to/"), "to");
        assert_eq!(base(""), ".");
    }

    #[test]
    fn rel_matches_go() {
        assert_eq!(rel("/a", "/a/b").unwrap(), "b");
        assert_eq!(rel("/a", "/a").unwrap(), ".");
        assert_eq!(rel("/a/b", "/a").unwrap(), "..");
        assert_eq!(rel("/work", "/work-other").unwrap(), "../work-other");
        assert_eq!(rel(".", "x/y").unwrap(), "x/y");
        assert_eq!(rel(".", "../x").unwrap(), "../x");
        assert!(rel("/a", "b").is_err());
    }
}
