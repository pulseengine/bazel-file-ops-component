//! The check stage's output (REQ-KS-011, REQ-KS-013) and kapsel's exit codes
//! (REQ-KS-014).
//!
//! A [`Plan`] is the only thing the compiler accepts (REQ-KS-022): there is no
//! path from source text to a compiled module that does not go through
//! `kapsel_check::check`. The plan serializes as versioned JSON
//! (`schema: "kapsel.plan.v0"`) for `kapsel check --json` and
//! `zwinger explain`.

use serde::{Deserialize, Serialize};

pub use kapsel_ast::{AndOrOp, RedirectOp, Span};

pub const PLAN_SCHEMA: &str = "kapsel.plan.v0";

/// What `kapsel check` produces: a plan, or a refusal. Never both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum CheckOutcome {
    Plan(Plan),
    Refused(Refusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub schema: String,
    pub source_sha256: String,
    /// At least one finding has `severity: refuse`.
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub schema: String,
    /// Hex sha256 of the exact source text the plan was made from.
    pub source_sha256: String,
    pub body: Vec<PAndOr>,
    /// What the plan needs, as far as static analysis can tell.
    pub grants: Grants,
    /// Findings with `severity: warn`. Refuse findings never appear in a plan.
    pub warnings: Vec<Finding>,
}

/// A rule hit. Rule ids are stable, e.g. `KS-R001` (`|| true`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    /// The exact source text of `span`.
    pub excerpt: String,
    /// A concrete fix the user can apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Extra context lines (why the rule exists, what was measured).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Secondary spans with labels, e.g. the earlier command a duplicate repeats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub span: Span,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Refuse,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Grants {
    /// Paths read, as written in the source (literal paths only).
    pub read: Vec<String>,
    /// Paths written or created, as written in the source (literal only).
    pub write: Vec<String>,
    /// True when some path depends on a parameter or command substitution, so
    /// the lists above are incomplete and the host must decide at run time.
    pub dynamic: bool,
    /// Native programs needed. Always empty in v0.3 (native tools are refused).
    pub native: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PAndOr {
    pub first: PPipeline,
    pub rest: Vec<(AndOrOp, PPipeline)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PPipeline {
    pub negated: bool,
    pub stages: Vec<PStage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PStage {
    Call(Call),
    For { var: String, items: Vec<PWord>, body: Vec<PAndOr> },
    While { until: bool, condition: Vec<PAndOr>, body: Vec<PAndOr> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// `None` for an assignment-only command (`X=1`): the compiler emits
    /// `set-var` for each assignment and no call.
    pub tool: Option<Tool>,
    /// Includes the command name as `argv[0]`, which is always a literal.
    pub argv: Vec<PWord>,
    pub assignments: Vec<(String, PWord)>,
    pub redirects: Vec<PRedirect>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "name")]
pub enum Tool {
    /// A command implemented by kapsel (REQ-KS-025), including the special
    /// built-ins `cd`, `export`, `exit`, `true`, `false`, `:`, `test`, `[`.
    Builtin(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PRedirect {
    /// Resolved fd: the explicit one, or 0 for `<`/`<&`/heredoc, 1 otherwise.
    pub fd: u32,
    pub op: RedirectOp,
    pub target: PRedirectTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PRedirectTarget {
    Word { word: PWord },
    Fd { fd: u32 },
    /// Heredoc body; a quoted delimiter gives a single quoted literal.
    Heredoc { body: PWord },
}

/// A word after the check stage: quoting is resolved into per-part flags,
/// which is what expansion needs (field splitting and globbing apply only to
/// unquoted parts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PWord {
    pub parts: Vec<PPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PPart {
    Lit { text: String, quoted: bool },
    Var { name: String, quoted: bool },
    /// Glob pattern text in fnmatch syntax (`*`, `?`, `[...]`), always unquoted.
    Glob { pattern: String },
    /// `$(...)`: run `body` with stdout captured, then substitute the output
    /// with trailing newlines removed.
    Capture { body: Vec<PAndOr>, quoted: bool },
}

impl PWord {
    pub fn literal(text: impl Into<String>) -> Self {
        Self { parts: vec![PPart::Lit { text: text.into(), quoted: false }] }
    }

    /// The word's text if it has no expansions at all.
    pub fn as_literal(&self) -> Option<String> {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                PPart::Lit { text, .. } => out.push_str(text),
                _ => return None,
            }
        }
        Some(out)
    }
}

/// kapsel's own exit codes. A command's status (0..=255) passes through
/// unchanged; built-ins never exit with one of these values, so a kapsel
/// failure is distinguishable from a built-in failure (REQ-KS-014).
pub mod exit {
    /// Bad command line (EX_USAGE).
    pub const USAGE: i32 = 64;
    /// The check stage refused the input: a parse error or a refuse finding.
    pub const REFUSED: i32 = 65;
    /// Internal error: compile or instantiate failed (EX_SOFTWARE).
    pub const INTERNAL: i32 = 70;
    /// Invalid or missing grants (EX_CONFIG).
    pub const GRANTS: i32 = 78;
    /// Command not found. Not reachable in v0.3: an unknown command is refused.
    pub const NOT_FOUND: i32 = 127;
}

/// The built-in command names (REQ-KS-025 plus the special built-ins). The
/// check stage accepts exactly these; the host implements exactly these.
pub const BUILTINS: &[&str] = &[
    // special
    ":", "true", "false", "cd", "export", "exit", "test", "[", "pwd",
    // first batch, measured (REQ-KS-025)
    "cat", "echo", "printf", "head", "tail", "wc", "ls", "mkdir", "cp", "mv", "rm",
    "grep", "sed", "sort", "uniq", "cut", "tr", "tee", "date", "seq", "sleep",
];
