//! Plan to WASM component compiler (REQ-KS-020, DD-KS-005).
//!
//! Owner: compiler workstream. Emits a component of world `kapsel:shell/plan`
//! (`wit/kapsel-shell.wit`). The same plan always gives byte-identical output.
//!
//! The compiler follows three rules borrowed from pulseengine/synth:
//!
//! 1. **Honesty:** a plan construct without a lowering is declined with a
//!    machine-readable [`CompileError`] (`KC-001 unsupported-construct`, ...),
//!    never compiled into something silently different.
//! 2. **Selector as a rule table:** every plan node is lowered by a named rule
//!    from [`RULES`]; a node no rule selects is declined.
//! 3. **Translation validation on every compile:** [`validate`] decodes the
//!    finished component, re-derives its skeleton (host calls, control
//!    structure, pipe wiring, argument data) and checks it against a skeleton
//!    derived independently from the plan. A mismatch is `KC-900`.
//!
//! # Shape of the output
//!
//! The core module has no allocator and no `cabi_realloc`: every argument the
//! plan passes to the host (strings, words, redirect lists, assignment lists)
//! is laid out at compile time in one active data segment, in canonical ABI
//! layout. The whole plan compiles into the single exported function `run`;
//! a plan has no recursion, so captures and loop bodies are inlined.
//!
//! Memory map: bytes `0..8` unused, `8..16` the return area for
//! `expand-list`, the data segment from [`DATA_BASE`].
//!
//! A `$(...)` part becomes a `captured` part whose `pipe` field is a slot in
//! the data segment. Right before the call that uses the word, the module
//! creates a pipe, runs the capture body with stdout set to that pipe and
//! stores the handle into the slot with `i32.store`. Captures of one command
//! run in POSIX expansion order: command words, then redirections, then
//! assignments.
//!
//! Status: `set-status` follows every executed pipeline (after `!` is
//! applied), so `$?` is right inside an and-or list and after it.
//!
//! Subshells: every `$(...)` body and every stage of a pipeline with two or
//! more stages runs between `scope-push` and `scope-pop`, inside a wasm block.
//! `exit n` inside a scope sets the status to n and branches to the end of
//! the innermost scope's block, so the scope is popped and n becomes the
//! substitution's or stage's status. Only with no scope open does `exit n`
//! return n from `run`. Pipes are created outside the scopes.

use kapsel_plan::{PLAN_SCHEMA, Plan, Span};

pub mod layout;
mod lower;
mod validate;

pub use layout::DATA_BASE;
pub use validate::validate;

/// The WIT the compiled component targets, embedded so host and compiler use
/// the same text.
pub const SHELL_WIT: &str = include_str!("../../../wit/kapsel-shell.wit");

/// The interface every compiled plan imports, and the only one.
pub const HOST_INTERFACE: &str = "kapsel:shell/host@0.1.0";

/// Decline and failure codes. Each is `KC-nnn reason`.
pub mod code {
    /// A plan node that no rule in [`crate::RULES`] lowers.
    pub const UNSUPPORTED: &str = "KC-001 unsupported-construct";
    /// The plan's `schema` is not the one this compiler reads.
    pub const SCHEMA: &str = "KC-002 plan-schema";
    /// The plan exceeds a limit: locals, memory, or a list longer than
    /// [`crate::layout::MAX_LIST`] elements.
    pub const LIMIT: &str = "KC-003 limit-exceeded";
    /// The translation validator found the component does not match the plan.
    pub const VALIDATOR: &str = "KC-900 validator-mismatch";
    /// wit-parser or wit-component failed. Not reachable from a valid core
    /// module; kept so such a failure is reported, not hidden.
    pub const ENCODER: &str = "KC-901 encoder-failure";
}

/// Why a plan did not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// One of the constants in [`code`].
    pub code: &'static str,
    /// The plan construct concerned, e.g. `redirect`, `pipeline`, `call`.
    pub construct: String,
    /// The source span of the enclosing command, when the plan has one.
    pub span: Option<Span>,
    pub message: String,
}

impl CompileError {
    pub(crate) fn new(
        code: &'static str,
        construct: impl Into<String>,
        span: Option<Span>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            construct: construct.into(),
            span,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}: {}", self.code, self.construct, self.message)?;
        if let Some(s) = self.span {
            write!(f, " (bytes {}..{})", s.start, s.end)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

/// One lowering rule: a plan node shape and the routine that emits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub id: &'static str,
    pub name: &'static str,
    /// The plan node shape the rule selects.
    pub shape: &'static str,
}

macro_rules! rules {
    ($($konst:ident = $id:literal, $name:literal, $shape:literal;)*) => {
        $(pub const $konst: Rule = Rule { id: $id, name: $name, shape: $shape };)*
        /// Every lowering rule. A plan node no rule selects is declined (KC-001).
        pub const RULES: &[Rule] = &[$($konst),*];
    };
}

/// The rule table.
pub mod rule {
    use super::Rule;
    rules! {
        LIST_SEQ     = "KC-R01", "list-sequence",   "`a; b`: and-or lists in order";
        AND_LINK     = "KC-R02", "and-link",        "`a && b`: run b when status == 0";
        OR_LINK      = "KC-R03", "or-link",         "`a || b`: run b when status != 0";
        PIPE_SINGLE  = "KC-R04", "pipeline-single", "pipeline of one stage";
        PIPE_MULTI   = "KC-R05", "pipeline-multi",  "pipeline of N >= 2 stages: N-1 pipes";
        PIPE_NEGATE  = "KC-R06", "pipeline-negate", "`! p`: status = (status == 0)";
        CALL_BUILTIN = "KC-R07", "call-builtin",    "built-in call with a literal argv[0]";
        ASSIGN_ONLY  = "KC-R08", "assign-only",     "`X=v` with no command and no redirect";
        FOR_LOOP     = "KC-R09", "for-loop",        "`for v in words; do body; done`";
        WHILE_LOOP   = "KC-R10", "while-loop",      "`while cond; do body; done`";
        UNTIL_LOOP   = "KC-R11", "until-loop",      "`until cond; do body; done`";
        CAPTURE      = "KC-R12", "capture",         "`$(body)` part: pipe, body in a subshell scope, slot store";
        REDIR_PATH   = "KC-R13", "redirect-path",   "`<`, `>`, `>>` with a word target";
        REDIR_DUP    = "KC-R14", "redirect-dup",    "`>&n`, `<&n` with an fd target";
        HEREDOC      = "KC-R15", "heredoc",         "`<<` with a body word";
        PART_LIT     = "KC-R16", "part-lit",        "literal word part";
        PART_VAR     = "KC-R17", "part-var",        "parameter word part";
        PART_GLOB    = "KC-R18", "part-glob",       "glob word part";
        SCOPE_STAGE  = "KC-R19", "scope-stage",     "each stage of an N >= 2 pipeline in a subshell scope";
        EXIT_SCOPE   = "KC-R20", "exit-scope",      "`exit n` inside a scope: pop it, status n";
        EXIT_PLAN    = "KC-R21", "exit-plan",       "`exit n` with no scope open: run returns n";
    }
}
pub use rule::RULES;

/// What the compiler knowingly gets wrong compared with POSIX `sh`. Each entry
/// is listed in `KNOWN.md` with the test that pins it; the count is pinned too.
/// Empty since the WIT gained `scope-push`/`scope-pop` (KW-1..KW-4 are fixed
/// and are now behaviour tests).
pub const KNOWN_WRONG: &[(&str, &str)] = &[];

/// Plan shapes the compiler declines with KC-001. Listed in `KNOWN.md`; the
/// count is pinned by a test, and each has a decline test.
pub const DECLINED: &[(&str, &str)] = &[
    ("KD-1", "pipeline with no stages"),
    ("KD-2", "built-in call with an empty argv"),
    ("KD-3", "built-in call whose argv[0] is not a literal"),
    ("KD-4", "command with no tool but with argv words"),
    (
        "KD-5",
        "command with no tool and no assignments (e.g. a redirect-only `> f`)",
    ),
    ("KD-6", "command with no tool but with redirects"),
    (
        "KD-7",
        "redirect whose target kind does not match its operator",
    ),
    ("KD-8", "`while`/`until` with an empty condition"),
];

/// What one compilation produced, with the rules that fired.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub component: Vec<u8>,
    /// Rule ids that lowered at least one node, sorted.
    pub rules_fired: Vec<&'static str>,
}

/// Compiles a checked plan into component bytes. Runs the translation
/// validator before returning.
pub fn compile(plan: &Plan) -> Result<Vec<u8>, CompileError> {
    compile_report(plan).map(|c| c.component)
}

/// [`compile`], also reporting which rules fired.
pub fn compile_report(plan: &Plan) -> Result<Compiled, CompileError> {
    let (mut module, rules_fired) = lower_core(plan)?;
    let enc = |e: String| CompileError::new(code::ENCODER, "component", None, e);

    let mut resolve = wit_parser::Resolve::new();
    let pkg = resolve
        .push_str("kapsel-shell.wit", SHELL_WIT)
        .map_err(|e| enc(format!("parsing the shell WIT: {e:#}")))?;
    let world = resolve
        .select_world(&[pkg], Some("plan"))
        .map_err(|e| enc(format!("selecting world `plan`: {e:#}")))?;
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        false,
    )
    .map_err(|e| enc(format!("embedding component metadata: {e:#}")))?;

    let component = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .map_err(|e| enc(format!("reading the core module: {e:#}")))?
        .encode()
        .map_err(|e| enc(format!("encoding the component: {e:#}")))?;

    validate(plan, &component)?;
    Ok(Compiled {
        component,
        rules_fired,
    })
}

/// Compiles a checked plan into the core module only (no component metadata,
/// no validation). For `wasm-tools print` when debugging the generated code.
pub fn compile_core(plan: &Plan) -> Result<Vec<u8>, CompileError> {
    lower_core(plan).map(|(m, _)| m)
}

fn lower_core(plan: &Plan) -> Result<(Vec<u8>, Vec<&'static str>), CompileError> {
    if plan.schema != PLAN_SCHEMA {
        return Err(CompileError::new(
            code::SCHEMA,
            "plan",
            None,
            format!(
                "schema is `{}`, this compiler reads `{PLAN_SCHEMA}`",
                plan.schema
            ),
        ));
    }
    lower::lower(plan)
}
