//! Similar-command analysis (the KS-S rules).
//!
//! Each simple command gets a normalized [`shape::CommandShape`]: tool, flags
//! in canonical form, operand classes, and the literal paths it reads, writes
//! or removes. The rules compare shapes within a script and across a session's
//! history and report [`kapsel_plan::Finding`]s with `related` spans pointing
//! at the other command:
//!
//! | Id | Severity | Finding |
//! |---|---|---|
//! | KS-S001 | warn | a failed command re-run unchanged, nothing in between changed its inputs |
//! | KS-S002 | warn | the same tool with diverging flags for the same purpose |
//! | KS-S003 | warn | a path read but never written, 1–2 edits from a path written earlier |
//! | KS-S004 | warn (note-level) | an identical line repeated with no state change in between |
//! | KS-S005 | refuse | `rm`/`mv` of a path 1–2 edits from a path the script uses heavily |
//!
//! Everything is a pure function of its input: shapes, distances and findings
//! are deterministic, which is what lets kapsel-db memoize them.

pub mod distance;
pub mod rules;
pub mod shape;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

pub use rules::{HistoryEntry, SessionFinding, analyze_session, analyze_shapes, history_text};
pub use shape::{CommandShape, shapes};

/// Every rule id this crate can put on a finding, in order (REQ-KS-016).
/// KS-S001 is session-only (`analyze_session`); the others also run on a script.
pub const RULES: &[&str] = &[
    rules::KS_S001,
    rules::KS_S002,
    rules::KS_S003,
    rules::KS_S004,
    rules::KS_S005,
];

use kapsel_ast::Script;
use kapsel_plan::Finding;

/// Shapes `script` and runs KS-S002..S005 over it.
pub fn analyze_script(source: &str, script: &Script) -> Vec<Finding> {
    analyze_shapes(source, &shapes(source, script))
}

#[cfg(test)]
mod tests;
