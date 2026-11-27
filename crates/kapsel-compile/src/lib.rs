//! Plan to WASM component compiler (REQ-KS-020, DD-KS-005).
//!
//! Owner: compiler workstream. Emits a component of world `kapsel:shell/plan`
//! (`wit/kapsel-shell.wit`). The same plan always gives byte-identical output.
//!
//! # Shape of the output
//!
//! The core module has no allocator and no `cabi_realloc`: every argument the
//! plan passes to the host (strings, words, redirect lists, assignment lists)
//! is laid out at compile time in one active data segment, in canonical ABI
//! layout. The whole plan compiles into the single exported function `run`;
//! there is no recursion in a plan, so captures and loop bodies are inlined.
//!
//! Memory map: bytes `0..8` are unused, `8..16` is the return area for
//! `expand-list`, the data segment starts at [`DATA_BASE`].
//!
//! A `$(...)` part becomes a `captured` part whose `pipe` field is a slot in
//! the data segment. Right before the call that uses the word, the module
//! creates a pipe, runs the capture body with stdout set to that pipe and
//! stores the handle into the slot with `i32.store`.
//!
//! Status: `set-status` follows every executed pipeline (after `!` is
//! applied), so `$?` is right both inside an and-or list and after it.

use std::collections::BTreeMap;

use kapsel_plan::{
    AndOrOp, Call, PAndOr, PPart, PPipeline, PRedirect, PRedirectTarget, PStage, PWord, Plan,
    RedirectOp,
};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, ImportSection, Instruction, MemArg, MemArgTarget, MemorySection, MemoryType,
    Module, TypeSection, ValType,
};

/// The WIT the compiled component targets, embedded so host and compiler use
/// the same text.
pub const SHELL_WIT: &str = include_str!("../../../wit/kapsel-shell.wit");

/// The interface every compiled plan imports.
pub const HOST_INTERFACE: &str = "kapsel:shell/host@0.1.0";

/// Address of the `expand-list` return area (two u32s).
const RET_AREA: u32 = 8;
/// Address where the compile-time data segment starts.
pub const DATA_BASE: u32 = 16;

// Core function indices: the imports in order, then `run`.
const F_PIPE: u32 = 0;
const F_CALL: u32 = 1;
const F_SET_VAR: u32 = 2;
const F_SET_STATUS: u32 = 3;
const F_EXPAND_LIST: u32 = 4;
const F_BIND_ITEM: u32 = 5;
const F_RUN: u32 = 6;

#[derive(Debug)]
pub struct CompileError(pub String);

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CompileError {}

/// Compiles a checked plan into component bytes.
pub fn compile(plan: &Plan) -> Result<Vec<u8>, CompileError> {
    let mut module = compile_core(plan)?;

    let mut resolve = wit_parser::Resolve::new();
    let pkg = resolve
        .push_str("kapsel-shell.wit", SHELL_WIT)
        .map_err(|e| CompileError(format!("parsing the shell WIT: {e:#}")))?;
    let world = resolve
        .select_world(&[pkg], Some("plan"))
        .map_err(|e| CompileError(format!("selecting world `plan`: {e:#}")))?;
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        false,
    )
    .map_err(|e| CompileError(format!("embedding component metadata: {e:#}")))?;

    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .map_err(|e| CompileError(format!("reading the core module: {e:#}")))?
        .encode()
        .map_err(|e| CompileError(format!("encoding the component: {e:#}")))
}

/// Compiles a checked plan into the core module only (no component metadata).
/// Useful with `wasm-tools print` when debugging the generated code.
pub fn compile_core(plan: &Plan) -> Result<Vec<u8>, CompileError> {
    let mut cx = Cx::default();
    let status = cx.new_local();
    cx.status = status;
    cx.list(&plan.body, Handle::Session, Handle::Session)?;
    cx.emit(Instruction::LocalGet(status));
    cx.emit(Instruction::End);
    Ok(cx.finish())
}

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
}

impl Data {
    fn alloc(&mut self, size: u32, align: u32) -> u32 {
        while self.bytes.len() as u32 % align != 0 {
            self.bytes.push(0);
        }
        let addr = DATA_BASE + self.bytes.len() as u32;
        self.bytes.resize(self.bytes.len() + size as usize, 0);
        addr
    }

    fn put_u32(&mut self, addr: u32, v: u32) {
        let o = (addr - DATA_BASE) as usize;
        self.bytes[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u8(&mut self, addr: u32, v: u8) {
        self.bytes[(addr - DATA_BASE) as usize] = v;
    }

    /// Returns `(ptr, len)`; identical strings share one copy.
    fn string(&mut self, s: &str) -> (u32, u32) {
        if let Some(&p) = self.strings.get(s) {
            return (p, s.len() as u32);
        }
        let p = self.alloc(s.len() as u32, 1);
        let o = (p - DATA_BASE) as usize;
        self.bytes[o..o + s.len()].copy_from_slice(s.as_bytes());
        self.strings.insert(s.to_owned(), p);
        (p, s.len() as u32)
    }
}

// Canonical ABI layouts of the WIT types (all align 4):
//   part (variant, u8 discriminant, payload at 4): 16 bytes
//     lit/var: string @4, quoted @12; glob: string @4; captured: pipe @4, quoted @8
//   word = list<part>: 8 bytes (ptr, len)
//   tuple<string, word>: 16 bytes
//   redirect: fd @0, op (u8) @4, target @8, target-fd @16: 20 bytes
const PART_SIZE: u32 = 16;
const WORD_SIZE: u32 = 8;
const ASSIGN_SIZE: u32 = 16;
const REDIRECT_SIZE: u32 = 20;

/// Target of a `2>&1`-style redirect: the fd lives in `target-fd`.
static EMPTY_WORD: PWord = PWord { parts: Vec::new() };

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
}

impl Cx {
    fn emit(&mut self, i: Instruction<'static>) {
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

    fn set_status_from_local(&mut self) {
        self.emit(Instruction::LocalGet(self.status));
        self.emit(Instruction::Call(F_SET_STATUS));
    }

    // ---- data layout -------------------------------------------------------

    /// Lays out a word; `$(...)` parts are pushed onto `caps` in order.
    fn word<'a>(&mut self, w: &'a PWord, caps: &mut Vec<Capture<'a>>) -> (u32, u32) {
        let n = w.parts.len() as u32;
        let base = self.data.alloc(PART_SIZE * n, 4);
        for (i, p) in w.parts.iter().enumerate() {
            let at = base + PART_SIZE * i as u32;
            match p {
                PPart::Lit { text, quoted } | PPart::Var { name: text, quoted } => {
                    let disc = if matches!(p, PPart::Lit { .. }) { 0 } else { 1 };
                    let (sp, sl) = self.data.string(text);
                    self.data.put_u8(at, disc);
                    self.data.put_u32(at + 4, sp);
                    self.data.put_u32(at + 8, sl);
                    self.data.put_u8(at + 12, *quoted as u8);
                }
                PPart::Glob { pattern } => {
                    let (sp, sl) = self.data.string(pattern);
                    self.data.put_u8(at, 2);
                    self.data.put_u32(at + 4, sp);
                    self.data.put_u32(at + 8, sl);
                }
                PPart::Capture { body, quoted } => {
                    self.data.put_u8(at, 3);
                    self.data.put_u8(at + 8, *quoted as u8);
                    caps.push(Capture { slot: at + 4, body });
                }
            }
        }
        (base, n)
    }

    fn word_list<'a>(&mut self, ws: &'a [PWord], caps: &mut Vec<Capture<'a>>) -> (u32, u32) {
        let n = ws.len() as u32;
        let base = self.data.alloc(WORD_SIZE * n, 4);
        for (i, w) in ws.iter().enumerate() {
            let (p, l) = self.word(w, caps);
            let at = base + WORD_SIZE * i as u32;
            self.data.put_u32(at, p);
            self.data.put_u32(at + 4, l);
        }
        (base, n)
    }

    fn redirects<'a>(&mut self, rs: &'a [PRedirect], caps: &mut Vec<Capture<'a>>) -> (u32, u32) {
        let n = rs.len() as u32;
        let base = self.data.alloc(REDIRECT_SIZE * n, 4);

        for (i, r) in rs.iter().enumerate() {
            let at = base + REDIRECT_SIZE * i as u32;
            let op = match r.op {
                RedirectOp::Read => 0,
                RedirectOp::Write => 1,
                RedirectOp::Append => 2,
                RedirectOp::DupOut => 3,
                RedirectOp::DupIn => 4,
                RedirectOp::Heredoc => 5,
            };
            let ((wp, wl), target_fd) = match &r.target {
                PRedirectTarget::Word { word } => (self.word(word, caps), 0),
                PRedirectTarget::Heredoc { body } => (self.word(body, caps), 0),
                PRedirectTarget::Fd { fd } => (self.word(&EMPTY_WORD, caps), *fd),
            };
            self.data.put_u32(at, r.fd);
            self.data.put_u8(at + 4, op);
            self.data.put_u32(at + 8, wp);
            self.data.put_u32(at + 12, wl);
            self.data.put_u32(at + 16, target_fd);
        }
        (base, n)
    }

    fn assignments<'a>(
        &mut self,
        asg: &'a [(String, PWord)],
        caps: &mut Vec<Capture<'a>>,
    ) -> (u32, u32) {
        let n = asg.len() as u32;
        let base = self.data.alloc(ASSIGN_SIZE * n, 4);
        for (i, (name, w)) in asg.iter().enumerate() {
            let at = base + ASSIGN_SIZE * i as u32;
            let (np, nl) = self.data.string(name);
            let (wp, wl) = self.word(w, caps);
            self.data.put_u32(at, np);
            self.data.put_u32(at + 4, nl);
            self.data.put_u32(at + 8, wp);
            self.data.put_u32(at + 12, wl);
        }
        (base, n)
    }

    // ---- control flow ------------------------------------------------------

    /// `;`-separated and-or lists. Leaves the last status in `status`.
    fn list(&mut self, list: &[PAndOr], stdin: Handle, stdout: Handle) -> Result<(), CompileError> {
        for ao in list {
            self.and_or(ao, stdin, stdout)?;
        }
        Ok(())
    }

    fn and_or(&mut self, ao: &PAndOr, stdin: Handle, stdout: Handle) -> Result<(), CompileError> {
        self.pipeline(&ao.first, stdin, stdout)?;
        for (op, p) in &ao.rest {
            self.emit(Instruction::LocalGet(self.status));
            if matches!(op, AndOrOp::And) {
                self.emit(Instruction::I32Eqz);
            }
            self.emit(Instruction::If(BlockType::Empty));
            self.pipeline(p, stdin, stdout)?;
            self.emit(Instruction::End);
        }
        Ok(())
    }

    fn pipeline(&mut self, p: &PPipeline, stdin: Handle, stdout: Handle) -> Result<(), CompileError> {
        let n = p.stages.len();
        if n == 0 {
            return Err(CompileError("empty pipeline in plan".into()));
        }
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
            self.stage(stage, sin, sout)?;
        }
        if p.negated {
            self.emit(Instruction::LocalGet(self.status));
            self.emit(Instruction::I32Eqz);
            self.emit(Instruction::LocalSet(self.status));
        }
        self.set_status_from_local();
        Ok(())
    }

    fn stage(&mut self, s: &PStage, stdin: Handle, stdout: Handle) -> Result<(), CompileError> {
        match s {
            PStage::Call(c) => self.call(c, stdin, stdout),
            PStage::For { var, items, body } => {
                let mut caps = Vec::new();
                let (wp, wl) = self.word_list(items, &mut caps);
                self.run_captures(&caps, stdin)?;
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
                self.list(body, stdin, stdout)?;
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
            PStage::While { until, condition, body } => {
                let last = self.new_local();
                self.i32(0);
                self.emit(Instruction::LocalSet(last));
                self.emit(Instruction::Block(BlockType::Empty));
                self.emit(Instruction::Loop(BlockType::Empty));
                self.list(condition, stdin, stdout)?;
                self.emit(Instruction::LocalGet(self.status));
                if *until {
                    // until: leave when the condition succeeds
                    self.emit(Instruction::I32Eqz);
                }
                self.emit(Instruction::BrIf(1));
                self.list(body, stdin, stdout)?;
                self.emit(Instruction::LocalGet(self.status));
                self.emit(Instruction::LocalSet(last));
                self.emit(Instruction::Br(0));
                self.emit(Instruction::End);
                self.emit(Instruction::End);
                self.emit(Instruction::LocalGet(last));
                self.emit(Instruction::LocalSet(self.status));
                Ok(())
            }
        }
    }

    /// Runs each capture body into a fresh pipe and stores the handle into the
    /// capture's slot. Leaves the last body's status in `status`.
    fn run_captures(&mut self, caps: &[Capture<'_>], stdin: Handle) -> Result<(), CompileError> {
        for c in caps {
            let pipe = self.new_local();
            self.emit(Instruction::Call(F_PIPE));
            self.emit(Instruction::LocalSet(pipe));
            self.list(c.body, stdin, Handle::Local(pipe))?;
            self.i32(c.slot);
            self.emit(Instruction::LocalGet(pipe));
            self.emit(Instruction::I32Store(mem(0)));
        }
        Ok(())
    }

    fn call(&mut self, c: &Call, stdin: Handle, stdout: Handle) -> Result<(), CompileError> {
        let mut caps = Vec::new();
        if c.tool.is_none() {
            // `X=1 Y=$(...)`: status 0, or the last capture's status (POSIX).
            let mut sets = Vec::with_capacity(c.assignments.len());
            for (name, w) in &c.assignments {
                let (np, nl) = self.data.string(name);
                let (wp, wl) = self.word(w, &mut caps);
                sets.push((np, nl, wp, wl));
            }
            self.i32(0);
            self.emit(Instruction::LocalSet(self.status));
            self.run_captures(&caps, stdin)?;
            for (np, nl, wp, wl) in sets {
                self.i32(np);
                self.i32(nl);
                self.i32(wp);
                self.i32(wl);
                self.emit(Instruction::Call(F_SET_VAR));
            }
            return Ok(());
        }
        // POSIX expansion order: command words, redirections, assignments.
        let (ap, al) = self.word_list(&c.argv, &mut caps);
        let (rp, rl) = self.redirects(&c.redirects, &mut caps);
        let (sp, sl) = self.assignments(&c.assignments, &mut caps);
        self.run_captures(&caps, stdin)?;
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
        // 256 + n: the built-in was `exit n`; stop the whole plan.
        self.emit(Instruction::LocalGet(self.status));
        self.i32(256);
        self.emit(Instruction::I32GeU);
        self.emit(Instruction::If(BlockType::Empty));
        self.emit(Instruction::LocalGet(self.status));
        self.i32(256);
        self.emit(Instruction::I32Sub);
        self.emit(Instruction::Return);
        self.emit(Instruction::End);
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

        let mut imports = ImportSection::new();
        for (name, ty) in [
            ("pipe", 0),
            ("call", 1),
            ("set-var", 2),
            ("set-status", 3),
            ("expand-list", 4),
            ("bind-item", 2),
        ] {
            imports.import(HOST_INTERFACE, name, EntityType::Function(ty));
        }

        let mut funcs = FunctionSection::new();
        funcs.function(0);

        let total = DATA_BASE as u64 + self.data.bytes.len() as u64;
        let pages = total.div_ceil(65536).max(1);
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: pages,
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
        data.active(0, &ConstExpr::i32_const(DATA_BASE as i32), self.data.bytes.iter().copied());

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

fn mem(offset: u64) -> MemArg {
    MemArg { offset, align: 2, target: MemArgTarget::Memory(0), ordering: None }
}
