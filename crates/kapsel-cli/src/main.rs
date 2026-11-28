//! `kapsel`: check, compile and run shell commands as WASM components.
//!
//! Every input path goes parse -> check -> plan -> compile -> run
//! (REQ-KS-022). Exit codes are in `kapsel_plan::exit` (REQ-KS-014).
//!
//! The only way to a running component is `run_source`, which takes the plan
//! out of `front_end` (parse + check). There is no other call to
//! `Session::run_component` in this crate.

mod editor;

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use kapsel_db::{RootDatabase, Session as DbSession, SourceFile};
use kapsel_diag::{Options, Source};
use kapsel_host::{GrantSet, Session};
use kapsel_plan::{CheckOutcome, Finding, Plan, exit};
use kapsel_seal::{Cache, SigningKey, keys_dir};
use salsa::Setter;

const USAGE: &str = "\
usage: kapsel [GLOBAL FLAGS] -c COMMAND
       kapsel [GLOBAL FLAGS] FILE
       kapsel [GLOBAL FLAGS]                       (interactive)
       kapsel [GLOBAL FLAGS] check [--json] (-c COMMAND | FILE)
       kapsel [GLOBAL FLAGS] compile (-c COMMAND | FILE) -o OUT.wasm
       kapsel explain RULE-ID                      (e.g. KS-R001)

global flags:
  --rw DIR     grant read and write access to DIR (the current directory is always granted)
  --ro DIR     grant read access to DIR
  --key FILE   signing key (default: a key under $KAPSEL_HOME/keys, created on first use)
  --grants-from-plan  (testing, REQ-KS-013) run FILE or -c with exactly the paths the
               plan grants, not the current directory; refused if the grants are dynamic
  --drop-grant PATH   (testing, with --grants-from-plan) leave PATH out of those grants
  -h, --help   this text
";

#[derive(PartialEq, Clone, Copy)]
enum Sub {
    Run,
    Check,
    Compile,
    Explain,
}

struct Opts {
    sub: Sub,
    rw: Vec<String>,
    ro: Vec<String>,
    key: Option<PathBuf>,
    grants_from_plan: bool,
    drop_grants: Vec<String>,
    json: bool,
    out: Option<PathBuf>,
    cmd: Option<String>,
    file: Option<String>,
}

fn usage_error(msg: &str) -> i32 {
    eprintln!("kapsel: {msg}");
    eprint!("{USAGE}");
    exit::USAGE
}

fn parse_args(args: &[String]) -> Result<Opts, i32> {
    let mut o = Opts {
        sub: Sub::Run,
        rw: vec![],
        ro: vec![],
        key: None,
        grants_from_plan: false,
        drop_grants: vec![],
        json: false,
        out: None,
        cmd: None,
        file: None,
    };
    let mut i = 0;
    let mut positional = 0;
    let value = |i: &mut usize, flag: &str| -> Result<String, i32> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| usage_error(&format!("{flag} needs a value")))
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Err(0);
            }
            "-c" => {
                if o.cmd.is_some() {
                    return Err(usage_error("-c given twice"));
                }
                o.cmd = Some(value(&mut i, "-c")?);
            }
            "--rw" => o.rw.push(value(&mut i, "--rw")?),
            "--ro" => o.ro.push(value(&mut i, "--ro")?),
            "--key" => o.key = Some(PathBuf::from(value(&mut i, "--key")?)),
            "--grants-from-plan" => o.grants_from_plan = true,
            "--drop-grant" => o.drop_grants.push(value(&mut i, "--drop-grant")?),
            "-o" => o.out = Some(PathBuf::from(value(&mut i, "-o")?)),
            "--json" => o.json = true,
            "check" if positional == 0 && o.sub == Sub::Run && o.file.is_none() => {
                o.sub = Sub::Check
            }
            "compile" if positional == 0 && o.sub == Sub::Run && o.file.is_none() => {
                o.sub = Sub::Compile
            }
            "explain" if positional == 0 && o.sub == Sub::Run && o.file.is_none() => {
                o.sub = Sub::Explain
            }
            s if s.starts_with('-') && s != "-" => {
                return Err(usage_error(&format!("unknown flag {s}")));
            }
            _ => {
                if o.file.is_some() {
                    return Err(usage_error(&format!("unexpected argument '{a}'")));
                }
                o.file = Some(a.to_string());
            }
        }
        if !a.starts_with('-') && a != "check" && a != "compile" && a != "explain" {
            positional += 1;
        }
        i += 1;
    }
    if o.cmd.is_some() && o.file.is_some() {
        return Err(usage_error("give either -c COMMAND or a FILE, not both"));
    }
    if o.sub == Sub::Explain {
        if o.cmd.is_some() || o.json || o.out.is_some() || o.file.is_none() {
            return Err(usage_error("`explain` takes exactly one RULE-ID"));
        }
        return Ok(o);
    }
    if o.json && o.sub != Sub::Check {
        return Err(usage_error("--json belongs to `check`"));
    }
    if o.grants_from_plan && (o.sub != Sub::Run || (o.cmd.is_none() && o.file.is_none())) {
        return Err(usage_error(
            "--grants-from-plan runs -c COMMAND or a FILE, not `check`, `compile` or the prompt",
        ));
    }
    if !o.drop_grants.is_empty() && !o.grants_from_plan {
        return Err(usage_error("--drop-grant needs --grants-from-plan"));
    }
    if o.out.is_some() && o.sub != Sub::Compile {
        return Err(usage_error("-o belongs to `compile`"));
    }
    if o.sub != Sub::Run && o.cmd.is_none() && o.file.is_none() {
        return Err(usage_error(
            "`check` and `compile` need -c COMMAND or a FILE",
        ));
    }
    if o.sub == Sub::Compile && o.out.is_none() {
        return Err(usage_error("`compile` needs -o OUT.wasm"));
    }
    Ok(o)
}

fn render_opts() -> Options {
    Options {
        color: std::io::stderr().is_terminal(),
    }
}

/// The findings an outcome carries: a refusal's findings or a plan's warnings.
fn outcome_findings(outcome: &CheckOutcome) -> &[Finding] {
    match outcome {
        CheckOutcome::Refused(r) => &r.findings,
        CheckOutcome::Plan(p) => &p.warnings,
    }
}

/// `check --json`: the CheckOutcome object exactly as before (plan schema
/// unchanged), plus one extra top-level `diagnostics` field holding the
/// kapsel-diag JSON (`kapsel.diag.v0`).
fn outcome_json(src: Source<'_>, outcome: &CheckOutcome) -> Result<String, String> {
    let mut v = serde_json::to_value(outcome).map_err(|e| e.to_string())?;
    let diag: serde_json::Value =
        serde_json::from_str(&kapsel_diag::render_json(src, outcome_findings(outcome)))
            .map_err(|e| e.to_string())?;
    match v.as_object_mut() {
        Some(o) => {
            o.insert("diagnostics".into(), diag);
        }
        None => return Err("outcome is not a JSON object".into()),
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// Prints an outcome (JSON on stdout, or rustc-style findings on stderr).
/// Ok(plan) only when nothing was refused.
fn report(name: &str, text: &str, outcome: CheckOutcome, json: bool) -> Result<Plan, i32> {
    let src = Source::new(name, text);
    if json {
        match outcome_json(src, &outcome) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("kapsel: internal: cannot serialize the outcome: {e}");
                return Err(exit::INTERNAL);
            }
        }
    } else {
        eprint!(
            "{}",
            kapsel_diag::render_outcome(src, &outcome, render_opts())
        );
    }
    match outcome {
        CheckOutcome::Plan(p) => Ok(p),
        CheckOutcome::Refused(_) => Err(exit::REFUSED),
    }
}

/// parse -> check through kapsel-db (so KS-S002..S005 apply within the
/// input). `name` is the script path or `-c`.
fn front_end(name: &str, source: &str, json: bool) -> Result<Plan, i32> {
    let db = RootDatabase::default();
    let file = SourceFile::new(&db, name.to_string(), source.to_string());
    let outcome = kapsel_db::check(&db, file).clone();
    report(name, source, outcome, json)
}

/// The long-lived analysis state of an interactive session.
struct Analysis {
    db: RootDatabase,
    session: DbSession,
    files: Vec<SourceFile>,
    statuses: Vec<Option<i32>>,
}

impl Analysis {
    fn new() -> Analysis {
        let db = RootDatabase::default();
        let session = DbSession::new(&db, vec![], vec![]);
        Analysis {
            db,
            session,
            files: vec![],
            statuses: vec![],
        }
    }

    fn sync(&mut self) {
        self.session
            .set_commands(&mut self.db)
            .to(self.files.clone());
        self.session
            .set_statuses(&mut self.db)
            .to(self.statuses.clone());
    }

    /// Checks `line` as the next command of the session: its own findings
    /// plus the cross-command ones (KS-S001 ...), all rendered against the
    /// history text. A refusal leaves the session unchanged.
    fn check_line(&mut self, line: &str) -> Result<Plan, i32> {
        let file = SourceFile::new(
            &self.db,
            format!("history:{}", self.files.len() + 1),
            line.to_string(),
        );
        self.files.push(file);
        self.statuses.push(None);
        self.sync();
        let last = self.files.len() - 1;
        let mut outcome = kapsel_db::check(&self.db, file).clone();
        let hist = kapsel_db::history(&self.db, self.session);
        let off = hist.offsets[last];
        // Move the line's own findings into the history text's offsets.
        let shift = |fs: &mut Vec<Finding>| {
            for f in fs {
                f.span.start += off;
                f.span.end += off;
                for r in &mut f.related {
                    r.span.start += off;
                    r.span.end += off;
                }
            }
        };
        match &mut outcome {
            CheckOutcome::Refused(r) => shift(&mut r.findings),
            CheckOutcome::Plan(p) => shift(&mut p.warnings),
        }
        let extra: Vec<Finding> = kapsel_db::session_findings(&self.db, self.session)
            .iter()
            .filter(|s| s.command == last)
            .map(|s| s.finding.clone())
            .collect();
        let outcome = kapsel_db::merge(outcome, &extra);
        let result = report("history", &hist.text, outcome, false);
        if result.is_err() {
            self.files.pop();
            self.statuses.pop();
            self.sync();
        }
        result
    }

    /// Records the exit status of the line that just ran.
    fn ran(&mut self, status: i32) {
        if let Some(s) = self.statuses.last_mut() {
            *s = Some(status);
            self.sync();
        }
    }
}

struct Ctx {
    key_file: Option<PathBuf>,
}

impl Ctx {
    fn key(&self) -> Result<SigningKey, i32> {
        let k = match &self.key_file {
            Some(f) => SigningKey::from_file(f),
            None => SigningKey::load_or_create(&keys_dir()),
        };
        k.map_err(|e| {
            eprintln!("kapsel: internal: signing key: {e}");
            exit::INTERNAL
        })
    }

    /// Compiles and seals a checked plan.
    fn compile(&self, plan: &Plan, source: &str) -> Result<Vec<u8>, i32> {
        // Keys and the cache live under KAPSEL_HOME or HOME. Without either,
        // fail closed instead of writing into the working directory.
        let set = |v: &str| std::env::var_os(v).is_some_and(|x| !x.is_empty());
        if !set("KAPSEL_HOME") && !set("HOME") {
            eprintln!("kapsel: no KAPSEL_HOME or HOME; set KAPSEL_HOME");
            return Err(exit::GRANTS);
        }
        let key = self.key()?;
        match kapsel_seal::compile_sealed(plan, source, &Cache::default_location(), &key) {
            Ok((bytes, _)) => Ok(bytes),
            Err(e) => {
                eprintln!("kapsel: internal: compile: {e}");
                Err(exit::INTERNAL)
            }
        }
    }
}

/// parse -> check -> compile -> run. Returns the exit status. The session is
/// made after the check, from the plan (`--grants-from-plan` needs it).
fn run_source(
    ctx: &Ctx,
    name: &str,
    source: &str,
    make_session: impl FnOnce(&Plan) -> Result<Session, i32>,
) -> i32 {
    let plan = match front_end(name, source, false) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let mut session = match make_session(&plan) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let bytes = match ctx.compile(&plan, source) {
        Ok(b) => b,
        Err(code) => return code,
    };
    match session.run_component(&bytes) {
        Ok(st) => (st & 0xff) as i32,
        Err(e) => {
            eprintln!("kapsel: internal: {e}");
            exit::INTERNAL
        }
    }
}

fn check_grant(flag: &str, dir: &str) -> Result<PathBuf, i32> {
    let p = PathBuf::from(dir);
    match p.canonicalize() {
        Ok(c) if c.is_dir() => Ok(c),
        Ok(_) => {
            eprintln!("kapsel: {flag} {dir}: not a directory");
            Err(exit::GRANTS)
        }
        Err(e) => {
            eprintln!("kapsel: {flag} {dir}: {e}");
            Err(exit::GRANTS)
        }
    }
}

/// The name shown after `-->`: the script path, or `-c`.
fn input_name(o: &Opts) -> String {
    match (&o.cmd, &o.file) {
        (Some(_), _) => "-c".to_string(),
        (None, Some(f)) => f.clone(),
        (None, None) => unreachable!("callers check"),
    }
}

fn explain_rule(id: &str) -> i32 {
    match kapsel_diag::explain(id) {
        Some(text) => {
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            0
        }
        None => {
            eprintln!("kapsel: unknown rule id '{id}'");
            eprintln!("known rule ids: {}", kapsel_diag::RULE_IDS.join(" "));
            exit::USAGE
        }
    }
}

fn read_source(o: &Opts) -> Result<String, i32> {
    match (&o.cmd, &o.file) {
        (Some(c), _) => Ok(c.clone()),
        (None, Some(f)) => std::fs::read_to_string(f).map_err(|e| {
            eprintln!("kapsel: {f}: {e}");
            exit::USAGE
        }),
        (None, None) => unreachable!("callers check"),
    }
}

fn interactive(session: &mut Session, ctx: &Ctx) -> i32 {
    let stdin = std::io::stdin();
    let mut last = 0;
    let mut analysis = Analysis::new();
    // A line editor only on a terminal; every other input keeps the plain reader below.
    let mut ed = editor::Editor::new();
    loop {
        let mut buf = String::new();
        let mut prompt = "kapsel$ ";
        loop {
            if let Some(e) = ed.as_mut() {
                e.set_cwd(session.cwd());
                match e.read() {
                    editor::Read::Line(l) => buf = l,
                    editor::Read::Interrupted => continue,
                    editor::Read::Eof => return last,
                    editor::Read::Unavailable => {
                        ed = None;
                        continue;
                    }
                }
                break;
            }
            eprint!("{prompt}");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => {
                    if buf.is_empty() {
                        return last;
                    }
                    break; // run what we have; the parser will say what is missing
                }
                Ok(_) => {}
            }
            buf.push_str(&line);
            if kapsel_syntax::is_incomplete(&buf) {
                prompt = "> ";
                continue;
            }
            break;
        }
        if buf.trim().is_empty() {
            continue;
        }
        let at_eof = !buf.ends_with('\n');
        // A refused line prints findings and leaves the session unchanged.
        // The trailing newline is not part of the command; leaving it out
        // keeps the history text one line per command.
        let line = buf.trim_end_matches(['\n', '\r']);
        match analysis.check_line(line) {
            Err(_) => {}
            Ok(plan) => match ctx.compile(&plan, line) {
                Err(_) => {}
                Ok(bytes) => match session.run_component(&bytes) {
                    Ok(st) => {
                        last = (st & 0xff) as i32;
                        analysis.ran(last);
                    }
                    Err(e) => eprintln!("kapsel: internal: {e}"),
                },
            },
        }
        if let Some(n) = session.exit_requested() {
            return n & 0xff;
        }
        if at_eof {
            return last;
        }
    }
}

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match parse_args(&args) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let ctx = Ctx {
        key_file: o.key.clone(),
    };

    match o.sub {
        Sub::Explain => return explain_rule(o.file.as_deref().unwrap_or("")),
        Sub::Check => {
            let source = match read_source(&o) {
                Ok(s) => s,
                Err(c) => return c,
            };
            return match front_end(&input_name(&o), &source, o.json) {
                Ok(_) => 0,
                Err(c) => c,
            };
        }
        Sub::Compile => {
            let source = match read_source(&o) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let plan = match front_end(&input_name(&o), &source, false) {
                Ok(p) => p,
                Err(c) => return c,
            };
            let bytes = match ctx.compile(&plan, &source) {
                Ok(b) => b,
                Err(c) => return c,
            };
            let out = o.out.as_ref().unwrap();
            return match std::fs::write(out, bytes) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("kapsel: {}: {e}", out.display());
                    exit::INTERNAL
                }
            };
        }
        Sub::Run => {}
    }

    // Grants: the current directory read-write, plus the flags. With
    // --grants-from-plan the current directory is not granted; the plan's
    // paths are added once the check has produced it.
    let mut grants = GrantSet::default();
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kapsel: current directory: {e}");
            return exit::GRANTS;
        }
    };
    if !o.grants_from_plan {
        grants.read_write.push(cwd.clone());
    }
    for d in &o.rw {
        match check_grant("--rw", d) {
            Ok(p) => grants.read_write.push(p),
            Err(c) => return c,
        }
    }
    for d in &o.ro {
        match check_grant("--ro", d) {
            Ok(p) => grants.read_only.push(p),
            Err(c) => return c,
        }
    }
    let new_session = |grants: GrantSet| {
        Session::new(grants).map_err(|e| {
            eprintln!("kapsel: {e}");
            exit::GRANTS
        })
    };

    if o.cmd.is_some() || o.file.is_some() {
        let source = match read_source(&o) {
            Ok(s) => s,
            Err(c) => return c,
        };
        return run_source(&ctx, &input_name(&o), &source, |plan| {
            if o.grants_from_plan {
                plan_grants(plan, &cwd, &o.drop_grants, &mut grants)?;
            }
            new_session(grants)
        });
    }
    match new_session(grants) {
        Ok(mut session) => interactive(&mut session, &ctx),
        Err(c) => c,
    }
}

/// `--grants-from-plan`: the plan's read and write paths, made absolute
/// against `cwd`, as exact-path grants, less every `--drop-grant` path.
/// Refused (exit 78) when the plan's grants are dynamic: then the plan does
/// not know every path, and there is nothing exact to grant.
fn plan_grants(
    plan: &Plan,
    cwd: &std::path::Path,
    drop: &[String],
    grants: &mut GrantSet,
) -> Result<(), i32> {
    if plan.grants.dynamic {
        eprintln!("kapsel: --grants-from-plan: the plan's grants are dynamic");
        return Err(exit::GRANTS);
    }
    for d in drop {
        let known = plan.grants.read.contains(d) || plan.grants.write.contains(d);
        if !known {
            eprintln!("kapsel: --drop-grant {d}: not a grant of the plan");
            return Err(exit::GRANTS);
        }
    }
    let abs = |p: &String| cwd.join(p);
    let keep = |p: &&String| !drop.contains(p);
    grants
        .paths_read
        .extend(plan.grants.read.iter().filter(keep).map(abs));
    grants
        .paths_write
        .extend(plan.grants.write.iter().filter(keep).map(abs));
    Ok(())
}

fn main() {
    let code = real_main();
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
