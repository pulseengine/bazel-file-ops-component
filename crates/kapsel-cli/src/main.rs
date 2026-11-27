//! `kapsel`: check, compile and run shell commands as WASM components.
//!
//! Every input path goes parse -> check -> plan -> compile -> run
//! (REQ-KS-022). Exit codes are in `kapsel_plan::exit` (REQ-KS-014).
//!
//! The only way to a running component is `run_source`, which takes the plan
//! out of `front_end` (parse + check). There is no other call to
//! `Session::run_component` in this crate.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use kapsel_host::{GrantSet, Session};
use kapsel_plan::{CheckOutcome, Finding, Plan, Severity, exit};
use kapsel_seal::{Cache, SigningKey, keys_dir};

const USAGE: &str = "\
usage: kapsel [GLOBAL FLAGS] -c COMMAND
       kapsel [GLOBAL FLAGS] FILE
       kapsel [GLOBAL FLAGS]                       (interactive)
       kapsel [GLOBAL FLAGS] check [--json] (-c COMMAND | FILE)
       kapsel [GLOBAL FLAGS] compile (-c COMMAND | FILE) -o OUT.wasm

global flags:
  --rw DIR     grant read and write access to DIR (the current directory is always granted)
  --ro DIR     grant read access to DIR
  --key FILE   signing key (default: a key under $KAPSEL_HOME/keys, created on first use)
  -h, --help   this text
";

#[derive(PartialEq, Clone, Copy)]
enum Sub {
    Run,
    Check,
    Compile,
}

struct Opts {
    sub: Sub,
    rw: Vec<String>,
    ro: Vec<String>,
    key: Option<PathBuf>,
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
    let mut o = Opts { sub: Sub::Run, rw: vec![], ro: vec![], key: None, json: false, out: None, cmd: None, file: None };
    let mut i = 0;
    let mut positional = 0;
    let value = |i: &mut usize, flag: &str| -> Result<String, i32> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| usage_error(&format!("{flag} needs a value")))
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
            "-o" => o.out = Some(PathBuf::from(value(&mut i, "-o")?)),
            "--json" => o.json = true,
            "check" if positional == 0 && o.sub == Sub::Run && o.file.is_none() => o.sub = Sub::Check,
            "compile" if positional == 0 && o.sub == Sub::Run && o.file.is_none() => o.sub = Sub::Compile,
            s if s.starts_with('-') && s != "-" => return Err(usage_error(&format!("unknown flag {s}"))),
            _ => {
                if o.file.is_some() {
                    return Err(usage_error(&format!("unexpected argument '{a}'")));
                }
                o.file = Some(a.to_string());
            }
        }
        if !a.starts_with('-') && a != "check" && a != "compile" {
            positional += 1;
        }
        i += 1;
    }
    if o.cmd.is_some() && o.file.is_some() {
        return Err(usage_error("give either -c COMMAND or a FILE, not both"));
    }
    if o.json && o.sub != Sub::Check {
        return Err(usage_error("--json belongs to `check`"));
    }
    if o.out.is_some() && o.sub != Sub::Compile {
        return Err(usage_error("-o belongs to `compile`"));
    }
    if o.sub != Sub::Run && o.cmd.is_none() && o.file.is_none() {
        return Err(usage_error("`check` and `compile` need -c COMMAND or a FILE"));
    }
    if o.sub == Sub::Compile && o.out.is_none() {
        return Err(usage_error("`compile` needs -o OUT.wasm"));
    }
    Ok(o)
}

/// 1-based line and column (in characters) of a byte offset.
fn line_col(source: &str, offset: u32) -> (usize, usize) {
    let off = (offset as usize).min(source.len());
    let mut off = off;
    while off > 0 && !source.is_char_boundary(off) {
        off -= 1;
    }
    let before = &source[..off];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// Renders findings for the terminal. One function so the diagnostics crate
/// can replace it at integration.
fn render_findings(source: &str, findings: &[Finding]) -> String {
    let mut out = String::new();
    for f in findings {
        let tag = match f.severity {
            Severity::Refuse => "refused",
            Severity::Warn => "warning",
        };
        let (l, c) = line_col(source, f.span.start);
        out.push_str(&format!("kapsel: {tag}: {} {}\n", f.rule, f.message));
        let first = f.excerpt.lines().next().unwrap_or("");
        out.push_str(&format!("  {l}:{c}  {first}\n"));
        for r in &f.related {
            let (rl, rc) = line_col(source, r.span.start);
            out.push_str(&format!("  {rl}:{rc}  {}\n", r.label));
        }
        if let Some(h) = &f.help {
            out.push_str(&format!("  help: {h}\n"));
        }
        for n in &f.notes {
            out.push_str(&format!("  note: {n}\n"));
        }
    }
    out
}

/// parse -> check. Prints findings; Ok(plan) only when nothing was refused.
fn front_end(source: &str, json: bool) -> Result<Plan, i32> {
    let outcome = match kapsel_syntax::parse(source) {
        Err(errs) => kapsel_check::refuse_parse_errors(source, &errs),
        Ok(script) => kapsel_check::check(source, &script),
    };
    if json {
        match serde_json::to_string_pretty(&outcome) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("kapsel: internal: cannot serialize the outcome: {e}");
                return Err(exit::INTERNAL);
            }
        }
    }
    match outcome {
        CheckOutcome::Plan(p) => {
            if !p.warnings.is_empty() {
                eprint!("{}", render_findings(source, &p.warnings));
            }
            Ok(p)
        }
        CheckOutcome::Refused(r) => {
            if !json {
                eprint!("{}", render_findings(source, &r.findings));
            }
            Err(exit::REFUSED)
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

/// parse -> check -> compile -> run. Returns the exit status.
fn run_source(session: &mut Session, ctx: &Ctx, source: &str) -> i32 {
    let plan = match front_end(source, false) {
        Ok(p) => p,
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
    loop {
        let mut buf = String::new();
        let mut prompt = "kapsel$ ";
        loop {
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
        match front_end(&buf, false) {
            Err(_) => {}
            Ok(plan) => match ctx.compile(&plan, &buf) {
                Err(_) => {}
                Ok(bytes) => match session.run_component(&bytes) {
                    Ok(st) => last = (st & 0xff) as i32,
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
    let ctx = Ctx { key_file: o.key.clone() };

    match o.sub {
        Sub::Check => {
            let source = match read_source(&o) {
                Ok(s) => s,
                Err(c) => return c,
            };
            return match front_end(&source, o.json) {
                Ok(_) => 0,
                Err(c) => c,
            };
        }
        Sub::Compile => {
            let source = match read_source(&o) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let plan = match front_end(&source, false) {
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

    // Grants: the current directory read-write, plus the flags.
    let mut grants = GrantSet::default();
    match std::env::current_dir() {
        Ok(d) => grants.read_write.push(d),
        Err(e) => {
            eprintln!("kapsel: current directory: {e}");
            return exit::GRANTS;
        }
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
    let mut session = match Session::new(grants) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kapsel: {e}");
            return exit::GRANTS;
        }
    };

    if o.cmd.is_some() || o.file.is_some() {
        let source = match read_source(&o) {
            Ok(s) => s,
            Err(c) => return c,
        };
        return run_source(&mut session, &ctx, &source);
    }
    interactive(&mut session, &ctx)
}

fn main() {
    let code = real_main();
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
