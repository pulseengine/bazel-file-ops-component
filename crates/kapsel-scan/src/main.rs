//! `kapsel-scan [--root DIR]... [--repos-in DIR] [--rule NAME]... [--json] [--verbose]`
//!
//! Exit codes: 0 all selected rules pass, 1 a rule failed (for `run-blocks`
//! only a parser crash fails: refusals are advisory), 2 usage error.

use std::path::PathBuf;
use std::process::ExitCode;

use kapsel_scan::runs::{self, Counts};
use kapsel_scan::{DEFAULT_RULES, RULES, run_rule};

fn usage() -> ExitCode {
    eprintln!(
        "usage: kapsel-scan [--root DIR]... [--repos-in DIR] [--rule NAME]... [--json] [--verbose]\n\
         rules: {} (default: {})\n\
         --repos-in DIR  scan every subdirectory of DIR as a repository (run-blocks)",
        RULES.join(", "),
        DEFAULT_RULES.join(", ")
    );
    ExitCode::from(2)
}

fn name_of(p: &std::path::Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".")
        .to_string()
}

/// `run-blocks` over every root; returns the exit code.
fn run_blocks(
    roots: &[PathBuf],
    json: bool,
    verbose: bool,
    docs: &mut Vec<serde_json::Value>,
) -> i32 {
    let mut total = Counts::default();
    let mut all = runs::RunsReport::default();
    for root in roots {
        let rep = runs::run_blocks(root);
        let name = name_of(&std::fs::canonicalize(root).unwrap_or(root.clone()));
        total.add(&rep.counts());
        if json {
            docs.push(runs::render_json(&name, &rep));
        } else {
            print!("{}", runs::render_text(&name, &rep, verbose));
        }
        all.blocks.extend(rep.blocks);
        all.errors.extend(rep.errors);
    }
    if roots.len() > 1 {
        if json {
            let mut d = runs::render_json("TOTAL", &all);
            d.as_object_mut().map(|o| o.remove("blocks"));
            docs.push(d);
        } else {
            print!("{}", runs::render_text("TOTAL", &all, false));
        }
    }
    i32::from(total.crash > 0)
}

fn real_main() -> ExitCode {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut rules: Vec<String> = Vec::new();
    let mut json = false;
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => match args.next() {
                Some(v) => roots.push(PathBuf::from(v)),
                None => return usage(),
            },
            "--repos-in" => match args.next() {
                Some(v) => {
                    let Ok(rd) = std::fs::read_dir(&v) else {
                        eprintln!("kapsel-scan: cannot read {v}");
                        return ExitCode::from(2);
                    };
                    let mut d: Vec<PathBuf> = rd
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect();
                    d.sort();
                    roots.extend(d);
                }
                None => return usage(),
            },
            "--rule" => match args.next() {
                Some(v) => rules.push(v),
                None => return usage(),
            },
            "--json" => json = true,
            "--verbose" => verbose = true,
            _ => return usage(),
        }
    }
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    if rules.is_empty() {
        rules = DEFAULT_RULES.iter().map(|r| r.to_string()).collect();
    }
    let mut code = 0;
    let mut docs = Vec::new();
    for rule in &rules {
        if rule == "run-blocks" {
            code = code.max(run_blocks(&roots, json, verbose, &mut docs));
            continue;
        }
        let Some(rep) = run_rule(rule, &roots[0]) else {
            eprintln!("kapsel-scan: unknown rule `{rule}`");
            return usage();
        };
        let (out, err, c) = rep.render();
        code = code.max(c);
        if json {
            let sites: Vec<_> = rep
                .sites
                .iter()
                .map(|s| serde_json::json!({"file": s.file, "line": s.line, "ok": s.ok, "reason": s.reason}))
                .collect();
            docs.push(serde_json::json!({
                "rule": rep.rule, "passed": c == 0, "sites": sites, "errors": rep.errors,
            }));
        } else {
            let prefix = if rules.len() > 1 {
                format!("[{rule}] ")
            } else {
                String::new()
            };
            for l in out.lines() {
                println!("{prefix}{l}");
            }
            for l in err.lines() {
                eprintln!("{prefix}{l}");
            }
        }
    }
    if json {
        println!("{}", serde_json::Value::Array(docs));
    }
    ExitCode::from(code as u8)
}

fn main() -> ExitCode {
    // A deep script must not overflow the default main-thread stack; a stack
    // overflow aborts and cannot be caught.
    match std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(real_main)
    {
        Ok(h) => h.join().unwrap_or(ExitCode::from(1)),
        Err(_) => real_main(),
    }
}
