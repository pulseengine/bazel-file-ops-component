//! mkdir rm cp mv ls

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::timefmt;
use super::{Ctx, Spec, getopt};
use crate::grants;

pub fn mkdir(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "pv", long: &[("parents", 'p'), ("verbose", 'v')], numeric: false, supported: "-p -v" };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    if p.operands.is_empty() {
        ctx.msg("missing operand");
        return 1;
    }
    let mut status = 0;
    for d in &p.operands {
        let Some(path) = ctx.wpath(d, true) else {
            status = 1;
            continue;
        };
        let r = if p.has('p') {
            if path.is_dir() { Ok(()) } else { fs::create_dir_all(&path) }
        } else {
            fs::create_dir(&path)
        };
        match r {
            Ok(()) => {
                if p.has('v') {
                    ctx.puts(&format!("mkdir: created directory '{d}'\n"));
                }
            }
            Err(e) => {
                ctx.msg(&format!("cannot create directory '{d}': {}", crate::shell::errstr(&e)));
                status = 1;
            }
        }
    }
    status
}

pub fn rm(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "rRfv", long: &[("recursive", 'r'), ("force", 'f'), ("verbose", 'v')], numeric: false, supported: "-r -R -f -v" };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    let (rec, force) = (p.has('r') || p.has('R'), p.has('f'));
    if p.operands.is_empty() {
        if force {
            return 0;
        }
        ctx.msg("missing operand");
        return 1;
    }
    let mut status = 0;
    for f in &p.operands {
        let base = f.trim_end_matches('/');
        let last = base.rsplit('/').next().unwrap_or("");
        if last == "." || last == ".." {
            ctx.msg(&format!("refusing to remove '.' or '..' directory: skipping '{f}'"));
            status = 1;
            continue;
        }
        let Some(path) = ctx.wpath(if base.is_empty() { f } else { base }, false) else {
            status = 1;
            continue;
        };
        let md = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                if !(force && e.kind() == std::io::ErrorKind::NotFound) {
                    ctx.msg(&format!("cannot remove '{f}': {}", crate::shell::errstr(&e)));
                    status = 1;
                }
                continue;
            }
        };
        let r = if md.is_dir() {
            if !rec {
                ctx.msg(&format!("cannot remove '{f}': Is a directory"));
                status = 1;
                continue;
            }
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        match r {
            Ok(()) => {
                if p.has('v') {
                    ctx.puts(&format!("removed '{f}'\n"));
                }
            }
            Err(e) => {
                ctx.msg(&format!("cannot remove '{f}': {}", crate::shell::errstr(&e)));
                status = 1;
            }
        }
    }
    status
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        let t = fs::read_link(src)?;
        let _ = fs::remove_file(dst);
        std::os::unix::fs::symlink(t, dst)
    } else if md.is_dir() {
        if !dst.is_dir() {
            fs::create_dir(dst)?;
        }
        let mut names: Vec<_> = fs::read_dir(src)?.filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        names.sort();
        for n in names {
            copy_tree(&src.join(&n), &dst.join(&n))?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

fn basename(arg: &str) -> Option<String> {
    Path::new(arg.trim_end_matches('/')).file_name().map(|n| n.to_string_lossy().into_owned())
}

pub fn cp(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "rRfv", long: &[("recursive", 'r'), ("force", 'f'), ("verbose", 'v')], numeric: false, supported: "-r -R -f -v" };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    let rec = p.has('r') || p.has('R');
    transfer(ctx, &p.operands, false, rec, p.has('v'))
}

pub fn mv(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "fv", long: &[("force", 'f'), ("verbose", 'v')], numeric: false, supported: "-f -v" };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    transfer(ctx, &p.operands, true, true, p.has('v'))
}

fn transfer(ctx: &mut Ctx, ops: &[String], moving: bool, rec: bool, verbose: bool) -> i32 {
    if ops.is_empty() {
        ctx.msg("missing file operand");
        return 1;
    }
    if ops.len() == 1 {
        let o = ops[0].clone();
        ctx.msg(&format!("missing destination file operand after '{o}'"));
        return 1;
    }
    let (srcs, dest) = ops.split_at(ops.len() - 1);
    let dest = &dest[0];
    let Some(dres) = ctx.wpath(dest, true) else { return 1 };
    let dest_is_dir = dres.is_dir();
    if srcs.len() > 1 && !dest_is_dir {
        ctx.msg(&format!("target '{dest}' is not a directory"));
        return 1;
    }
    let mut status = 0;
    for s in srcs {
        let sres = if moving { ctx.wpath(s, false) } else { ctx.rpath(s) };
        let Some(sres) = sres else {
            status = 1;
            continue;
        };
        let smd = match if moving { fs::symlink_metadata(&sres) } else { fs::metadata(&sres) } {
            Ok(m) => m,
            Err(e) => {
                ctx.msg(&format!("cannot stat '{s}': {}", crate::shell::errstr(&e)));
                status = 1;
                continue;
            }
        };
        let target: PathBuf = if dest_is_dir {
            match basename(s) {
                Some(b) => dres.join(b),
                None => dres.clone(),
            }
        } else {
            dres.clone()
        };
        // Target text for messages and the grant check on the final path.
        let tshown = if dest_is_dir { format!("{}/{}", dest.trim_end_matches('/'), basename(s).unwrap_or_default()) } else { dest.clone() };
        let tres = grants::resolve(&ctx.sh.cwd, &target);
        if !ctx.sh.grants.can_write(&tres) {
            ctx.grants_msg(&tshown);
            status = 1;
            continue;
        }
        if let (Ok(a), Ok(b)) = (fs::canonicalize(&sres), fs::canonicalize(&tres)) {
            if a == b {
                ctx.msg(&format!("'{s}' and '{tshown}' are the same file"));
                status = 1;
                continue;
            }
        }
        if smd.is_dir() && tres.starts_with(&sres) {
            ctx.msg(&format!("cannot {} a directory, '{s}', into itself, '{tshown}'", if moving { "move" } else { "copy" }));
            status = 1;
            continue;
        }
        if smd.is_dir() && !rec {
            ctx.msg(&format!("-r not specified; omitting directory '{s}'"));
            status = 1;
            continue;
        }
        let r = if moving {
            match fs::rename(&sres, &tres) {
                Ok(()) => Ok(()),
                Err(e) if e.raw_os_error() == Some(18) => copy_tree(&sres, &tres).and_then(|_| {
                    if smd.is_dir() { fs::remove_dir_all(&sres) } else { fs::remove_file(&sres) }
                }),
                Err(e) => Err(e),
            }
        } else {
            copy_tree(&sres, &tres)
        };
        match r {
            Ok(()) => {
                if verbose {
                    ctx.puts(&format!("'{s}' -> '{tshown}'\n"));
                }
            }
            Err(e) => {
                let what = if moving { "move" } else { "create regular file" };
                if moving {
                    ctx.msg(&format!("cannot {what} '{s}' to '{tshown}': {}", crate::shell::errstr(&e)));
                } else {
                    ctx.msg(&format!("cannot {what} '{tshown}': {}", crate::shell::errstr(&e)));
                }
                status = 1;
            }
        }
    }
    status
}

// ---- ls -------------------------------------------------------------------

fn name_lookup(file: &str, id: u32) -> Option<String> {
    let s = fs::read_to_string(file).ok()?;
    for l in s.lines() {
        let f: Vec<&str> = l.split(':').collect();
        if f.len() > 2 && f[2].parse::<u32>().ok() == Some(id) {
            return Some(f[0].to_string());
        }
    }
    None
}

fn mode_string(md: &fs::Metadata) -> String {
    let m = md.permissions().mode();
    let t = if md.file_type().is_symlink() {
        'l'
    } else if md.is_dir() {
        'd'
    } else {
        '-'
    };
    let mut s = String::new();
    s.push(t);
    for (i, c) in "rwxrwxrwx".chars().enumerate() {
        s.push(if m & (1 << (8 - i)) != 0 { c } else { '-' });
    }
    s
}

struct Entry {
    name: String,
    path: PathBuf,
}

fn long_lines(entries: &[Entry], with_total: bool) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut rows: Vec<(String, String, String, String, String, String, String)> = Vec::new();
    let mut blocks = 0u64;
    for e in entries {
        let Ok(md) = fs::symlink_metadata(&e.path) else { continue };
        blocks += md.blocks();
        let own = name_lookup("/etc/passwd", md.uid()).unwrap_or_else(|| md.uid().to_string());
        let grp = name_lookup("/etc/group", md.gid()).unwrap_or_else(|| md.gid().to_string());
        let mt = md.mtime();
        let date = if mt > now - 182 * 86400 - 43200 && mt <= now + 3600 {
            timefmt::format(mt, "%b %e %H:%M")
        } else {
            timefmt::format(mt, "%b %e  %Y")
        };
        let mut name = e.name.clone();
        if md.file_type().is_symlink() {
            if let Ok(t) = fs::read_link(&e.path) {
                name = format!("{name} -> {}", t.display());
            }
        }
        rows.push((mode_string(&md), md.nlink().to_string(), own, grp, md.size().to_string(), date, name));
    }
    let w = |f: &dyn Fn(&(String, String, String, String, String, String, String)) -> usize| rows.iter().map(f).max().unwrap_or(0);
    let (wl, wo, wg, ws) = (w(&|r| r.1.len()), w(&|r| r.2.len()), w(&|r| r.3.len()), w(&|r| r.4.len()));
    let mut out = String::new();
    if with_total {
        out.push_str(&format!("total {}\n", blocks.div_ceil(2)));
    }
    for r in &rows {
        out.push_str(&format!("{} {:>wl$} {:<wo$} {:<wg$} {:>ws$} {} {}\n", r.0, r.1, r.2, r.3, r.4, r.5, r.6));
    }
    out
}

pub fn ls(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "1aAld",
        long: &[("all", 'a'), ("almost-all", 'A'), ("directory", 'd')],
        numeric: false,
        supported: "-1 -a -A -l -d",
    };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    let (all, almost, long, dir_self) = (p.has('a'), p.has('A'), p.has('l'), p.has('d'));
    let ops = if p.operands.is_empty() { vec![".".to_string()] } else { p.operands.clone() };
    let mut status = 0;
    let mut files: Vec<Entry> = Vec::new();
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    for o in &ops {
        let Some(r) = ctx.rpath(o) else {
            status = 2;
            continue;
        };
        let md = match fs::metadata(&r) {
            Ok(m) => m,
            Err(e) => {
                ctx.msg(&format!("cannot access '{o}': {}", crate::shell::errstr(&e)));
                status = 2;
                continue;
            }
        };
        if md.is_dir() && !dir_self {
            dirs.push((o.clone(), r));
        } else {
            files.push(Entry { name: o.clone(), path: grants::resolve_nofollow_last(&ctx.sh.cwd, Path::new(o)) });
        }
    }
    files.sort_by(|x, y| x.name.cmp(&y.name));
    let mut first = true;
    if !files.is_empty() {
        let s = if long { long_lines(&files, false) } else { files.iter().map(|e| format!("{}\n", e.name)).collect() };
        ctx.puts(&s);
        first = false;
    }
    let header = ops.len() > 1;
    for (name, r) in dirs {
        let mut names: Vec<String> = match fs::read_dir(&r) {
            Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
            Err(e) => {
                ctx.msg(&format!("cannot open directory '{name}': {}", crate::shell::errstr(&e)));
                status = 2;
                continue;
            }
        };
        if !all && !almost {
            names.retain(|n| !n.starts_with('.'));
        }
        if all {
            names.push(".".into());
            names.push("..".into());
        }
        names.sort();
        if header {
            let h = format!("{}{name}:\n", if first { "" } else { "\n" });
            ctx.puts(&h);
        }
        first = false;
        let entries: Vec<Entry> = names.iter().map(|n| Entry { name: n.clone(), path: r.join(n) }).collect();
        let s = if long { long_lines(&entries, true) } else { entries.iter().map(|e| format!("{}\n", e.name)).collect() };
        ctx.puts(&s);
    }
    status
}
