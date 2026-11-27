//! Path resolution and grant checks (REQ-KS-022, fail closed).
//!
//! Rules, settled in the TinyGo component (tinygo/security.go, GHSA-f5c6-8h5q-6486
//! and GHSA-hm34-86h8-vwx2):
//! - `..` is applied after the symlinks before it are followed (as the kernel does);
//! - `resolve` expands every symlink on the way, dangling ones included, and
//!   fails (denies) on a loop instead of guessing;
//! - nothing is written through an existing symlink in the final component;
//! - a path outside every grant is treated as if it did not exist: no probe
//!   (`stat`, `readdir`) runs on it.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::{GrantSet, HostError};

/// Why a path was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantErr {
    Outside,
    Symlink,
    Loop,
}

impl GrantErr {
    /// The text after `kapsel: <path>: `.
    pub fn reason(self) -> &'static str {
        match self {
            GrantErr::Outside => "outside the session's grants",
            GrantErr::Symlink => "refusing to write through symlink",
            GrantErr::Loop => "cannot resolve (symlink loop)",
        }
    }
}

/// A grant set with every directory canonicalized.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    pub read_write: Vec<PathBuf>,
    pub read_only: Vec<PathBuf>,
}


impl Grants {
    pub fn new(set: &GrantSet) -> Result<Self, HostError> {
        let canon = |p: &PathBuf| -> Result<PathBuf, HostError> {
            let c = p
                .canonicalize()
                .map_err(|e| HostError(format!("grant {}: {}", p.display(), e)))?;
            if !c.is_dir() {
                return Err(HostError(format!("grant {}: not a directory", p.display())));
            }
            Ok(c)
        };
        Ok(Self {
            read_write: set.read_write.iter().map(canon).collect::<Result<_, _>>()?,
            read_only: set.read_only.iter().map(canon).collect::<Result<_, _>>()?,
        })
    }

    pub fn can_write(&self, resolved: &Path) -> bool {
        self.read_write.iter().any(|g| resolved.starts_with(g))
    }

    pub fn can_read(&self, resolved: &Path) -> bool {
        self.can_write(resolved) || self.read_only.iter().any(|g| resolved.starts_with(g))
    }

    /// Resolves an operand for reading and checks it against the grants.
    pub fn check_read(&self, cwd: &Path, arg: &str) -> Result<PathBuf, GrantErr> {
        let r = resolve(cwd, Path::new(arg))?;
        if self.can_read(&r) { Ok(r) } else { Err(GrantErr::Outside) }
    }

    /// Resolves an operand for writing. With `follow_last`, an existing
    /// symlink in the last component is refused (it would be written through);
    /// without it, the link itself is the target (rm, mv).
    pub fn check_write(&self, cwd: &Path, arg: &str, follow_last: bool) -> Result<PathBuf, GrantErr> {
        let p = Path::new(arg);
        let nofollow = resolve_nofollow_last(cwd, p)?;
        if follow_last {
            if let Ok(md) = std::fs::symlink_metadata(&nofollow) {
                if md.file_type().is_symlink() {
                    // Judge the link only if it lies inside the grants
                    // (nothing outside is probed). A loop or a target outside
                    // is reported as such, any other link as a symlink.
                    if !self.can_write(&nofollow) {
                        return Err(GrantErr::Outside);
                    }
                    return Err(match resolve(cwd, p) {
                        Err(e) => e,
                        Ok(r) if !self.can_write(&r) => GrantErr::Outside,
                        Ok(_) => GrantErr::Symlink,
                    });
                }
            }
            let r = resolve(cwd, p)?;
            if self.can_write(&r) { Ok(r) } else { Err(GrantErr::Outside) }
        } else if self.can_write(&nofollow) {
            Ok(nofollow)
        } else {
            Err(GrantErr::Outside)
        }
    }

    /// Checks an absolute destination during a recursive copy: its parent
    /// must resolve inside a read-write grant and the entry itself must not
    /// be an existing symlink.
    pub fn check_dest_abs(&self, dst: &Path) -> Result<(), GrantErr> {
        let parent = dst.parent().unwrap_or(Path::new("/"));
        let name = dst.file_name().ok_or(GrantErr::Outside)?;
        let rp = resolve(Path::new("/"), parent)?;
        let full = rp.join(name);
        if !self.can_write(&full) {
            return Err(GrantErr::Outside);
        }
        if let Ok(md) = std::fs::symlink_metadata(&full) {
            if md.file_type().is_symlink() {
                return Err(GrantErr::Symlink);
            }
        }
        Ok(())
    }
}

/// Resolves `path` (made absolute against `cwd`) following every symlink.
/// Components that do not exist are kept as written. More than 40 links, or an
/// unreadable link, is an error (the caller denies).
pub fn resolve(cwd: &Path, path: &Path) -> Result<PathBuf, GrantErr> {
    let abs = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    let mut pending: Vec<OsString> = Vec::new();
    let mut cur = PathBuf::from("/");
    let push_comps = |pending: &mut Vec<OsString>, p: &Path| {
        let mut v: Vec<OsString> = p
            .components()
            .map(|c| match c {
                Component::RootDir => OsString::from("/"),
                Component::CurDir => OsString::from("."),
                Component::ParentDir => OsString::from(".."),
                Component::Normal(n) => n.to_os_string(),
                Component::Prefix(p) => p.as_os_str().to_os_string(),
            })
            .collect();
        v.reverse();
        pending.extend(v);
    };
    push_comps(&mut pending, &abs);
    let mut links = 0;
    while let Some(c) = pending.pop() {
        if c == "/" {
            cur = PathBuf::from("/");
        } else if c == "." {
        } else if c == ".." {
            cur.pop();
        } else {
            let next = cur.join(&c);
            match std::fs::symlink_metadata(&next) {
                Ok(md) if md.file_type().is_symlink() => {
                    links += 1;
                    if links > 40 {
                        return Err(GrantErr::Loop);
                    }
                    let target = std::fs::read_link(&next).map_err(|_| GrantErr::Loop)?;
                    push_comps(&mut pending, &target);
                }
                _ => cur = next,
            }
        }
    }
    Ok(cur)
}

/// Like `resolve`, but a symlink in the last component is not followed.
pub fn resolve_nofollow_last(cwd: &Path, path: &Path) -> Result<PathBuf, GrantErr> {
    let abs = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    match (abs.parent(), abs.components().next_back()) {
        (Some(parent), Some(Component::Normal(name))) => Ok(resolve(cwd, parent)?.join(name)),
        _ => resolve(cwd, &abs),
    }
}
