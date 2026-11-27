use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The kapsel binary: `KAPSEL_BIN`, or the workspace's debug build.
fn kapsel_bin() -> PathBuf {
    match std::env::var_os("KAPSEL_BIN") {
        Some(p) => PathBuf::from(p),
        None => workspace_root().join("target/debug/kapsel"),
    }
}

fn scratch(name: &str, side: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("kapsel-corpus-{}", std::process::id()))
        .join(name)
        .join(side);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Relative path -> contents for every file under `dir` (directories as "<dir>").
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                out.insert(rel, b"<dir>".to_vec());
                walk(base, &p, out);
            } else {
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Outcome {
    stdout: String,
    status: Option<i32>,
    files: BTreeMap<String, Vec<u8>>,
}

fn run(program: &Path, script: &Path, cwd: &Path) -> Outcome {
    let out = Command::new(program)
        .arg(script)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .output()
        .unwrap_or_else(|e| panic!("running {}: {e}", program.display()));
    Outcome {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        status: out.status.code(),
        files: tree(cwd),
    }
}

#[test]
fn corpus_agrees_with_dash() {
    let bin = kapsel_bin();
    assert!(
        bin.exists(),
        "{} not found: build it with `cargo build -p kapsel-cli` or set KAPSEL_BIN",
        bin.display()
    );
    let corpus = workspace_root().join("testdata/kapsel/corpus");
    let mut scripts: Vec<_> = fs::read_dir(&corpus)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "sh"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no scripts in {}", corpus.display());

    let mut mismatches = Vec::new();
    for script in &scripts {
        let name = script.file_stem().unwrap().to_string_lossy().into_owned();
        let dash = run(Path::new("/bin/dash"), script, &scratch(&name, "dash"));
        let kapsel = run(&bin, script, &scratch(&name, "kapsel"));
        let mut diffs = Vec::new();
        if dash.stdout != kapsel.stdout {
            diffs.push(format!("stdout\n  dash:   {:?}\n  kapsel: {:?}", dash.stdout, kapsel.stdout));
        }
        if dash.status != kapsel.status {
            diffs.push(format!("status dash {:?} kapsel {:?}", dash.status, kapsel.status));
        }
        if dash.files != kapsel.files {
            let d: Vec<_> = dash.files.keys().collect();
            let k: Vec<_> = kapsel.files.keys().collect();
            diffs.push(format!("files\n  dash:   {d:?}\n  kapsel: {k:?} (or contents differ)"));
        }
        if !diffs.is_empty() {
            mismatches.push(format!("{name}: {}", diffs.join("; ")));
        }
    }
    let _ = fs::remove_dir_all(std::env::temp_dir().join(format!("kapsel-corpus-{}", std::process::id())));

    println!("{} scripts, {} mismatches", scripts.len(), mismatches.len());
    for m in &mismatches {
        println!("MISMATCH {m}");
    }
    if cfg!(target_os = "linux") {
        assert!(mismatches.is_empty(), "{} of {} scripts disagree with dash", mismatches.len(), scripts.len());
    }
}
