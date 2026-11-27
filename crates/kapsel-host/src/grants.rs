//! Path resolution and grant checks (REQ-KS-022, fail closed).
//!
//! `resolve` walks a path component by component and expands every symlink on
//! the way, dangling ones included, so a link inside a granted directory that
//! points outside is judged by where it points, not where it lives.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::{GrantSet, HostError};

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
}

/// Resolves `path` (made absolute against `cwd`) following every symlink.
/// Components that do not exist are kept as written.
pub fn resolve(cwd: &Path, path: &Path) -> PathBuf {
    let abs = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    let mut pending: Vec<OsString> = Vec::new();
    let mut cur = PathBuf::from("/");
    // Components are consumed from the back of `pending` (reversed).
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
                        // Loop: give up on the target, keep the link itself.
                        cur = next;
                        continue;
                    }
                    match std::fs::read_link(&next) {
                        Ok(target) => push_comps(&mut pending, &target),
                        Err(_) => cur = next,
                    }
                }
                _ => cur = next,
            }
        }
    }
    cur
}

/// Like `resolve`, but a symlink in the last component is not followed (for
/// `rm`, `mv` source: they act on the link itself).
pub fn resolve_nofollow_last(cwd: &Path, path: &Path) -> PathBuf {
    let abs = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    match (abs.parent(), abs.components().next_back()) {
        (Some(parent), Some(Component::Normal(name))) => resolve(cwd, parent).join(name),
        _ => resolve(cwd, &abs),
    }
}
