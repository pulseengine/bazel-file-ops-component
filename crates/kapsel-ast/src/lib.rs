//! Typed syntax tree for kapsel's POSIX subset (REQ-KS-010).
//!
//! This is the contract between the parser (`kapsel-syntax`) and the check
//! stage (`kapsel-check`). The parser keeps a lossless rowan tree internally
//! (DD-KS-002) and lowers it to these types. Every node carries the byte span
//! of its source, so a refusal can quote exactly what it refuses.
//!
//! The subset: pipes, redirection, `&&` and `||`, `;` and newlines, quoting,
//! parameters, globs, heredocs, `$(...)`, and `for`/`while`/`until` loops.
//! Anything else is a [`ParseError`] naming the construct; it never reaches
//! execution.

use serde::{Deserialize, Serialize};

/// Byte range `[start, end)` in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

/// A whole script or `-c` command: and-or lists separated by `;` or newlines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    pub items: Vec<AndOrList>,
    pub span: Span,
}

/// `p1 && p2 || p3`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndOrList {
    pub first: Pipeline,
    pub rest: Vec<(AndOrOp, Pipeline)>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AndOrOp {
    And,
    Or,
}

/// `[!] c1 | c2 | ...`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pipeline {
    pub negated: bool,
    pub commands: Vec<Command>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Command {
    Simple(SimpleCommand),
    For(ForLoop),
    /// `while` and `until`.
    While(WhileLoop),
}

/// `NAME=value ... word ... redirect ...`. `words` may be empty when the
/// command is only assignments (`X=1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleCommand {
    pub assignments: Vec<Assignment>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub name: String,
    pub value: Word,
    pub span: Span,
}

/// One shell word, as a sequence of parts that are concatenated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub parts: Vec<WordPart>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WordPart {
    /// Unquoted text, backslash escapes already removed.
    Literal { text: String },
    /// `'...'`.
    SingleQuoted { text: String },
    /// `"..."`; contains only `Literal`, `Param` and `CommandSubst`.
    DoubleQuoted { parts: Vec<WordPart> },
    /// `$name`, `${name}`, and the specials `$?`, `$#`, `$0`..`$9`, `$@`, `$*`.
    /// `${name:-x}` and other operators are refused by the parser for now.
    Param { name: String },
    /// `$(...)`. Backquotes are refused.
    CommandSubst { script: Box<Script> },
    /// Unquoted `*`, `?` or `[...]`.
    Glob { glob: GlobPart },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum GlobPart {
    Star,
    Question,
    /// Bracket expression contents, without the brackets.
    Class { set: String },
}

/// `[fd]op target`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redirect {
    /// Explicit fd before the operator, if any (`2>`).
    pub fd: Option<u32>,
    pub op: RedirectOp,
    pub target: RedirectTarget,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectOp {
    /// `<`
    Read,
    /// `>`
    Write,
    /// `>>`
    Append,
    /// `>&`
    DupOut,
    /// `<&`
    DupIn,
    /// `<<` and `<<-`
    Heredoc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RedirectTarget {
    Word { word: Word },
    /// `2>&1`: the duplicated fd.
    Fd { fd: u32 },
    /// Heredoc body. `quoted` is true when the delimiter was quoted, in which
    /// case `body` is a single literal part with no expansion.
    Heredoc { body: Word, quoted: bool, strip_tabs: bool },
}

/// `for var in items; do body; done`. Without `in`, the parser refuses
/// (positional parameters are not in the subset yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForLoop {
    pub var: String,
    pub items: Vec<Word>,
    pub body: Vec<AndOrList>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhileLoop {
    /// `until` when true.
    pub until: bool,
    pub condition: Vec<AndOrList>,
    pub body: Vec<AndOrList>,
    pub span: Span,
}

/// A construct outside the subset, or malformed input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    /// Stable short name of the construct, e.g. `"subshell"`, `"backquote"`,
    /// `"function-definition"`, `"case"`, `"if"`, `"param-operator"`,
    /// `"background"`, or `"syntax"` for malformed input.
    pub construct: String,
    pub message: String,
    pub span: Span,
}
