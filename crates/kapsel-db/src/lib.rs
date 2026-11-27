//! Salsa database for kapsel (follows spar's `spar-base-db` and rivet).
//!
//! Every command kapsel sees is a [`SourceFile`] input; a [`Session`] input
//! holds the history of an interactive session or the lines of a script with
//! their exit statuses. The pipeline is a chain of tracked functions, so
//! editing one command recomputes only that command's queries:
//!
//! ```text
//! SourceFile ─▶ parse ─▶ check_stage ─┐
//!                  │                   ├─▶ check ─▶ compile   (in-memory module cache)
//!                  └──▶ shapes ─▶ script_findings
//!                          │
//! Session ─────────────────┴──▶ history ─▶ session_findings   (KS-S001..S005 across commands)
//! ```
//!
//! The compile memo is the in-memory module cache; the signed on-disk cache
//! arrives in v0.4.
//!
//! The parse, check and compile stages are reached through a [`Frontend`] of
//! plain function pointers, defaulting to `kapsel_syntax::parse`,
//! `kapsel_check::check` and `kapsel_compile::compile`. Tests and tools can
//! substitute their own.

use std::sync::Arc;

use kapsel_analysis::{CommandShape, HistoryEntry, SessionFinding};
use kapsel_ast::{ParseError, Script};
use kapsel_plan::{CheckOutcome, Finding, PLAN_SCHEMA, Plan, Refusal, Severity};

/// The stages the database calls. Function pointers keep the database free of
/// generic parameters and make the stages swappable in tests.
#[derive(Clone, Copy)]
pub struct Frontend {
    pub parse: fn(&str) -> Result<Script, Vec<ParseError>>,
    pub check: fn(&str, &Script) -> CheckOutcome,
    pub refuse_parse_errors: fn(&str, &[ParseError]) -> CheckOutcome,
    pub compile: fn(&Plan) -> Result<Vec<u8>, String>,
}

impl Default for Frontend {
    fn default() -> Self {
        Self {
            parse: kapsel_syntax::parse,
            check: kapsel_check::check,
            refuse_parse_errors: kapsel_check::refuse_parse_errors,
            compile: |plan| kapsel_compile::compile(plan).map_err(|e| e.to_string()),
        }
    }
}

impl std::fmt::Debug for Frontend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Frontend")
    }
}

#[salsa::db]
pub trait Db: salsa::Database {
    fn frontend(&self) -> &Frontend;
}

/// One command line, script or `-c` string.
#[salsa::input(debug)]
pub struct SourceFile {
    /// Name for diagnostics (`-c`, a script path, `history:3`). Not identity.
    #[returns(ref)]
    pub name: String,
    #[returns(ref)]
    pub text: String,
}

/// A session: the commands in order and the exit status of each one that ran.
#[salsa::input(debug)]
pub struct Session {
    #[returns(ref)]
    pub commands: Vec<SourceFile>,
    /// Exit status per command, `None` if refused or not yet run. Same
    /// length as `commands`.
    #[returns(ref)]
    pub statuses: Vec<Option<i32>>,
}

/// Result of parsing one source file.
///
/// TODO(ws/parser): add the lossless rowan green tree once
/// `kapsel_syntax::parse_green(text) -> Parse { green, errors }` lands, as
/// spar's `ParseResult` does, and lower to the AST from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOutcome {
    pub script: Option<Arc<Script>>,
    pub errors: Arc<[ParseError]>,
}

impl ParseOutcome {
    pub fn ok(&self) -> bool {
        self.script.is_some() && self.errors.is_empty()
    }
}

#[salsa::tracked]
pub fn parse(db: &dyn Db, file: SourceFile) -> ParseOutcome {
    match (db.frontend().parse)(file.text(db)) {
        Ok(script) => ParseOutcome { script: Some(Arc::new(script)), errors: Arc::from([]) },
        Err(errors) => ParseOutcome { script: None, errors: errors.into() },
    }
}

/// The check stage alone (`kapsel_check`), before the similar-command rules.
#[salsa::tracked]
pub fn check_stage(db: &dyn Db, file: SourceFile) -> CheckOutcome {
    let parsed = parse(db, file);
    let text = file.text(db);
    match &parsed.script {
        Some(script) if parsed.errors.is_empty() => (db.frontend().check)(text, script),
        _ => (db.frontend().refuse_parse_errors)(text, &parsed.errors),
    }
}

/// Command shapes of one file; empty when it does not parse.
#[salsa::tracked]
pub fn shapes(db: &dyn Db, file: SourceFile) -> Arc<Vec<CommandShape>> {
    let parsed = parse(db, file);
    Arc::new(match &parsed.script {
        Some(script) => kapsel_analysis::shapes(file.text(db), script),
        None => Vec::new(),
    })
}

/// KS-S002..S005 within one file.
#[salsa::tracked]
pub fn script_findings(db: &dyn Db, file: SourceFile) -> Arc<Vec<Finding>> {
    Arc::new(kapsel_analysis::analyze_shapes(file.text(db), &shapes(db, file)))
}

/// The full check: the check stage plus the similar-command rules of this
/// file. A refuse-level KS-S finding (KS-S005) turns a plan into a refusal;
/// warn-level ones join the plan's warnings.
#[salsa::tracked]
pub fn check(db: &dyn Db, file: SourceFile) -> CheckOutcome {
    merge(check_stage(db, file).clone(), script_findings(db, file))
}

/// Adds `extra` findings to a check outcome (see [`check`]).
pub fn merge(outcome: CheckOutcome, extra: &[Finding]) -> CheckOutcome {
    if extra.is_empty() {
        return outcome;
    }
    match outcome {
        CheckOutcome::Refused(mut r) => {
            r.findings.extend(extra.iter().cloned());
            CheckOutcome::Refused(r)
        }
        CheckOutcome::Plan(mut plan) => {
            if extra.iter().any(|f| f.severity == Severity::Refuse) {
                let mut findings: Vec<Finding> = extra.iter().filter(|f| f.severity == Severity::Refuse).cloned().collect();
                findings.extend(plan.warnings);
                findings.extend(extra.iter().filter(|f| f.severity != Severity::Refuse).cloned());
                CheckOutcome::Refused(Refusal { schema: PLAN_SCHEMA.into(), source_sha256: plan.source_sha256, findings })
            } else {
                plan.warnings.extend(extra.iter().cloned());
                CheckOutcome::Plan(plan)
            }
        }
    }
}

/// Compiles the file's plan. The memo is the in-memory module cache: the same
/// text is compiled once per database.
#[salsa::tracked]
pub fn compile(db: &dyn Db, file: SourceFile) -> Result<Arc<[u8]>, String> {
    match check(db, file).clone() {
        CheckOutcome::Plan(plan) => (db.frontend().compile)(&plan).map(Arc::from),
        CheckOutcome::Refused(r) => Err(format!(
            "refused by the check stage ({} finding{}); there is no plan to compile",
            r.findings.len(),
            if r.findings.len() == 1 { "" } else { "s" }
        )),
    }
}

/// The session's commands joined into one text, and each command's offset.
/// Session findings index into this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub text: String,
    pub offsets: Vec<u32>,
}

impl History {
    /// Index of the command holding byte `pos` of the history text.
    pub fn command_at(&self, pos: u32) -> usize {
        self.offsets.iter().rposition(|&o| o <= pos).unwrap_or(0)
    }
}

#[salsa::tracked]
pub fn history(db: &dyn Db, session: Session) -> Arc<History> {
    let texts: Vec<&str> = session.commands(db).iter().map(|f| f.text(db).as_str()).collect();
    let (text, offsets) = kapsel_analysis::history_text(&texts);
    Arc::new(History { text, offsets })
}

/// Cross-command analysis over the session (KS-S001, and KS-S002/S003/S005
/// across commands). Reuses each command's memoized shapes, so after one
/// command changes only that command is re-parsed and re-shaped.
#[salsa::tracked]
pub fn session_findings(db: &dyn Db, session: Session) -> Arc<Vec<SessionFinding>> {
    let files = session.commands(db);
    let statuses = session.statuses(db);
    let shaped: Vec<Arc<Vec<CommandShape>>> = files.iter().map(|f| shapes(db, *f).clone()).collect();
    let entries: Vec<HistoryEntry<'_>> = shaped
        .iter()
        .enumerate()
        .map(|(i, s)| HistoryEntry { shapes: s, status: statuses.get(i).copied().flatten() })
        .collect();
    let h = history(db, session);
    Arc::new(kapsel_analysis::analyze_session(&h.text, &h.offsets, &entries))
}

/// The default database.
#[salsa::db]
#[derive(Clone, Default)]
pub struct RootDatabase {
    storage: salsa::Storage<Self>,
    frontend: Frontend,
}

impl RootDatabase {
    /// A database with custom stages, and an optional salsa event callback.
    pub fn with_frontend(frontend: Frontend, events: Option<Box<dyn Fn(salsa::Event) + Send + Sync + 'static>>) -> Self {
        Self { storage: salsa::Storage::new(events), frontend }
    }
}

#[salsa::db]
impl salsa::Database for RootDatabase {}

#[salsa::db]
impl Db for RootDatabase {
    fn frontend(&self) -> &Frontend {
        &self.frontend
    }
}

#[cfg(test)]
mod tests;
