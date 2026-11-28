//! Lowering: plan nodes to core wasm, one rule per node (see [`crate::RULES`]).
//!
//! Each `select_*` function is the selector for one node category: it returns
//! the rule that lowers the node, or declines with KC-001. The `emit_*`
//! routines are the rules' emission code.

use std::collections::{BTreeMap, BTreeSet};

use kapsel_plan::{
    AndOrOp, Call, PAndOr, PPart, PPipeline, PRedirect, PRedirectTarget, PStage, PWord, Plan,
    RedirectOp, Span,
};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, ImportSection, Instruction, MemArg, MemArgTarget, MemorySection,
    MemoryType, Module, TypeSection, ValType,
};

use crate::layout::*;
use crate::rule::*;
use crate::{CompileError, HOST_INTERFACE, Rule, code};

// Core function indices: the imports in order, then `run`.
pub(crate) const F_PIPE: u32 = 0;
pub(crate) const F_CALL: u32 = 1;
pub(crate) const F_SET_VAR: u32 = 2;
pub(crate) const F_SET_STATUS: u32 = 3;
pub(crate) const F_EXPAND_LIST: u32 = 4;
pub(crate) const F_BIND_ITEM: u32 = 5;
pub(crate) const F_SCOPE_PUSH: u32 = 6;
pub(crate) const F_SCOPE_POP: u32 = 7;
const F_RUN: u32 = 8;
pub(crate) const IMPORTS: [&str; 8] = [
    "pipe",
    "call",
    "set-var",
    "set-status",
    "expand-list",
    "bind-item",
    "scope-push",
    "scope-pop",
];

/// wasmparser's limit on locals per function.
const MAX_LOCALS: u32 = 50_000;

/// Target of a `2>&1`-style redirect: the fd lives in `target-fd`.
static EMPTY_WORD: PWord = PWord { parts: Vec::new() };

pub(crate) fn lower(plan: &Plan) -> Result<(Vec<u8>, Vec<&'static str>), CompileError> {
    let mut cx = Cx::default();
    cx.status = cx.new_local();
    cx.emit_list(&plan.body, Handle::Session, Handle::Session, None)?;
    cx.emit(Instruction::LocalGet(cx.status));
    cx.emit(Instruction::End);
    if cx.locals > MAX_LOCALS {
        return Err(CompileError::new(
            code::LIMIT,
            "plan",
            None,
            format!("needs {} locals, the limit is {MAX_LOCALS}", cx.locals),
        ));
    }
    if let Some(n) = cx.data.too_long {
        return Err(CompileError::new(
            code::LIMIT,
            "plan",
            None,
            format!("a list of {n} elements exceeds the limit of {MAX_LIST}"),
        ));
    }
    if cx.data.overflow {
        return Err(CompileError::new(
            code::LIMIT,
            "plan",
            None,
            "argument data does not fit a 32-bit memory",
        ));
    }
    let fired = cx.fired.iter().copied().collect();
    Ok((cx.finish(), fired))
}

fn decline(construct: &str, span: Option<Span>, message: impl Into<String>) -> CompileError {
    CompileError::new(code::UNSUPPORTED, construct, span, message)
}

// ---- selectors --------------------------------------------------------------

fn select_link(op: &AndOrOp) -> Rule {
    match op {
        AndOrOp::And => AND_LINK,
        AndOrOp::Or => OR_LINK,
    }
}

fn select_pipeline(p: &PPipeline) -> Result<Rule, CompileError> {
    match p.stages.len() {
        0 => Err(decline("pipeline", None, "pipeline with no stages")),
        1 => Ok(PIPE_SINGLE),
        _ => Ok(PIPE_MULTI),
    }
}

fn select_stage(s: &PStage) -> Result<Rule, CompileError> {
    match s {
        PStage::Call(c) => select_call(c),
        PStage::For { .. } => Ok(FOR_LOOP),
        PStage::While { condition, .. } if condition.is_empty() => Err(decline(
            "while",
            None,
            "`while`/`until` with an empty condition",
        )),
        PStage::While { until: false, .. } => Ok(WHILE_LOOP),
        PStage::While { until: true, .. } => Ok(UNTIL_LOOP),
    }
}

fn select_call(c: &Call) -> Result<Rule, CompileError> {
    let span = Some(c.span);
    match &c.tool {
        Some(_) => match c.argv.first() {
            None => Err(decline("call", span, "built-in call with an empty argv")),
            Some(w) if w.as_literal().is_none() => Err(decline(
                "call",
                span,
                "argv[0] is not a literal (dynamic command name)",
            )),
            Some(_) => Ok(CALL_BUILTIN),
        },
        None if !c.argv.is_empty() => Err(decline(
            "call",
            span,
            "command with no tool but with argv words",
        )),
        None if !c.redirects.is_empty() => Err(decline(
            "call",
            span,
            "command with no tool but with redirects (redirect-only command)",
        )),
        None if c.assignments.is_empty() => Err(decline(
            "call",
            span,
            "command with no tool and no assignments",
        )),
        None => Ok(ASSIGN_ONLY),
    }
}

fn select_redirect(r: &PRedirect, span: Option<Span>) -> Result<Rule, CompileError> {
    use RedirectOp::*;
    match (&r.op, &r.target) {
        (Read | Write | Append, PRedirectTarget::Word { .. }) => Ok(REDIR_PATH),
        (DupOut | DupIn, PRedirectTarget::Fd { .. }) => Ok(REDIR_DUP),
        (Heredoc, PRedirectTarget::Heredoc { .. }) => Ok(HEREDOC),
        (op, t) => Err(decline(
            "redirect",
            span,
            format!(
                "operator {op:?} with a {} target",
                match t {
                    PRedirectTarget::Word { .. } => "word",
                    PRedirectTarget::Fd { .. } => "fd",
                    PRedirectTarget::Heredoc { .. } => "heredoc",
                }
            ),
        )),
    }
}

fn select_part(p: &PPart) -> Rule {
    match p {
        PPart::Lit { .. } => PART_LIT,
        PPart::Var { .. } => PART_VAR,
        PPart::Glob { .. } => PART_GLOB,
        PPart::Capture { .. } => CAPTURE,
    }
}

// ---- emission ---------------------------------------------------------------

/// A pipe handle known at compile time (`0`, the session stream) or held in
/// a local.
#[derive(Clone, Copy)]
enum Handle {
    Session,
    Local(u32),
}

/// Compile-time data segment, addressed from [`DATA_BASE`].
#[derive(Default)]
struct Data {
    bytes: Vec<u8>,
    strings: BTreeMap<String, u32>,
    overflow: bool,
    /// The longest list over [`MAX_LIST`] elements, if any.
    too_long: Option<usize>,
}

impl Data {
    /// A list of `n` elements of `stride` bytes, 4-aligned. Lists longer than
    /// [`MAX_LIST`] are declined (KC-003); below it `stride * n` cannot wrap.
    fn alloc_list(&mut self, n: usize, stride: u32) -> u32 {
        if n > MAX_LIST as usize {
            self.too_long = Some(self.too_long.unwrap_or(0).max(n));
            self.overflow = true;
            return 0;
        }
        self.alloc(stride * n as u32, 4)
    }

    fn alloc(&mut self, size: u32, align: u32) -> u32 {
        let start = align_up(self.bytes.len() as u64, align as u64);
        let end = start + size as u64;
        if DATA_BASE as u64 + end > MAX_END {
            self.overflow = true;
            return 0;
        }
        self.bytes.resize(end as usize, 0);
        DATA_BASE + start as u32
    }

    fn put_u32(&mut self, addr: u32, v: u32) {
        if self.overflow {
            return;
        }
        let o = (addr - DATA_BASE) as usize;
        self.bytes[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u8(&mut self, addr: u32, v: u8) {
        if self.overflow {
            return;
        }
        self.bytes[(addr - DATA_BASE) as usize] = v;
    }

    /// Returns `(ptr, len)`; identical strings share one copy.
    fn string(&mut self, s: &str) -> (u32, u32) {
        if let Some(&p) = self.strings.get(s) {
            return (p, s.len() as u32);
        }
        let p = self.alloc(s.len() as u32, 1);
        if self.overflow {
            return (0, 0);
        }
        let o = (p - DATA_BASE) as usize;
        self.bytes[o..o + s.len()].copy_from_slice(s.as_bytes());
        self.strings.insert(s.to_owned(), p);
        (p, s.len() as u32)
    }
}

/// A capture whose output handle must be stored into `slot` before a call.
struct Capture<'a> {
    slot: u32,
    body: &'a [PAndOr],
}

#[derive(Default)]
struct Cx {
    data: Data,
    code: Vec<Instruction<'static>>,
    locals: u32,
    status: u32,
    fired: BTreeSet<&'static str>,
    /// Open control structures (block, loop, if) at the current point.
    depth: u32,
    /// `depth` just inside each open scope's block, innermost last.
    scopes: Vec<u32>,
}

impl Cx {
    fn fire(&mut self, r: Rule) {
        self.fired.insert(r.id);
    }

    fn emit(&mut self, i: Instruction<'static>) {
        match i {
            Instruction::Block(_) | Instruction::Loop(_) | Instruction::If(_) => self.depth += 1,
            Instruction::End => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        self.code.push(i);
    }

    fn new_local(&mut self) -> u32 {
        self.locals += 1;
        self.locals - 1
    }

    fn handle(&mut self, h: Handle) {
        match h {
            Handle::Session => self.emit(Instruction::I32Const(0)),
            Handle::Local(l) => self.emit(Instruction::LocalGet(l)),
        }
    }

    fn i32(&mut self, v: u32) {
        self.emit(Instruction::I32Const(v as i32));
    }

    // ---- data layout -------------------------------------------------------

    /// Lays out a word; `$(...)` parts are pushed onto `caps` in order.
    fn word<'a>(&mut self, w: &'a PWord, caps: &mut Vec<Capture<'a>>) -> (u32, u32) {
        let n = w.parts.len();
        let base = self.data.alloc_list(n, PART_SIZE);
        let n = n as u32;
        for (i, p) in w.parts.iter().enumerate() {
            let at = base + PART_SIZE * i as u32;
            let rule = select_part(p);
            self.fire(rule);
            match p {
                PPart::Lit { text, quoted } | PPart::Var { name: text, quoted } => {
                    let disc = if rule == PART_LIT { 0 } else { 1 };
                    let (sp, sl) = self.data.string(text);
                    self.data.put_u8(at + PART_DISC, disc);
                    self.data.put_u32(at + PART_STR_PTR, sp);
                    self.data.put_u32(at + PART_STR_LEN, sl);
                    self.data.put_u8(at + PART_QUOTED, *quoted as u8);
                }
                PPart::Glob { pattern } => {
                    let (sp, sl) = self.data.string(pattern);
                    self.data.put_u8(at + PART_DISC, 2);
                    self.data.put_u32(at + PART_STR_PTR, sp);
                    self.data.put_u32(at + PART_STR_LEN, sl);
                }
                PPart::Capture { body, quoted } => {
                    self.data.put_u8(at + PART_DISC, 3);
                    self.data.put_u8(at + PART_CAP_QUOTED, *quoted as u8);
                    caps.push(Capture {
                        slot: at + PART_CAP_PIPE,
                        body,
                    });
                }
            }
        }
        (base, n)
    }

    fn word_list<'a>(&mut self, ws: &'a [PWord], caps: &mut Vec<Capture<'a>>) -> (u32, u32) {
        let n = ws.len();
        let base = self.data.alloc_list(n, WORD_SIZE);
        let n = n as u32;
        for (i, w) in ws.iter().enumerate() {
            let (p, l) = self.word(w, caps);
            let at = base + WORD_SIZE * i as u32;
            self.data.put_u32(at + LIST_PTR, p);
            self.data.put_u32(at + LIST_LEN, l);
        }
        (base, n)
    }

    fn redirects<'a>(
        &mut self,
        rs: &'a [PRedirect],
        span: Option<Span>,
        caps: &mut Vec<Capture<'a>>,
    ) -> Result<(u32, u32), CompileError> {
        let n = rs.len();
        let base = self.data.alloc_list(n, REDIRECT_SIZE);
        let n = n as u32;
        for (i, r) in rs.iter().enumerate() {
            let rule = select_redirect(r, span)?;
            self.fire(rule);
            let at = base + REDIRECT_SIZE * i as u32;
            let ((wp, wl), target_fd) = match &r.target {
                PRedirectTarget::Word { word } => (self.word(word, caps), 0),
                PRedirectTarget::Heredoc { body } => (self.word(body, caps), 0),
                PRedirectTarget::Fd { fd } => (self.word(&EMPTY_WORD, caps), *fd),
            };
            self.data.put_u32(at + REDIR_FD, r.fd);
            self.data.put_u8(at + REDIR_OP, redirect_op_index(&r.op));
            self.data.put_u32(at + REDIR_TARGET_PTR, wp);
            self.data.put_u32(at + REDIR_TARGET_LEN, wl);
            self.data.put_u32(at + REDIR_TARGET_FD, target_fd);
        }
        Ok((base, n))
    }

    fn assignments<'a>(
        &mut self,
        asg: &'a [(String, PWord)],
        caps: &mut Vec<Capture<'a>>,
    ) -> (u32, u32) {
        let n = asg.len();
        let base = self.data.alloc_list(n, ASSIGN_SIZE);
        let n = n as u32;
        for (i, (name, w)) in asg.iter().enumerate() {
            let at = base + ASSIGN_SIZE * i as u32;
            let (np, nl) = self.data.string(name);
            let (wp, wl) = self.word(w, caps);
            self.data.put_u32(at + ASSIGN_NAME_PTR, np);
            self.data.put_u32(at + ASSIGN_NAME_LEN, nl);
            self.data.put_u32(at + ASSIGN_WORD_PTR, wp);
            self.data.put_u32(at + ASSIGN_WORD_LEN, wl);
        }
        (base, n)
    }

    // ---- control flow ------------------------------------------------------

    /// KC-R01. `;`-separated and-or lists. Leaves the last status in `status`.
    fn emit_list(
        &mut self,
        list: &[PAndOr],
        stdin: Handle,
        stdout: Handle,
        span: Option<Span>,
    ) -> Result<(), CompileError> {
        self.fire(LIST_SEQ);
        for ao in list {
            self.emit_pipeline(&ao.first, stdin, stdout, span)?;
            for (op, p) in &ao.rest {
                // KC-R02 / KC-R03
                let rule = select_link(op);
                self.fire(rule);
                self.emit(Instruction::LocalGet(self.status));
                if rule == AND_LINK {
                    self.emit(Instruction::I32Eqz);
                }
                self.emit(Instruction::If(BlockType::Empty));
                self.emit_pipeline(p, stdin, stdout, span)?;
                self.emit(Instruction::End);
            }
        }
        Ok(())
    }

    /// KC-R04 / KC-R05, then KC-R06 when negated.
    fn emit_pipeline(
        &mut self,
        p: &PPipeline,
        stdin: Handle,
        stdout: Handle,
        span: Option<Span>,
    ) -> Result<(), CompileError> {
        let rule = select_pipeline(p)?;
        self.fire(rule);
        let n = p.stages.len();
        let mut pipes = Vec::with_capacity(n - 1);
        for _ in 1..n {
            let l = self.new_local();
            self.emit(Instruction::Call(F_PIPE));
            self.emit(Instruction::LocalSet(l));
            pipes.push(Handle::Local(l));
        }
        for (i, stage) in p.stages.iter().enumerate() {
            let sin = if i == 0 { stdin } else { pipes[i - 1] };
            let sout = if i + 1 == n { stdout } else { pipes[i] };
            if n >= 2 {
                // KC-R19: each stage of a multi-stage pipeline is a subshell.
                self.fire(SCOPE_STAGE);
                self.emit_scoped(|cx| cx.emit_stage(stage, sin, sout, span))?;
            } else {
                self.emit_stage(stage, sin, sout, span)?;
            }
        }
        if p.negated {
            self.fire(PIPE_NEGATE);
            self.emit(Instruction::LocalGet(self.status));
            self.emit(Instruction::I32Eqz);
            self.emit(Instruction::LocalSet(self.status));
        }
        self.emit(Instruction::LocalGet(self.status));
        self.emit(Instruction::Call(F_SET_STATUS));
        Ok(())
    }

    fn emit_stage(
        &mut self,
        s: &PStage,
        stdin: Handle,
        stdout: Handle,
        span: Option<Span>,
    ) -> Result<(), CompileError> {
        let rule = select_stage(s)?;
        self.fire(rule);
        match s {
            PStage::Call(c) if rule == CALL_BUILTIN => self.emit_call(c, stdin, stdout),
            PStage::Call(c) => self.emit_assign_only(c, stdin),
            PStage::For { var, items, body } => {
                self.emit_for(var, items, body, stdin, stdout, span)
            }
            PStage::While {
                until,
                condition,
                body,
            } => self.emit_while(*until, condition, body, stdin, stdout, span),
        }
    }

    /// KC-R12. Runs each capture body into a fresh pipe and stores the handle
    /// into the capture's slot. Leaves the last body's status in `status`.
    fn emit_captures(
        &mut self,
        caps: &[Capture<'_>],
        stdin: Handle,
        span: Option<Span>,
    ) -> Result<(), CompileError> {
        for c in caps {
            let pipe = self.new_local();
            self.emit(Instruction::Call(F_PIPE));
            self.emit(Instruction::LocalSet(pipe));
            self.emit_scoped(|cx| cx.emit_list(c.body, stdin, Handle::Local(pipe), span))?;
            self.i32(c.slot);
            self.emit(Instruction::LocalGet(pipe));
            self.emit(Instruction::I32Store(mem(0)));
        }
        Ok(())
    }

    /// KC-R08. `X=1 Y=$(...)`: status 0, or the last capture's status (POSIX).
    fn emit_assign_only(&mut self, c: &Call, stdin: Handle) -> Result<(), CompileError> {
        let mut caps = Vec::new();
        let mut sets = Vec::with_capacity(c.assignments.len());
        for (name, w) in &c.assignments {
            let (np, nl) = self.data.string(name);
            let (wp, wl) = self.word(w, &mut caps);
            sets.push((np, nl, wp, wl));
        }
        self.i32(0);
        self.emit(Instruction::LocalSet(self.status));
        self.emit_captures(&caps, stdin, Some(c.span))?;
        for (np, nl, wp, wl) in sets {
            self.i32(np);
            self.i32(nl);
            self.i32(wp);
            self.i32(wl);
            self.emit(Instruction::Call(F_SET_VAR));
        }
        Ok(())
    }

    /// KC-R07. Captures, the call, then the `exit` check: 256 + n ends the
    /// innermost scope with status n (KC-R20), or stops the plan with status
    /// n when no scope is open (KC-R21).
    fn emit_call(&mut self, c: &Call, stdin: Handle, stdout: Handle) -> Result<(), CompileError> {
        let span = Some(c.span);
        let mut caps = Vec::new();
        // POSIX expansion order: command words, redirections, assignments.
        let (ap, al) = self.word_list(&c.argv, &mut caps);
        let (rp, rl) = self.redirects(&c.redirects, span, &mut caps)?;
        let (sp, sl) = self.assignments(&c.assignments, &mut caps);
        self.emit_captures(&caps, stdin, span)?;
        self.i32(ap);
        self.i32(al);
        self.i32(sp);
        self.i32(sl);
        self.i32(rp);
        self.i32(rl);
        self.handle(stdin);
        self.handle(stdout);
        self.emit(Instruction::Call(F_CALL));
        self.emit(Instruction::LocalSet(self.status));
        self.emit(Instruction::LocalGet(self.status));
        self.i32(256);
        self.emit(Instruction::I32GeU);
        self.emit(Instruction::If(BlockType::Empty));
        self.emit(Instruction::LocalGet(self.status));
        self.i32(256);
        self.emit(Instruction::I32Sub);
        match self.scopes.last().copied() {
            Some(scope_depth) => {
                self.fire(EXIT_SCOPE);
                self.emit(Instruction::LocalSet(self.status));
                // Branch to the end of the scope's block; scope-pop follows it.
                self.emit(Instruction::Br(self.depth - scope_depth));
            }
            None => {
                self.fire(EXIT_PLAN);
                self.emit(Instruction::Return);
            }
        }
        self.emit(Instruction::End);
        Ok(())
    }

    /// Runs `body` in a subshell scope: `scope-push`, a block that an `exit`
    /// inside branches out of, `scope-pop`. The status after the pop is the
    /// body's status, or n after `exit n`.
    fn emit_scoped(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<(), CompileError>,
    ) -> Result<(), CompileError> {
        self.emit(Instruction::Call(F_SCOPE_PUSH));
        self.emit(Instruction::Block(BlockType::Empty));
        self.scopes.push(self.depth);
        body(self)?;
        self.scopes.pop();
        self.emit(Instruction::End);
        self.emit(Instruction::Call(F_SCOPE_POP));
        Ok(())
    }

    /// KC-R09. `expand-list`, then bind and run the body per item. Status is
    /// the last body's, 0 when the list is empty.
    fn emit_for(
        &mut self,
        var: &str,
        items: &[PWord],
        body: &[PAndOr],
        stdin: Handle,
        stdout: Handle,
        span: Option<Span>,
    ) -> Result<(), CompileError> {
        let mut caps = Vec::new();
        let (wp, wl) = self.word_list(items, &mut caps);
        self.emit_captures(&caps, stdin, span)?;
        let (vp, vl) = self.data.string(var);
        let list = self.new_local();
        let count = self.new_local();
        let idx = self.new_local();
        let last = self.new_local();
        self.i32(wp);
        self.i32(wl);
        self.i32(RET_AREA);
        self.emit(Instruction::Call(F_EXPAND_LIST));
        self.i32(RET_AREA);
        self.emit(Instruction::I32Load(mem(0)));
        self.emit(Instruction::LocalSet(list));
        self.i32(RET_AREA);
        self.emit(Instruction::I32Load(mem(4)));
        self.emit(Instruction::LocalSet(count));
        self.i32(0);
        self.emit(Instruction::LocalSet(idx));
        self.i32(0);
        self.emit(Instruction::LocalSet(last));
        self.emit(Instruction::Block(BlockType::Empty));
        self.emit(Instruction::Loop(BlockType::Empty));
        self.emit(Instruction::LocalGet(idx));
        self.emit(Instruction::LocalGet(count));
        self.emit(Instruction::I32GeU);
        self.emit(Instruction::BrIf(1));
        self.i32(vp);
        self.i32(vl);
        self.emit(Instruction::LocalGet(list));
        self.emit(Instruction::LocalGet(idx));
        self.emit(Instruction::Call(F_BIND_ITEM));
        self.emit_list(body, stdin, stdout, span)?;
        self.emit(Instruction::LocalGet(self.status));
        self.emit(Instruction::LocalSet(last));
        self.emit(Instruction::LocalGet(idx));
        self.i32(1);
        self.emit(Instruction::I32Add);
        self.emit(Instruction::LocalSet(idx));
        self.emit(Instruction::Br(0));
        self.emit(Instruction::End);
        self.emit(Instruction::End);
        self.emit(Instruction::LocalGet(last));
        self.emit(Instruction::LocalSet(self.status));
        Ok(())
    }

    /// KC-R10 / KC-R11. Status is the last body's, 0 when it never ran.
    fn emit_while(
        &mut self,
        until: bool,
        condition: &[PAndOr],
        body: &[PAndOr],
        stdin: Handle,
        stdout: Handle,
        span: Option<Span>,
    ) -> Result<(), CompileError> {
        let last = self.new_local();
        self.i32(0);
        self.emit(Instruction::LocalSet(last));
        self.emit(Instruction::Block(BlockType::Empty));
        self.emit(Instruction::Loop(BlockType::Empty));
        self.emit_list(condition, stdin, stdout, span)?;
        self.emit(Instruction::LocalGet(self.status));
        if until {
            // until: leave when the condition succeeds
            self.emit(Instruction::I32Eqz);
        }
        self.emit(Instruction::BrIf(1));
        self.emit_list(body, stdin, stdout, span)?;
        self.emit(Instruction::LocalGet(self.status));
        self.emit(Instruction::LocalSet(last));
        self.emit(Instruction::Br(0));
        self.emit(Instruction::End);
        self.emit(Instruction::End);
        self.emit(Instruction::LocalGet(last));
        self.emit(Instruction::LocalSet(self.status));
        Ok(())
    }

    // ---- module assembly ---------------------------------------------------

    fn finish(self) -> Vec<u8> {
        let i32s = |n: usize| vec![ValType::I32; n];
        let mut types = TypeSection::new();
        types.ty().function([], [ValType::I32]); // 0: pipe, run
        types.ty().function(i32s(8), [ValType::I32]); // 1: call
        types.ty().function(i32s(4), []); // 2: set-var, bind-item
        types.ty().function(i32s(1), []); // 3: set-status
        types.ty().function(i32s(3), []); // 4: expand-list (retptr)
        types.ty().function([], []); // 5: scope-push, scope-pop

        let mut imports = ImportSection::new();
        for (name, ty) in IMPORTS.iter().zip([0, 1, 2, 3, 4, 2, 5, 5]) {
            imports.import(HOST_INTERFACE, name, EntityType::Function(ty));
        }

        let mut funcs = FunctionSection::new();
        funcs.function(0);

        let total = DATA_BASE as u64 + self.data.bytes.len() as u64;
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: pages_for(total),
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });

        let mut exports = ExportSection::new();
        exports.export("memory", ExportKind::Memory, 0);
        exports.export("run", ExportKind::Func, F_RUN);

        let mut code = CodeSection::new();
        let mut f = Function::new([(self.locals, ValType::I32)]);
        for i in &self.code {
            f.instruction(i);
        }
        code.function(&f);

        let mut data = DataSection::new();
        data.active(
            0,
            &ConstExpr::i32_const(DATA_BASE as i32),
            self.data.bytes.iter().copied(),
        );

        let mut m = Module::new();
        m.section(&types);
        m.section(&imports);
        m.section(&funcs);
        m.section(&memory);
        m.section(&exports);
        m.section(&code);
        m.section(&data);
        m.finish()
    }
}

pub(crate) fn redirect_op_index(op: &RedirectOp) -> u8 {
    match op {
        RedirectOp::Read => 0,
        RedirectOp::Write => 1,
        RedirectOp::Append => 2,
        RedirectOp::DupOut => 3,
        RedirectOp::DupIn => 4,
        RedirectOp::Heredoc => 5,
    }
}

fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        target: MemArgTarget::Memory(0),
        ordering: None,
    }
}
