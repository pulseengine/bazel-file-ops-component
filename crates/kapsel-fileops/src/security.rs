//! Path confinement, ported rule for rule from `tinygo/security.go`.
//!
//! The rules:
//! - a destination resolves inside `workspace_dir`, compared at a path
//!   separator boundary after every symlink on the way is followed;
//! - a path with a `..` component is refused before it is resolved;
//! - a dangling symlink on the way is refused;
//! - writing or creating through an existing symlink is refused;
//! - run_command's `work_dir` and `output_file` resolve inside
//!   `workspace_dir` or its parent.
//!
//! These checks run on top of the WASI sandbox, not instead of it.

use std::io;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::gopath::{base, clean, dir, is_abs, join, rel};

/// TinyGo's `SecurityLevel`: 0 standard, 1 high, 2 strict. Only
/// `prepare_workspace` with a `security_config` changes it.
static LEVEL: AtomicI64 = AtomicI64::new(0);

pub fn set_security_level(level: i64) {
    LEVEL.store(level, Ordering::Relaxed);
}

/// `filepath.Abs` for an absolute path; a relative path stays relative and
/// is resolved against the working directory, which on WASI is the `.`
/// preopen (REQ-KS-017).
fn abs(p: &str) -> String {
    clean(p)
}

/// Go's `filepath.EvalSymlinks` (its `walkSymlinks`), one component at a
/// time with `lstat` and `readlink`.
pub fn eval_symlinks(path: &str) -> io::Result<String> {
    let mut path = path.to_string();
    let mut vol_len = if path.starts_with('/') { 1 } else { 0 };
    let mut vol = path[..vol_len].to_string();
    let mut dest = vol.clone();
    let mut links = 0;
    let mut start = vol_len;
    while start < path.len() {
        let b = path.as_bytes();
        while start < b.len() && b[start] == b'/' {
            start += 1;
        }
        let mut end = start;
        while end < b.len() && b[end] != b'/' {
            end += 1;
        }
        let comp = &path[start..end];
        if end == start {
            break;
        } else if comp == "." {
            start = end;
            continue;
        } else if comp == ".." {
            match dest.rfind('/').filter(|r| *r >= vol_len) {
                Some(r) if &dest[r + 1..] != ".." => dest.truncate(r),
                _ => {
                    if dest.len() > vol_len {
                        dest.push('/');
                    }
                    dest.push_str("..");
                }
            }
            start = end;
            continue;
        }
        if dest.len() > vol_len && !dest.ends_with('/') {
            dest.push('/');
        }
        dest.push_str(comp);
        let md = crate::gofs::lstat(&dest)?;
        if !md.file_type().is_symlink() {
            if !md.is_dir() && end < path.len() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "not a directory",
                ));
            }
            start = end;
            continue;
        }
        links += 1;
        if links > 255 {
            return Err(io::Error::other("EvalSymlinks: too many links"));
        }
        let link = crate::gofs::read_link(&dest)?;
        path = format!("{link}{}", &path[end..]);
        if link.starts_with('/') {
            dest = "/".to_string();
            vol = "/".to_string();
            vol_len = 1;
            start = 1;
        } else {
            match dest.rfind('/').filter(|r| *r >= vol_len) {
                Some(r) => dest.truncate(r),
                None => dest = vol.clone(),
            }
            start = 0;
        }
    }
    Ok(clean(&dest))
}

/// `resolvePath`: the symlink-resolved form of `p`. Missing trailing
/// components are appended unresolved to the deepest existing ancestor; a
/// dangling symlink on the way is an error.
pub fn resolve_path(p: &str) -> Result<String, String> {
    let abs = abs(p);
    let mut existing = abs.clone();
    let mut rest: Vec<String> = Vec::new();
    loop {
        match eval_symlinks(&existing) {
            Ok(resolved) => {
                let mut parts: Vec<&str> = vec![&resolved];
                parts.extend(rest.iter().map(String::as_str));
                return Ok(join(&parts));
            }
            Err(e) if crate::goerr::is_not_exist(&e) => {}
            Err(e) => return Err(e.to_string()),
        }
        if crate::gofs::lstat(&existing).is_ok() {
            return Err(format!("dangling symlink: {existing}"));
        }
        let parent = dir(&existing);
        if parent == existing {
            return Ok(abs);
        }
        rest.insert(0, base(&existing));
        existing = parent;
    }
}

/// `isWithin`: `p` resolves to `root` or below it.
pub fn is_within(root: &str, p: &str) -> bool {
    let (Ok(r), Ok(q)) = (resolve_path(root), resolve_path(p)) else {
        return false;
    };
    match rel(&r, &q) {
        Ok(rel) if !is_abs(&rel) => rel != ".." && !rel.starts_with("../"),
        _ => false,
    }
}

/// `hasDotDotComponent` on the raw path.
pub fn has_dot_dot(p: &str) -> bool {
    p.split('/').any(|c| c == "..")
}

/// `refuseSymlink`: an existing symlink at `p` is an error.
pub fn refuse_symlink(p: &str) -> Result<(), String> {
    match crate::gofs::lstat(p) {
        Ok(md) if md.file_type().is_symlink() => {
            Err(format!("refusing to write through symlink {p}"))
        }
        _ => Ok(()),
    }
}

/// `requireWithin`.
pub fn require_within(root: &str, p: &str, what: &str) -> Result<(), String> {
    if has_dot_dot(root) || has_dot_dot(p) {
        return Err(format!("{what} {p}: \"..\" components are not allowed"));
    }
    if !is_within(root, p) {
        return Err(format!("{what} {p} resolves outside {root}"));
    }
    Ok(())
}

/// `containsPathTraversal`: a `..` component that survives cleaning.
pub fn contains_path_traversal(p: &str) -> bool {
    clean(p).split('/').any(|c| c == "..")
}

/// `ValidatePath(path, []string{})` under the current security level.
pub fn validate_path(p: &str) -> Result<(), String> {
    if contains_path_traversal(p) {
        return Err(format!("path contains path traversal attempts: {p}"));
    }
    match LEVEL.load(Ordering::Relaxed) {
        0 | 1 => Ok(()),
        2 => Err("strict security mode requires explicit allowed directories".to_string()),
        _ => Err("unknown security level".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ported from TestContainsPathTraversal.
    #[test]
    fn path_traversal() {
        for (p, want) in [
            ("../x", true),
            ("a/../../x", true),
            ("..", true),
            ("a..b", false),
            ("a/b..c/d", false),
            ("/a/../b", false),
            ("a/b", false),
        ] {
            assert_eq!(contains_path_traversal(p), want, "{p}");
        }
    }

    #[test]
    fn dot_dot_component() {
        assert!(has_dot_dot("/w/link/../x"));
        assert!(!has_dot_dot("/w/a..b"));
    }
}
