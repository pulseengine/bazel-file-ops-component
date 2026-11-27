use std::cell::Cell;
use std::sync::{Arc, Mutex};

use kapsel_analysis::testkit;
use kapsel_plan::{CheckOutcome, Grants, PLAN_SCHEMA, Plan, Refusal, Severity};
use salsa::Setter;

use super::*;

thread_local! {
    static PARSES: Cell<usize> = const { Cell::new(0) };
    static COMPILES: Cell<usize> = const { Cell::new(0) };
}

/// Stages for tests: the testkit parser, a check stage that always plans, and
/// a compiler that emits the plan's source hash. Each test runs on its own
/// thread and salsa runs queries on the caller's thread, so the thread-local
/// counters are per test.
fn test_frontend() -> Frontend {
    Frontend {
        parse: |text| {
            PARSES.with(|c| c.set(c.get() + 1));
            Ok(testkit::parse(text))
        },
        check: |text, _| {
            CheckOutcome::Plan(Plan {
                schema: PLAN_SCHEMA.into(),
                source_sha256: format!("len{}", text.len()),
                body: Vec::new(),
                grants: Grants::default(),
                warnings: Vec::new(),
            })
        },
        refuse_parse_errors: |_, _| {
            CheckOutcome::Refused(Refusal { schema: PLAN_SCHEMA.into(), source_sha256: String::new(), findings: Vec::new() })
        },
        compile: |plan| {
            COMPILES.with(|c| c.set(c.get() + 1));
            Ok(plan.source_sha256.as_bytes().to_vec())
        },
    }
}

fn logged_db() -> (RootDatabase, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = log.clone();
    let db = RootDatabase::with_frontend(
        test_frontend(),
        Some(Box::new(move |e: salsa::Event| {
            if let salsa::EventKind::WillExecute { .. } = e.kind {
                sink.lock().unwrap().push(format!("{:?}", e.kind));
            }
        })),
    );
    (db, log)
}

fn executed(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    let mut v: Vec<String> = std::mem::take(&mut *log.lock().unwrap())
        .into_iter()
        .map(|s| s.trim_start_matches("WillExecute { database_key: ").trim_end_matches(" }").to_string())
        .collect();
    v.sort();
    v
}

fn run_all(db: &RootDatabase, session: Session) {
    for f in session.commands(db) {
        let _ = check(db, *f);
        let _ = compile(db, *f);
    }
    let _ = session_findings(db, session);
}

#[test]
fn editing_one_command_recomputes_only_that_command() {
    let (mut db, log) = logged_db();
    let a = SourceFile::new(&db, "history:1".into(), "mkdir -p out".into());
    let b = SourceFile::new(&db, "history:2".into(), "echo hi > out/a.txt".into());
    let c = SourceFile::new(&db, "history:3".into(), "cat out/a.txt".into());
    let session = Session::new(&db, vec![a, b, c], vec![Some(0), Some(0), None]);

    run_all(&db, session);
    let first = executed(&log);
    assert_eq!(PARSES.with(Cell::get), 3);
    assert_eq!(COMPILES.with(Cell::get), 3);
    assert_eq!(first.iter().filter(|s| s.starts_with("parse(")).count(), 3);

    // Same inputs: nothing executes.
    run_all(&db, session);
    assert_eq!(executed(&log), Vec::<String>::new());
    assert_eq!(PARSES.with(Cell::get), 3);

    // Edit the third command only.
    c.set_text(&mut db).to("cat out/a.text".into());
    run_all(&db, session);
    let second = executed(&log);
    println!("after editing command 3, executed: {second:#?}");
    assert_eq!(PARSES.with(Cell::get), 4, "only the edited command is re-parsed");
    assert_eq!(COMPILES.with(Cell::get), 4, "only the edited command is recompiled");
    let c_id = format!("{:?}", salsa::plumbing::AsId::as_id(&c));
    for q in ["parse", "check_stage", "shapes", "script_findings", "check", "compile"] {
        let hits: Vec<&String> = second.iter().filter(|s| s.starts_with(&format!("{q}("))).collect();
        assert_eq!(hits.len(), 1, "{q}: {second:#?}");
        assert!(hits[0].contains(&c_id), "{q} ran for another file: {hits:?}");
    }
    assert!(second.iter().any(|s| s.starts_with("history(")));
    assert!(second.iter().any(|s| s.starts_with("session_findings(")));
    assert_eq!(second.len(), 8, "{second:#?}");

    // The new text is a near miss of a path written by command 2.
    let f = session_findings(&db, session);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(f[0].finding.rule, "KS-S003");
    assert_eq!(f[0].command, 2);
}

#[test]
fn new_status_reruns_only_the_session_analysis() {
    let (mut db, log) = logged_db();
    let a = SourceFile::new(&db, "history:1".into(), "cat in.txt".into());
    let b = SourceFile::new(&db, "history:2".into(), "cat  'in.txt'".into());
    let session = Session::new(&db, vec![a, b], vec![None, None]);
    run_all(&db, session);
    assert!(session_findings(&db, session).is_empty());
    executed(&log);

    session.set_statuses(&mut db).to(vec![Some(1), None]);
    let f = session_findings(&db, session);
    let ran = executed(&log);
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(ran[0].starts_with("session_findings("), "{ran:?}");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].finding.rule, "KS-S001");
    assert_eq!(f[0].command, 1);
    let h = history(&db, session);
    assert_eq!(h.command_at(f[0].finding.span.start), 1);
}

#[test]
fn refuse_level_similar_command_finding_refuses_the_plan() {
    let db = RootDatabase::with_frontend(test_frontend(), None);
    let src = "mkdir -p results\necho a > results/a.txt\ncat results/a.txt\nrm -r reslts\n";
    let f = SourceFile::new(&db, "script.sh".into(), src.into());
    let CheckOutcome::Refused(r) = check(&db, f) else { panic!("expected a refusal") };
    assert_eq!(r.findings[0].rule, "KS-S005");
    assert_eq!(r.findings[0].severity, Severity::Refuse);
    assert!(compile(&db, f).as_ref().unwrap_err().contains("refused"));

    let ok = SourceFile::new(&db, "ok.sh".into(), "head -5 a\nhead -n 5 b\n".into());
    let CheckOutcome::Plan(p) = check(&db, ok) else { panic!("expected a plan") };
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(p.warnings[0].rule, "KS-S002");
    assert_eq!(&**compile(&db, ok).as_ref().unwrap(), b"len22");
}

/// The default frontend: kapsel_syntax, kapsel_check and kapsel_compile.
#[test]
fn default_frontend_runs_the_real_stages() {
    let db = RootDatabase::default();
    let ok = SourceFile::new(&db, "-c".into(), "mkdir -p out && echo hi > out/a.txt && cat out/a.txt".into());
    assert!(parse(&db, ok).ok());
    assert!(matches!(check(&db, ok), CheckOutcome::Plan(_)), "{:#?}", check(&db, ok));
    let bytes = compile(&db, ok).as_ref().expect("compiles").clone();
    assert!(bytes.starts_with(b"\0asm"));
    // The memo is the module cache: the same Arc comes back.
    assert!(Arc::ptr_eq(compile(&db, ok).as_ref().unwrap(), &bytes));

    let src = "mkdir -p results\necho a > results/a.txt\ncat results/a.txt\nrm -r reslts\n";
    let typo = SourceFile::new(&db, "s.sh".into(), src.into());
    let CheckOutcome::Refused(r) = check(&db, typo) else { panic!("expected a refusal") };
    assert!(r.findings.iter().any(|f| f.rule == "KS-S005"), "{r:#?}");

    let bad = SourceFile::new(&db, "-c".into(), "false || true".into());
    let CheckOutcome::Refused(r) = check(&db, bad) else { panic!("expected a refusal") };
    assert_eq!(r.findings[0].rule, "KS-R001");
}
