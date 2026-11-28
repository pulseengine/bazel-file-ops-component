//! REQ-KS-013, "omits a grant": a plan must list every path the command
//! touches. Every dash-corpus script and every acceptance command whose plan
//! has `grants.dynamic == false` runs twice in fresh scratch directories:
//! once as usual (the current directory read-write), once with
//! `--grants-from-plan`, which grants exactly the plan's read and write paths
//! and not the current directory. The two runs must agree on stdout, stderr,
//! exit status and the files left behind.
//!
//! The mutation half: the same run with one grant left out
//! (`--drop-grant PATH`) must differ, for at least one grant of every script
//! that has any. That is what a plan omitting a grant would look like, so the
//! first half could catch it.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

/// An input: a name, the script text, and files to create before it runs.
struct Input {
    name: String,
    script: String,
    files: Vec<(&'static str, &'static str)>,
}

/// The acceptance lines of docs/first-shell.md that produce a plan.
fn acceptance() -> Vec<Input> {
    let cmd = |name: &str, script: &str, files: Vec<(&'static str, &'static str)>| Input {
        name: format!("acceptance:{name}"),
        script: format!("{script}\n"),
        files,
    };
    vec![
        cmd("echo-wc", "echo hello | wc -c", vec![]),
        cmd(
            "mkdir-write-cat",
            "mkdir -p out && echo hi > out/a.txt && cat out/a.txt",
            vec![],
        ),
        cmd(
            "grep-missing",
            "grep -n foo missing.txt || echo none",
            vec![],
        ),
        cmd("variable", "X=world; echo \"hello $X\"", vec![]),
        cmd("cat-sort", "cat a | sort -u > b", vec![("a", "b\na\nb\n")]),
        cmd("script-file", "echo one\necho two", vec![]),
    ]
}

fn inputs() -> Vec<Input> {
    let mut v: Vec<Input> = common::corpus_scripts()
        .iter()
        .map(|p| Input {
            name: p.file_name().unwrap().to_string_lossy().into_owned(),
            script: std::fs::read_to_string(p).unwrap(),
            files: vec![],
        })
        .collect();
    v.extend(acceptance());
    v
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    files: BTreeMap<String, String>,
}

/// Relative path -> contents of every file under `dir` ("<dir>" for directories).
fn tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                out.insert(rel, "<dir>".into());
                walk(base, &p, out);
            } else {
                out.insert(
                    rel,
                    String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Rig {
    root: tempfile::TempDir,
    n: std::cell::Cell<usize>,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            root: tempfile::tempdir().unwrap(),
            n: std::cell::Cell::new(0),
        }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    /// The script, written outside every scratch directory.
    fn script_file(&self, input: &Input) -> PathBuf {
        let dir = self.root.path().join("scripts");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(input.name.replace(':', "-"));
        std::fs::write(&p, &input.script).unwrap();
        p
    }

    /// Runs kapsel with `args` in a fresh scratch directory holding the
    /// input's files. Paths of the scratch directory are replaced by `<cwd>`.
    fn run(&self, input: &Input, args: &[&str]) -> Outcome {
        self.n.set(self.n.get() + 1);
        let cwd = self.root.path().join(format!("run{}", self.n.get()));
        std::fs::create_dir_all(&cwd).unwrap();
        for (rel, text) in &input.files {
            std::fs::write(cwd.join(rel), text).unwrap();
        }
        let o = Command::cargo_bin("kapsel")
            .unwrap()
            .current_dir(&cwd)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("KAPSEL_HOME", self.home())
            .args(args)
            .output()
            .unwrap();
        let logical = cwd.to_string_lossy().into_owned();
        let physical = cwd.canonicalize().unwrap().to_string_lossy().into_owned();
        let norm = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .replace(&physical, "<cwd>")
                .replace(&logical, "<cwd>")
        };
        let files = tree(&cwd)
            .into_iter()
            .map(|(k, v)| (k, norm(v.as_bytes())))
            .collect();
        Outcome {
            status: o.status.code(),
            stdout: norm(&o.stdout),
            stderr: norm(&o.stderr),
            files,
        }
    }

    /// The plan's (read, write) grants, or None when the input is refused or
    /// its grants are dynamic.
    fn static_grants(&self, script: &Path) -> Option<(Vec<String>, Vec<String>)> {
        let o = Command::cargo_bin("kapsel")
            .unwrap()
            .current_dir(self.root.path())
            .env("KAPSEL_HOME", self.home())
            .args(["check", "--json"])
            .arg(script)
            .output()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        if v["outcome"] != "plan" || v["grants"]["dynamic"] != false {
            return None;
        }
        let list = |k: &str| -> Vec<String> {
            v["grants"][k]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect()
        };
        Some((list("read"), list("write")))
    }
}

/// The grants worth dropping one at a time: each distinct path that no other
/// granted path contains (`out/a.txt` is covered by `out`).
fn droppable(read: &[String], write: &[String]) -> Vec<String> {
    let mut all: Vec<String> = read.iter().chain(write).cloned().collect();
    all.sort();
    all.dedup();
    let covered = |p: &str| {
        all.iter()
            .any(|q| q != p && Path::new(p).starts_with(Path::new(q.as_str())))
    };
    all.iter().filter(|p| !covered(p)).cloned().collect()
}

/// Inputs whose grants are all needed but whose output does not show a
/// denied one: 014 prints only the line count of grep's error, which is 1
/// whether the error says "No such file" or "outside the session's grants".
const UNOBSERVABLE: &[&str] = &["014-stderr-dup.sh"];

#[test]
fn every_static_plan_runs_with_exactly_its_grants_and_fails_without_one() {
    let rig = Rig::new();
    let (mut eligible, mut skipped, mut drops, mut caught) = (0, 0, 0, 0);
    let mut wrong = Vec::new();
    for input in inputs() {
        let script = rig.script_file(&input);
        let Some((read, write)) = rig.static_grants(&script) else {
            skipped += 1;
            continue;
        };
        eligible += 1;
        let file = script.to_str().unwrap();
        let usual = rig.run(&input, &[file]);
        let exact = rig.run(&input, &["--grants-from-plan", file]);
        if exact != usual {
            wrong.push(format!(
                "{}: with exactly the plan's grants (read {read:?}, write {write:?}) the run \
                 differs:\n  usual: {usual:?}\n  exact: {exact:?}",
                input.name
            ));
            continue;
        }
        let candidates = droppable(&read, &write);
        let mut caught_here = Vec::new();
        for p in &candidates {
            drops += 1;
            let dropped = rig.run(&input, &["--grants-from-plan", "--drop-grant", p, file]);
            if dropped != usual {
                caught += 1;
                caught_here.push(p.clone());
            }
        }
        println!(
            "{}: read {read:?} write {write:?}; dropping one of {candidates:?} changes the run for {caught_here:?}",
            input.name
        );
        if !candidates.is_empty() && caught_here.is_empty() && !UNOBSERVABLE.contains(&&*input.name)
        {
            wrong.push(format!(
                "{}: no single dropped grant of {candidates:?} changes the run, so this input \
                 cannot show an omitted grant",
                input.name
            ));
        }
    }
    println!(
        "grants-from-plan: {eligible} inputs with static grants ({skipped} skipped: refused or \
         dynamic), {drops} single-grant drops, {caught} of them changed the run"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(eligible >= 25, "only {eligible} inputs have static grants");
    assert!(caught >= 10, "only {caught} dropped grants were noticed");
}

#[test]
fn grants_from_plan_flag_errors() {
    let rig = Rig::new();
    let input = Input {
        name: "x".into(),
        script: String::new(),
        files: vec![],
    };
    for (args, code, msg) in [
        (
            vec!["--grants-from-plan", "-c", "cat $F"],
            78,
            "the plan's grants are dynamic",
        ),
        (
            vec!["--grants-from-plan", "--drop-grant", "b", "-c", "cat a"],
            78,
            "--drop-grant b: not a grant of the plan",
        ),
        (
            vec!["--drop-grant", "a", "-c", "cat a"],
            64,
            "needs --grants-from-plan",
        ),
        (
            vec!["--grants-from-plan", "check", "-c", "cat a"],
            64,
            "--grants-from-plan runs",
        ),
    ] {
        let o = rig.run(&input, &args);
        assert_eq!(o.status, Some(code), "{args:?}: {}", o.stderr);
        assert!(o.stderr.contains(msg), "{args:?}: {}", o.stderr);
    }
}
