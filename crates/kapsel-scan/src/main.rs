//! `kapsel-scan [--root DIR] [--rule NAME]... [--json]`
//!
//! Exit codes: 0 all selected rules pass, 1 a rule failed, 2 usage error.

use std::path::PathBuf;
use std::process::ExitCode;

use kapsel_scan::{RULES, run_rule};

fn usage() -> ExitCode {
    eprintln!(
        "usage: kapsel-scan [--root DIR] [--rule NAME]... [--json]\nrules: {}",
        RULES.join(", ")
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut rules: Vec<String> = Vec::new();
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => match args.next() {
                Some(v) => root = PathBuf::from(v),
                None => return usage(),
            },
            "--rule" => match args.next() {
                Some(v) => rules.push(v),
                None => return usage(),
            },
            "--json" => json = true,
            _ => return usage(),
        }
    }
    if rules.is_empty() {
        rules = RULES.iter().map(|r| r.to_string()).collect();
    }
    let mut code = 0;
    let mut docs = Vec::new();
    for rule in &rules {
        let Some(rep) = run_rule(rule, &root) else {
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
