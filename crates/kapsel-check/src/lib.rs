//! The check stage (REQ-KS-011, REQ-KS-012, REQ-KS-013, DD-KS-003, DD-KS-004).
//!
//! Owner: check workstream. Turns a parsed script into a plan or a refusal and
//! runs nothing. Rule ids are stable (`KS-R001` = `|| true` / `|| :`).

use kapsel_ast as ast;
use kapsel_plan::{CheckOutcome, Finding};

/// Checks a parsed script. `source` is the exact text it was parsed from
/// (for `source_sha256` and finding excerpts).
pub fn check(source: &str, script: &ast::Script) -> CheckOutcome {
    let _ = (source, script);
    unimplemented!("check stage")
}

/// Turns parse errors into a refusal, so every input ends as a CheckOutcome.
pub fn refuse_parse_errors(source: &str, errors: &[ast::ParseError]) -> CheckOutcome {
    let _ = (source, errors);
    let _: Option<Finding> = None;
    unimplemented!("parse-error refusal")
}

pub use kapsel_plan::BUILTINS;
