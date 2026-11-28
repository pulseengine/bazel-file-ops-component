//! KS-R001 against the real binary (BL-1, second review of v0.3.0).
//!
//! The check stage says a right-hand side of `||` can fail by giving a plan.
//! This file holds it to that: the second review's bypasses must be refused
//! through the binary, and a generated search over a small grammar of
//! right-hand sides runs every one that gets a plan in three scratch
//! environments. A right-hand side that got a plan and exits 0 in all three
//! is a bypass, unless it uses a command on the counted allowlist of commands
//! that are 0 in practice but can fail (those are KS-R011's or nobody's).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kapsel_plan::CheckOutcome;

fn kapsel() -> PathBuf {
    assert_cmd::cargo::cargo_bin("kapsel")
}

/// The second review's list: each exited 0 after `cat missing.txt ||`.
const REVIEW2_BYPASSES: &[&str] = &[
    "for i in; do false; done",
    "X=1",
    "X=1 Y=2",
    "export X=1",
    "export X",
    "exit -256",
    "exit +0",
    "exit -0",
    "exit \" 0\"",
    "test x -o y",
    "test x -a y",
    "test '(' x ')'",
    "test a '<' b",
    "test \"x$(false)\"",
    "test x\"$(false)\"",
    "for i in a; do X=1; exit; done",
    "for i in a; do export Y=1; done",
];

#[test]
fn the_second_review_bypasses_are_refused_through_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    for rhs in REVIEW2_BYPASSES {
        let o = Command::new(kapsel())
            .current_dir(dir.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("KAPSEL_HOME", dir.path().join("home"))
            .args(["-c", &format!("cat missing.txt || {rhs}")])
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(65), "{rhs:?}: {err}");
        assert!(err.contains("KS-R001"), "{rhs:?}: {err}");
    }
}

// ---- the generated search ------------------------------------------------

/// Commands that are 0 in practice but can fail. A generated right-hand side
/// using one of them may get a plan and still exit 0 everywhere.
const ALLOWLIST: &[&str] = &[
    "echo",                               // KS-R011 warns on `|| echo`; fails on a closed stdout
    "printf",                             // KS-R011 warns on `|| printf`; fails on a closed stdout
    "pwd",                                // fails when the current directory is gone
    "cd .",                               // fails when the current directory is gone
    "seq 0",                              // fails on a closed stdout
    "export",                             // bare `export` lists variables; fails on a closed stdout
    "redirect-only",                      // `> out.txt` fails when the file cannot be created
    "file test on the current directory", // `test -d ""` fails when it is gone
    R013_TAG, // `! [ $V -eq 256 1 ]`: the word count of `$V` is unknown; KS-R013 warns
];

const R013_TAG: &str = "unquoted expansion in a test (KS-R013)";

/// xorshift64*, fixed seed: the same shapes on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[self.below(v.len())]
    }
    fn chance(&mut self, pct: usize) -> bool {
        self.below(100) < pct
    }
}

/// Words for `test`, `[`, `exit` and `true`: literals, operators, quoting
/// variants and expansions with and without literal text.
const WORDS: &[&str] = &[
    "x",
    "1",
    "0",
    "''",
    "\"\"",
    "a",
    "d",
    "nofile",
    "-n",
    "-z",
    "-e",
    "-f",
    "-d",
    "!",
    "'('",
    "')'",
    "=",
    "!=",
    "-o",
    "-a",
    "-eq",
    "-gt",
    "'<'",
    "'>'",
    "256",
    "-256",
    "+0",
    "-0",
    "\" 0\"",
    "+65",
    "'x'",
    "\"x\"",
    "\"$V\"",
    "$V",
    "\"x$V\"",
    "x\"$V\"",
    "\"x$(false)\"",
    "x\"$(true)\"",
    "\"$(echo 0)\"",
    "$(false)",
    "\"$(false)x\"",
    "\"-$V\"",
    "\"!$V\"",
];

const EXPORT_OPS: &[&str] = &[
    "X",
    "X=1",
    "Y=a",
    "1bad",
    "X=$(false)",
    "X=\"$V\"",
    "X=$(true)",
    "_Z=",
    "X=\"x$(false)\"",
];

const ASSIGNS: &[&str] = &[
    "X=1",
    "Y=",
    "X=$V",
    "X=\"$(true)\"",
    "X=$(false)",
    "Y=\"x$(false)\"",
    "Z='a b'",
];

/// Commands that can fail for real (their status depends on the environment).
const FAILABLE: &[&str] = &[
    "cat a",
    "cat nofile",
    "ls nofile",
    "ls d",
    "cd d",
    "grep x a",
    "test -e a",
    "[ -f f ]",
    "head -n 1 a",
];

/// Loop conditions whose outcome the check stage decides, or that exit; the
/// body of such a loop never runs forever.
const WHILE_COND: &[&str] = &[
    "false",
    "! :",
    "test ''",
    "test x = y",
    "[ -z x ]",
    "exit 0",
    "exit 1",
    "exit -256",
];
const UNTIL_COND: &[&str] = &[
    "true",
    ":",
    "X=1",
    "test x",
    "[ x -o '' ]",
    "export X",
    "exit 0",
    "exit 3",
];

const FOR_ITEMS: &[&str] = &["a", "b", "$V", "\"$V\"", "$(false)", "\"\""];

struct Gen {
    rng: Rng,
    tags: BTreeSet<&'static str>,
}

impl Gen {
    fn words(&mut self, max: usize) -> String {
        let n = self.rng.below(max + 1);
        let w: Vec<&str> = (0..n).map(|_| self.rng.pick(WORDS)).collect();
        // `"$(echo 0)"` is non-empty only as far as `echo` succeeds.
        if w.iter().any(|w| w.contains("echo")) {
            self.tags.insert("echo");
        }
        // A file test on `""` tests the current directory.
        if w.windows(2)
            .any(|p| matches!(p[0], "-e" | "-d" | "-f") && matches!(p[1], "''" | "\"\"" | "\"$V\""))
        {
            self.tags.insert("file test on the current directory");
        }
        w.join(" ")
    }

    /// Operands of `test` / `[`. An unquoted `$V` among them splits into any
    /// number of words, so the check stage does not fold the test (KS-R013
    /// warns instead).
    fn test_words(&mut self) -> String {
        let w = self.words(4);
        if w.split(' ').any(|x| x == "$V") {
            self.tags.insert(R013_TAG);
        }
        w
    }

    fn join(head: &str, rest: String) -> String {
        if rest.is_empty() {
            head.to_string()
        } else {
            format!("{head} {rest}")
        }
    }

    fn simple(&mut self) -> String {
        match self.rng.below(100) {
            0..=7 => {
                let w = self.words(1);
                Self::join(self.rng.pick(&["true", ":"]), w)
            }
            8..=11 => "false".into(),
            12..=27 => {
                let w = self.words(1);
                Self::join("exit", w)
            }
            28..=45 => {
                let w = self.test_words();
                Self::join("test", w)
            }
            46..=59 => format!("[ {} ]", self.test_words()).replace("[  ]", "[ ]"),
            60..=69 => {
                let n = 1 + self.rng.below(2);
                let ops: Vec<&str> = (0..n).map(|_| self.rng.pick(EXPORT_OPS)).collect();
                format!("export {}", ops.join(" "))
            }
            70..=81 => {
                let n = 1 + self.rng.below(2);
                let a: Vec<&str> = (0..n).map(|_| self.rng.pick(ASSIGNS)).collect();
                a.join(" ")
            }
            82..=91 => self.rng.pick(FAILABLE).into(),
            _ => {
                let (tag, text) = *[
                    ("echo", "echo x"),
                    ("printf", "printf x"),
                    ("pwd", "pwd"),
                    ("cd .", "cd ."),
                    ("seq 0", "seq 0"),
                    ("export", "export"),
                    ("redirect-only", "> out.txt"),
                ]
                .get(self.rng.below(7))
                .unwrap();
                self.tags.insert(tag);
                text.into()
            }
        }
    }

    fn command(&mut self, depth: usize) -> String {
        if depth == 0 || !self.rng.chance(30) {
            return self.simple();
        }
        match self.rng.below(3) {
            0 => {
                let n = self.rng.below(3);
                let items: Vec<&str> = (0..n).map(|_| self.rng.pick(FOR_ITEMS)).collect();
                let items = items.join(" ");
                let body = self.list(depth - 1);
                if items.is_empty() {
                    format!("for i in; do {body}; done")
                } else {
                    format!("for i in {items}; do {body}; done")
                }
            }
            1 => {
                let c = self.rng.pick(WHILE_COND);
                let body = self.list(depth - 1);
                format!("while {c}; do {body}; done")
            }
            _ => {
                let c = self.rng.pick(UNTIL_COND);
                let body = self.list(depth - 1);
                format!("until {c}; do {body}; done")
            }
        }
    }

    fn pipeline(&mut self, depth: usize) -> String {
        let neg = self.rng.chance(15);
        let mut p = self.command(depth);
        if self.rng.chance(15) {
            p = format!("{p} | {}", self.command(depth));
        }
        if neg { format!("! {p}") } else { p }
    }

    /// A loop body: one to three items separated by `;`.
    fn list(&mut self, depth: usize) -> String {
        let n = 1 + self.rng.below(3);
        (0..n)
            .map(|_| self.and_or(depth))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn and_or(&mut self, depth: usize) -> String {
        let p = self.pipeline(depth);
        if self.rng.chance(15) {
            let op = self.rng.pick(&["&&", "||"]);
            format!("{p} {op} {}", self.pipeline(depth))
        } else {
            p
        }
    }

    /// One right-hand side: an and-or list (no top-level `;`, which would end
    /// the `||`).
    fn rhs(&mut self) -> (String, BTreeSet<&'static str>) {
        self.tags.clear();
        let s = self.and_or(2);
        (s, std::mem::take(&mut self.tags))
    }
}

/// The scratch environments: cwd contents and environment variables.
struct Env {
    files: &'static [(&'static str, &'static str)],
    dirs: &'static [&'static str],
    vars: &'static [(&'static str, &'static str)],
}

const ENVS: &[Env] = &[
    // Empty directory, no variables beyond PATH and KAPSEL_HOME.
    Env {
        files: &[],
        dirs: &[],
        vars: &[],
    },
    // Files (even `nofile`) and a directory; variables set to plain values.
    Env {
        files: &[("a", "a\nx\n"), ("f", ""), ("nofile", "")],
        dirs: &["d", "x"],
        vars: &[("V", "o"), ("X", "x"), ("HOME", "/")],
    },
    // No `a`, a regular file `d` instead of a directory; a variable that
    // splits into two words and operator-like values.
    Env {
        files: &[("d", "")],
        dirs: &["b"],
        vars: &[("V", "x y"), ("Y", "-o"), ("Z", "!")],
    },
];

/// Two more environments for a right-hand side that exits 0 in all of
/// `ENVS`, before it counts: values of `V` that are operators of `test`,
/// with and without the files.
const CONFIRM: &[Env] = &[
    Env {
        files: &[("f", "")],
        dirs: &[],
        vars: &[("V", "!")],
    },
    Env {
        files: &[("a", "a\n")],
        dirs: &["d"],
        vars: &[("V", "-a")],
    },
];

/// Runs the script in a fresh directory for `env`. None on a timeout.
fn run_in(script: &str, env: &Env, root: &Path, n: usize) -> Option<i32> {
    let cwd = root.join(format!("r{n}"));
    std::fs::create_dir_all(&cwd).unwrap();
    for (f, text) in env.files {
        std::fs::write(cwd.join(f), text).unwrap();
    }
    for d in env.dirs {
        std::fs::create_dir_all(cwd.join(d)).unwrap();
    }
    let mut cmd = Command::new(kapsel());
    cmd.current_dir(&cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("KAPSEL_HOME", root.join("home"))
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env.vars {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let start = Instant::now();
    let code = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st.code();
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let _ = std::fs::remove_dir_all(&cwd);
    code
}

const SHAPES: usize = 3000;

#[test]
fn generated_rhs_with_a_plan_can_fail_somewhere() {
    let start = Instant::now();
    let mut g = Gen {
        rng: Rng(0x5eed_0fca_95e1),
        tags: BTreeSet::new(),
    };
    let mut shapes: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    let mut attempts = 0;
    while shapes.len() < SHAPES && attempts < SHAPES * 20 {
        attempts += 1;
        let (s, tags) = g.rhs();
        shapes.entry(s).or_insert(tags);
    }
    assert_eq!(shapes.len(), SHAPES, "the grammar yields enough shapes");

    // The check stage, in process: which right-hand sides get a plan.
    let mut planned = Vec::new();
    let mut refused = 0;
    for (rhs, tags) in &shapes {
        let src = format!("cat missing.txt || {rhs}\n");
        let plan = match kapsel_syntax::parse(&src) {
            Ok(script) => match kapsel_check::check(&src, &script) {
                CheckOutcome::Plan(p) => Some(p),
                CheckOutcome::Refused(_) => None,
            },
            Err(_) => None,
        };
        if let Some(plan) = plan {
            let mut tags = tags.clone();
            // The KS-R013 entry holds only where the plan carries the warning.
            if !plan.warnings.iter().any(|f| f.rule == "KS-R013") {
                tags.remove(R013_TAG);
            }
            planned.push((src, rhs.clone(), tags));
        } else {
            refused += 1;
        }
    }

    // The binary, in parallel: a planned right-hand side must exit non-zero
    // in at least one environment (stop at the first).
    let root = tempfile::tempdir().unwrap();
    let next = AtomicUsize::new(0);
    let runs = AtomicUsize::new(0);
    let confirmed = AtomicUsize::new(0);
    let timeouts = Mutex::new(Vec::new());
    let always_zero = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    std::thread::scope(|s| {
        for w in 0..workers {
            let (planned, next, runs, confirmed, timeouts, always_zero) =
                (&planned, &next, &runs, &confirmed, &timeouts, &always_zero);
            let home = root.path().join(format!("w{w}"));
            s.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((src, rhs, tags)) = planned.get(i) else {
                        break;
                    };
                    let mut zero_everywhere = true;
                    for (e, env) in ENVS.iter().chain(CONFIRM).enumerate() {
                        if e == ENVS.len() {
                            confirmed.fetch_add(1, Ordering::Relaxed);
                        }
                        let n = runs.fetch_add(1, Ordering::Relaxed);
                        match run_in(src, env, &home, n) {
                            Some(0) => {}
                            Some(_) => {
                                zero_everywhere = false;
                                break;
                            }
                            None => {
                                timeouts.lock().unwrap().push(format!("{rhs} (env {e})"));
                                zero_everywhere = false;
                                break;
                            }
                        }
                    }
                    if zero_everywhere {
                        always_zero
                            .lock()
                            .unwrap()
                            .push((rhs.clone(), tags.clone()));
                    }
                }
            });
        }
    });

    let always_zero = always_zero.into_inner().unwrap();
    let mut allowed: BTreeMap<&str, usize> = ALLOWLIST.iter().map(|a| (*a, 0)).collect();
    let mut bypasses = Vec::new();
    for (rhs, tags) in &always_zero {
        if tags.is_empty() {
            bypasses.push(rhs.clone());
        }
        for t in tags {
            *allowed.get_mut(t).expect("tag on the allowlist") += 1;
        }
    }
    let timeouts = timeouts.into_inner().unwrap();
    eprintln!(
        "r001 search: {} shapes ({attempts} drawn), {refused} refused, {} planned, {} runs, \
         {} reached the confirmation environments, \
         {} always 0 and allowlisted ({allowed:?}), {} timeouts, {:.1?}",
        shapes.len(),
        planned.len(),
        runs.load(Ordering::Relaxed),
        confirmed.load(Ordering::Relaxed),
        always_zero.len() - bypasses.len(),
        timeouts.len(),
        start.elapsed()
    );
    assert_eq!(
        ALLOWLIST.len(),
        9,
        "the allowlist is counted; extend it on purpose"
    );
    assert!(timeouts.is_empty(), "runs that timed out: {timeouts:#?}");
    assert!(
        bypasses.is_empty(),
        "{} right-hand sides got a plan and exit 0 in every environment:\n{}",
        bypasses.len(),
        bypasses.join("\n")
    );
}
