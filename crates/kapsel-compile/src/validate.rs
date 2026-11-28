//! Translation validator, run on every compile.
//!
//! Two skeletons are built and compared:
//!
//! - **derived:** decode the finished component with wasmparser, find the core
//!   module that exports `run`, and walk `run`'s operators with a small
//!   abstract interpreter. Host imports are identified by their import name,
//!   not by the compiler's index constants. Each host call becomes an event
//!   with its arguments read back from the data segment (argv, assignments,
//!   redirects rendered as text) and its stdin/stdout traced to the `pipe()`
//!   call that produced the handle. Control structures become events too.
//! - **expected:** walk the plan and write down the events its semantics
//!   require, without using the lowering code.
//!
//! Any difference is `KC-900 validator-mismatch`.

use std::collections::BTreeMap;

use kapsel_plan::{PAndOr, PPart, PPipeline, PRedirect, PRedirectTarget, PStage, PWord, Plan};
use wasmparser::{
    ComponentExternalKind, DataKind, ExternalKind, Operator, Parser, Payload, TypeRef, Validator,
    WasmFeatures,
};

use crate::layout::{ASSIGN_SIZE, PART_SIZE, REDIRECT_SIZE, WORD_SIZE};
use crate::lower::{IMPORTS, redirect_op_index};
use crate::{CompileError, HOST_INTERFACE, code};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Io {
    Session,
    Pipe(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ev {
    /// `pipe()`; pipes are numbered in code order.
    Pipe(u32),
    Call {
        argv: Vec<String>,
        assignments: Vec<(String, String)>,
        redirects: Vec<String>,
        stdin: Io,
        stdout: Io,
    },
    SetVar(String, String),
    SetStatus,
    ExpandList(Vec<String>),
    BindItem(String),
    /// A pipe handle stored into a captured-part slot.
    StoreCapture(u32),
    Block,
    Loop,
    If,
    Else,
    End,
    Return,
    ScopePush,
    ScopePop,
    /// A branch to the end of the innermost open scope's block.
    ExitScope,
}

fn mismatch(msg: impl Into<String>) -> CompileError {
    CompileError::new(code::VALIDATOR, "component", None, msg)
}

/// Checks a component against the plan it was compiled from. [`crate::compile`]
/// calls this on every compilation; it is public so tools and tests can check
/// a component they hold.
pub fn validate(plan: &Plan, component: &[u8]) -> Result<(), CompileError> {
    let derived = derive(component)?;
    let mut ex = Expected::default();
    ex.list(&plan.body, Io::Session, Io::Session);
    ex.evs.push(Ev::End);
    let expected = ex.evs;
    if derived != expected {
        let i = derived
            .iter()
            .zip(&expected)
            .take_while(|(a, b)| a == b)
            .count();
        return Err(mismatch(format!(
            "skeleton differs at event {i}: component has {:?}, plan needs {:?} \
             ({} events derived, {} expected)",
            derived.get(i),
            expected.get(i),
            derived.len(),
            expected.len()
        )));
    }
    Ok(())
}

// ---- rendering (shared by both sides; it is formatting only) -----------------

fn render_part(kind: u8, text: &str, quoted: bool, capture: Option<u32>) -> String {
    let body = match kind {
        0 => text.to_owned(),
        1 => format!("${{{text}}}"),
        2 => format!("<glob {text}>"),
        _ => match capture {
            Some(k) => format!("$(#{k})"),
            None => "$(unset)".to_owned(),
        },
    };
    if quoted { format!("\"{body}\"") } else { body }
}

fn render_redirect(fd: u32, op: u8, target: &str, target_fd: u32) -> String {
    format!("{fd} op{op} [{target}] &{target_fd}")
}

// ---- expected skeleton from the plan ----------------------------------------

#[derive(Default)]
struct Expected {
    evs: Vec<Ev>,
    pipes: u32,
    /// Subshell scopes open at this point of the plan.
    scopes: u32,
}

impl Expected {
    fn scoped(&mut self, body: impl FnOnce(&mut Self)) {
        self.evs.extend([Ev::ScopePush, Ev::Block]);
        self.scopes += 1;
        body(self);
        self.scopes -= 1;
        self.evs.extend([Ev::End, Ev::ScopePop]);
    }

    fn pipe(&mut self) -> u32 {
        let k = self.pipes;
        self.pipes += 1;
        self.evs.push(Ev::Pipe(k));
        k
    }

    fn list(&mut self, list: &[PAndOr], stdin: Io, stdout: Io) {
        for ao in list {
            self.pipeline(&ao.first, stdin, stdout);
            for (_, p) in &ao.rest {
                self.evs.push(Ev::If);
                self.pipeline(p, stdin, stdout);
                self.evs.push(Ev::End);
            }
        }
    }

    fn pipeline(&mut self, p: &PPipeline, stdin: Io, stdout: Io) {
        let n = p.stages.len();
        let pipes: Vec<Io> = (1..n).map(|_| Io::Pipe(self.pipe())).collect();
        for (i, s) in p.stages.iter().enumerate() {
            let sin = if i == 0 { stdin } else { pipes[i - 1] };
            let sout = if i + 1 == n { stdout } else { pipes[i] };
            if n >= 2 {
                self.scoped(|ex| ex.stage(s, sin, sout));
            } else {
                self.stage(s, sin, sout);
            }
        }
        self.evs.push(Ev::SetStatus);
    }

    /// Runs the captures of `words` in order; returns their pipe numbers.
    fn captures<'a>(&mut self, words: impl Iterator<Item = &'a PWord>, stdin: Io) -> Vec<u32> {
        let mut ids = Vec::new();
        for w in words {
            for p in &w.parts {
                if let PPart::Capture { body, .. } = p {
                    let k = self.pipe();
                    self.scoped(|ex| ex.list(body, stdin, Io::Pipe(k)));
                    self.evs.push(Ev::StoreCapture(k));
                    ids.push(k);
                }
            }
        }
        ids
    }

    fn stage(&mut self, s: &PStage, stdin: Io, stdout: Io) {
        match s {
            PStage::Call(c) if c.tool.is_some() => {
                let words = c
                    .argv
                    .iter()
                    .chain(c.redirects.iter().filter_map(redirect_word))
                    .chain(c.assignments.iter().map(|(_, w)| w));
                let ids = self.captures(words, stdin);
                let mut ids = ids.into_iter();
                let argv = c.argv.iter().map(|w| word(w, &mut ids)).collect();
                let redirects = c.redirects.iter().map(|r| redirect(r, &mut ids)).collect();
                let assignments = c
                    .assignments
                    .iter()
                    .map(|(n, w)| (n.clone(), word(w, &mut ids)))
                    .collect();
                self.evs.push(Ev::Call {
                    argv,
                    assignments,
                    redirects,
                    stdin,
                    stdout,
                });
                let exit = if self.scopes > 0 {
                    Ev::ExitScope
                } else {
                    Ev::Return
                };
                self.evs.extend([Ev::If, exit, Ev::End]);
            }
            PStage::Call(c) => {
                let ids = self.captures(c.assignments.iter().map(|(_, w)| w), stdin);
                let mut ids = ids.into_iter();
                for (n, w) in &c.assignments {
                    let v = word(w, &mut ids);
                    self.evs.push(Ev::SetVar(n.clone(), v));
                }
            }
            PStage::For { var, items, body } => {
                let ids = self.captures(items.iter(), stdin);
                let mut ids = ids.into_iter();
                let items = items.iter().map(|w| word(w, &mut ids)).collect();
                self.evs
                    .extend([Ev::ExpandList(items), Ev::Block, Ev::Loop]);
                self.evs.push(Ev::BindItem(var.clone()));
                self.list(body, stdin, stdout);
                self.evs.extend([Ev::End, Ev::End]);
            }
            PStage::While {
                condition, body, ..
            } => {
                self.evs.extend([Ev::Block, Ev::Loop]);
                self.list(condition, stdin, stdout);
                self.list(body, stdin, stdout);
                self.evs.extend([Ev::End, Ev::End]);
            }
        }
    }
}

fn redirect_word(r: &PRedirect) -> Option<&PWord> {
    match &r.target {
        PRedirectTarget::Word { word } | PRedirectTarget::Heredoc { body: word } => Some(word),
        PRedirectTarget::Fd { .. } => None,
    }
}

fn word(w: &PWord, ids: &mut impl Iterator<Item = u32>) -> String {
    w.parts
        .iter()
        .map(|p| match p {
            PPart::Lit { text, quoted } => render_part(0, text, *quoted, None),
            PPart::Var { name, quoted } => render_part(1, name, *quoted, None),
            PPart::Glob { pattern } => render_part(2, pattern, false, None),
            PPart::Capture { quoted, .. } => render_part(3, "", *quoted, ids.next()),
        })
        .collect()
}

fn redirect(r: &PRedirect, ids: &mut impl Iterator<Item = u32>) -> String {
    let (target, tfd) = match &r.target {
        PRedirectTarget::Word { word: w } | PRedirectTarget::Heredoc { body: w } => {
            (word(w, ids), 0)
        }
        PRedirectTarget::Fd { fd } => (String::new(), *fd),
    };
    render_redirect(r.fd, redirect_op_index(&r.op), &target, tfd)
}

// ---- derived skeleton from the component ------------------------------------

#[derive(Debug, Clone, Copy)]
enum Val {
    Const(u32),
    Pipe(u32),
    Unknown,
}

struct Memory(Vec<u8>);

impl Memory {
    fn bytes(&self, p: u32, n: u32) -> Result<&[u8], CompileError> {
        self.0
            .get(p as usize..p as usize + n as usize)
            .ok_or_else(|| {
                mismatch(format!(
                    "argument data {p}+{n} lies outside the data segment"
                ))
            })
    }
    fn u32(&self, p: u32) -> Result<u32, CompileError> {
        Ok(u32::from_le_bytes(self.bytes(p, 4)?.try_into().unwrap()))
    }
    fn u8(&self, p: u32) -> Result<u8, CompileError> {
        Ok(self.bytes(p, 1)?[0])
    }
    fn string(&self, p: u32, n: u32) -> Result<String, CompileError> {
        String::from_utf8(self.bytes(p, n)?.to_vec())
            .map_err(|_| mismatch(format!("string at {p} is not UTF-8")))
    }
    fn word(&self, p: u32, n: u32, slots: &BTreeMap<u32, u32>) -> Result<String, CompileError> {
        let mut out = String::new();
        for i in 0..n {
            let at = p + PART_SIZE * i;
            let kind = self.u8(at)?;
            out.push_str(&match kind {
                0 | 1 => render_part(
                    kind,
                    &self.string(self.u32(at + 4)?, self.u32(at + 8)?)?,
                    self.u8(at + 12)? != 0,
                    None,
                ),
                2 => render_part(
                    2,
                    &self.string(self.u32(at + 4)?, self.u32(at + 8)?)?,
                    false,
                    None,
                ),
                3 => render_part(3, "", self.u8(at + 8)? != 0, slots.get(&(at + 4)).copied()),
                d => return Err(mismatch(format!("part at {at} has discriminant {d}"))),
            });
        }
        Ok(out)
    }
    fn words(
        &self,
        p: u32,
        n: u32,
        slots: &BTreeMap<u32, u32>,
    ) -> Result<Vec<String>, CompileError> {
        (0..n)
            .map(|i| {
                let at = p + WORD_SIZE * i;
                self.word(self.u32(at)?, self.u32(at + 4)?, slots)
            })
            .collect()
    }
}

fn derive(component: &[u8]) -> Result<Vec<Ev>, CompileError> {
    let mut features = WasmFeatures::default();
    features.insert(WasmFeatures::COMPONENT_MODEL);
    Validator::new_with_features(features)
        .validate_all(component)
        .map_err(|e| mismatch(format!("component does not validate: {e}")))?;

    // Top level: the only import is the host interface; `run` is exported.
    let mut modules = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut depth = 0u32;
    for payload in Parser::new(0).parse_all(component) {
        let payload = payload.map_err(|e| mismatch(format!("component does not parse: {e}")))?;
        match payload {
            Payload::ModuleSection {
                unchecked_range, ..
            } => {
                modules
                    .push(&component[unchecked_range.start as usize..unchecked_range.end as usize]);
                depth += 1;
            }
            Payload::ComponentSection { .. } => depth += 1,
            Payload::End(_) if depth > 0 => depth -= 1,
            Payload::ComponentImportSection(r) if depth == 0 => {
                for i in r {
                    let i = i.map_err(|e| mismatch(e.to_string()))?;
                    imports.push(i.name.full_name().into_owned());
                }
            }
            Payload::ComponentExportSection(r) if depth == 0 => {
                for e in r {
                    let e = e.map_err(|e| mismatch(e.to_string()))?;
                    if e.kind == ComponentExternalKind::Func {
                        exports.push(e.name.full_name().into_owned());
                    }
                }
            }
            _ => {}
        }
    }
    if imports != [HOST_INTERFACE] {
        return Err(mismatch(format!(
            "component imports {imports:?}, expected only {HOST_INTERFACE}"
        )));
    }
    if exports != ["run"] {
        return Err(mismatch(format!(
            "component exports functions {exports:?}, expected only run"
        )));
    }

    let mut main = None;
    for m in modules {
        if let Some(found) = read_main(m)? {
            if main.is_some() {
                return Err(mismatch("more than one core module exports `run`"));
            }
            main = Some(found);
        }
    }
    let main = main.ok_or_else(|| mismatch("no core module exports `run`"))?;
    walk(&main)
}

struct Main<'a> {
    /// Function index → host import name, for imported functions.
    import_names: Vec<String>,
    run_body: wasmparser::FunctionBody<'a>,
    memory: Memory,
}

fn read_main(module: &[u8]) -> Result<Option<Main<'_>>, CompileError> {
    let err = |e: wasmparser::BinaryReaderError| mismatch(e.to_string());
    let mut import_names = Vec::new();
    let mut run_index = None;
    let mut bodies = Vec::new();
    let mut mem = Vec::new();
    for payload in Parser::new(0).parse_all(module) {
        match payload.map_err(err)? {
            Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.map_err(err)?;
                    if let TypeRef::Func(_) = i.ty {
                        import_names.push(if i.module == HOST_INTERFACE {
                            i.name.to_owned()
                        } else {
                            format!("{}#{}", i.module, i.name)
                        });
                    }
                }
            }
            Payload::ExportSection(r) => {
                for e in r {
                    let e = e.map_err(err)?;
                    if e.name == "run" && e.kind == ExternalKind::Func {
                        run_index = Some(e.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => bodies.push(body),
            Payload::DataSection(r) => {
                for d in r {
                    let d = d.map_err(err)?;
                    if let DataKind::Active { offset_expr, .. } = d.kind {
                        let mut ops = offset_expr.get_operators_reader();
                        let Operator::I32Const { value } = ops.read().map_err(err)? else {
                            return Err(mismatch("data segment offset is not an i32.const"));
                        };
                        let at = value as u32 as usize;
                        if mem.len() < at + d.data.len() {
                            mem.resize(at + d.data.len(), 0);
                        }
                        mem[at..at + d.data.len()].copy_from_slice(d.data);
                    }
                }
            }
            _ => {}
        }
    }
    let Some(run) = run_index else {
        return Ok(None);
    };
    for (i, name) in import_names.iter().enumerate() {
        if !IMPORTS.contains(&name.as_str()) {
            return Err(mismatch(format!(
                "core import {i} is `{name}`, not a host function"
            )));
        }
    }
    let local = (run as usize)
        .checked_sub(import_names.len())
        .ok_or_else(|| mismatch("`run` is an imported function"))?;
    if bodies.len() != 1 || local != 0 {
        return Err(mismatch(format!(
            "expected exactly one defined function, found {}",
            bodies.len()
        )));
    }
    Ok(Some(Main {
        import_names,
        run_body: bodies.remove(0),
        memory: Memory(mem),
    }))
}

fn walk(m: &Main<'_>) -> Result<Vec<Ev>, CompileError> {
    let err = |e: wasmparser::BinaryReaderError| mismatch(e.to_string());
    let mut evs = Vec::new();
    let mut stack: Vec<Val> = Vec::new();
    let mut locals: BTreeMap<u32, Val> = BTreeMap::new();
    let mut slots: BTreeMap<u32, u32> = BTreeMap::new();
    // Open control structures; true for a scope's block.
    let mut control: Vec<bool> = Vec::new();
    let mut pipes = 0u32;
    let pop = |stack: &mut Vec<Val>| {
        stack
            .pop()
            .ok_or_else(|| mismatch("operand stack underflow"))
    };
    let konst = |v: Val| match v {
        Val::Const(c) => Ok(c),
        other => Err(mismatch(format!(
            "host argument is {other:?}, not a compile-time constant"
        ))),
    };
    let io = |v: Val| match v {
        Val::Const(0) => Ok(Io::Session),
        Val::Pipe(k) => Ok(Io::Pipe(k)),
        other => Err(mismatch(format!(
            "stdin/stdout is {other:?}, not 0 or a pipe() result"
        ))),
    };
    let mem = &m.memory;

    let mut ops = m.run_body.get_operators_reader().map_err(err)?;
    while !ops.eof() {
        let op = ops.read().map_err(err)?;
        match op {
            Operator::I32Const { value } => stack.push(Val::Const(value as u32)),
            Operator::LocalGet { local_index } => {
                stack.push(locals.get(&local_index).copied().unwrap_or(Val::Unknown))
            }
            Operator::LocalSet { local_index } => {
                let v = pop(&mut stack)?;
                // A local set more than once (the status) is not traced.
                let v = if locals.contains_key(&local_index) {
                    Val::Unknown
                } else {
                    v
                };
                locals.insert(local_index, v);
            }
            Operator::I32Eqz | Operator::I32Load { .. } => {
                pop(&mut stack)?;
                stack.push(Val::Unknown);
            }
            Operator::I32GeU | Operator::I32Sub | Operator::I32Add => {
                pop(&mut stack)?;
                pop(&mut stack)?;
                stack.push(Val::Unknown);
            }
            Operator::I32Store { .. } => {
                let v = pop(&mut stack)?;
                let addr = konst(pop(&mut stack)?)?;
                let Val::Pipe(k) = v else {
                    return Err(mismatch(format!(
                        "store to {addr} of {v:?}, not a pipe handle"
                    )));
                };
                slots.insert(addr, k);
                evs.push(Ev::StoreCapture(k));
            }
            Operator::Block { .. } => {
                // A block opened right after scope-push is that scope's block.
                control.push(evs.last() == Some(&Ev::ScopePush));
                evs.push(Ev::Block);
            }
            Operator::Loop { .. } => {
                control.push(false);
                evs.push(Ev::Loop);
            }
            Operator::If { .. } => {
                pop(&mut stack)?;
                control.push(false);
                evs.push(Ev::If);
            }
            Operator::Else => evs.push(Ev::Else),
            Operator::End => {
                control.pop();
                evs.push(Ev::End);
            }
            Operator::Br { relative_depth } | Operator::BrIf { relative_depth } => {
                if matches!(op, Operator::BrIf { .. }) {
                    pop(&mut stack)?;
                }
                let d = relative_depth as usize;
                if d >= control.len() {
                    return Err(mismatch(format!("branch depth {d} leaves the function")));
                }
                let target = control.len() - 1 - d;
                let crossed = control[target + 1..].iter().any(|s| *s);
                if control[target] || crossed {
                    if crossed {
                        return Err(mismatch("a branch leaves more than one scope"));
                    }
                    evs.push(Ev::ExitScope);
                }
            }
            Operator::Return => {
                pop(&mut stack)?;
                evs.push(Ev::Return);
            }
            Operator::Call { function_index } => {
                let name = m.import_names.get(function_index as usize).ok_or_else(|| {
                    mismatch(format!("call to non-import function {function_index}"))
                })?;
                match name.as_str() {
                    "pipe" => {
                        stack.push(Val::Pipe(pipes));
                        evs.push(Ev::Pipe(pipes));
                        pipes += 1;
                    }
                    "call" => {
                        let mut a = [Val::Unknown; 8];
                        for slot in a.iter_mut().rev() {
                            *slot = pop(&mut stack)?;
                        }
                        let (ap, al) = (konst(a[0])?, konst(a[1])?);
                        let (sp, sl) = (konst(a[2])?, konst(a[3])?);
                        let (rp, rl) = (konst(a[4])?, konst(a[5])?);
                        let argv = mem.words(ap, al, &slots)?;
                        let mut assignments = Vec::new();
                        for i in 0..sl {
                            let at = sp + ASSIGN_SIZE * i;
                            let n = mem.string(mem.u32(at)?, mem.u32(at + 4)?)?;
                            let w = mem.word(mem.u32(at + 8)?, mem.u32(at + 12)?, &slots)?;
                            assignments.push((n, w));
                        }
                        let mut redirects = Vec::new();
                        for i in 0..rl {
                            let at = rp + REDIRECT_SIZE * i;
                            let target = mem.word(mem.u32(at + 8)?, mem.u32(at + 12)?, &slots)?;
                            redirects.push(render_redirect(
                                mem.u32(at)?,
                                mem.u8(at + 4)?,
                                &target,
                                mem.u32(at + 16)?,
                            ));
                        }
                        evs.push(Ev::Call {
                            argv,
                            assignments,
                            redirects,
                            stdin: io(a[6])?,
                            stdout: io(a[7])?,
                        });
                        stack.push(Val::Unknown);
                    }
                    "set-var" => {
                        let wl = konst(pop(&mut stack)?)?;
                        let wp = konst(pop(&mut stack)?)?;
                        let nl = konst(pop(&mut stack)?)?;
                        let np = konst(pop(&mut stack)?)?;
                        evs.push(Ev::SetVar(mem.string(np, nl)?, mem.word(wp, wl, &slots)?));
                    }
                    "set-status" => {
                        pop(&mut stack)?;
                        evs.push(Ev::SetStatus);
                    }
                    "expand-list" => {
                        pop(&mut stack)?;
                        let wl = konst(pop(&mut stack)?)?;
                        let wp = konst(pop(&mut stack)?)?;
                        evs.push(Ev::ExpandList(mem.words(wp, wl, &slots)?));
                    }
                    "bind-item" => {
                        pop(&mut stack)?;
                        pop(&mut stack)?;
                        let vl = konst(pop(&mut stack)?)?;
                        let vp = konst(pop(&mut stack)?)?;
                        evs.push(Ev::BindItem(mem.string(vp, vl)?));
                    }
                    "scope-push" => evs.push(Ev::ScopePush),
                    "scope-pop" => evs.push(Ev::ScopePop),
                    other => return Err(mismatch(format!("call to unknown import `{other}`"))),
                }
            }
            other => return Err(mismatch(format!("unexpected operator {other:?} in run"))),
        }
    }
    Ok(evs)
}
