//! End to end for the REQ-KS-063 built-ins. dash has no `edit`, `json`,
//! `yaml`, `toml`, `field` or `lines`, so these scripts cannot be double
//! entries like `testdata/kapsel/corpus/`; each one carries its expected
//! result instead:
//!
//! - `testdata/kapsel/builtins2/NAME.sh`: the script, run by `kapsel NAME.sh`
//!   in an empty scratch directory;
//! - `NAME.stdout`: its exact stdout (status must be 0);
//! - `NAME.files/`: every file the script leaves behind, byte for byte.
//!
//! Regenerate with `KAPSEL_BLESS=1` and review the diff.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn kapsel_bin() -> PathBuf {
    match std::env::var_os("KAPSEL_BIN") {
        Some(p) => PathBuf::from(p),
        None => workspace_root().join("target/debug/kapsel"),
    }
}

/// Relative path -> contents for every file under `dir`.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out
}

fn write_tree(dir: &Path, files: &BTreeMap<String, Vec<u8>>) {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    for (rel, data) in files {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }
}

#[test]
fn builtins2_scripts_match_expected() {
    let bin = kapsel_bin();
    assert!(
        bin.exists(),
        "{} not found: build it with `cargo build -p kapsel-cli` or set KAPSEL_BIN",
        bin.display()
    );
    let dir = workspace_root().join("testdata/kapsel/builtins2");
    let mut scripts: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "sh"))
        .collect();
    scripts.sort();
    assert!(
        scripts.len() >= 6,
        "want at least 6 scripts in {}",
        dir.display()
    );
    let bless = std::env::var_os("KAPSEL_BLESS").is_some();
    let scratch_root =
        std::env::temp_dir().join(format!("kapsel-builtins2-{}", std::process::id()));
    let mut failures = Vec::new();
    for script in &scripts {
        let name = script.file_stem().unwrap().to_string_lossy().into_owned();
        let work = scratch_root.join(&name).join("work");
        let home = scratch_root.join(&name).join("home");
        let _ = fs::remove_dir_all(&work);
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&home).unwrap();
        let out = Command::new(&bin)
            .arg(script)
            .current_dir(&work)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("KAPSEL_HOME", &home)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let files = tree(&work);
        let want_out_path = dir.join(format!("{name}.stdout"));
        let want_files_dir = dir.join(format!("{name}.files"));
        if bless {
            fs::write(&want_out_path, &stdout).unwrap();
            write_tree(&want_files_dir, &files);
        }
        let want_out = fs::read_to_string(&want_out_path).unwrap_or_default();
        let want_files = tree(&want_files_dir);
        if out.status.code() != Some(0) {
            failures.push(format!(
                "{name}: status {:?}, stderr:\n{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        if stdout != want_out {
            failures.push(format!(
                "{name}: stdout\n--- want\n{want_out}--- got\n{stdout}"
            ));
        }
        if files != want_files {
            let names = |m: &BTreeMap<String, Vec<u8>>| m.keys().cloned().collect::<Vec<_>>();
            let differ: Vec<&String> = files
                .iter()
                .filter(|(k, v)| want_files.get(*k) != Some(v))
                .map(|(k, _)| k)
                .collect();
            failures.push(format!(
                "{name}: files\n  want {:?}\n  got  {:?}\n  differ {differ:?}",
                names(&want_files),
                names(&files)
            ));
        }
    }
    let _ = fs::remove_dir_all(&scratch_root);
    assert!(
        failures.is_empty(),
        "{} of {} scripts differ:\n{}",
        failures.len(),
        scripts.len(),
        failures.join("\n")
    );
}
