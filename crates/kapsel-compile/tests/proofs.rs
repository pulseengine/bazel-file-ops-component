//! ordeal proofs for kapsel-compile (v0.3.0, owner decision 2026-11-28).
//!
//! Two families of QF_BV obligations, each committed under `proofs/` as its
//! `.smt2` query. Certificates are not committed: this test produces them
//! (see `proofs/README.md`):
//!
//! - **Status logic, per lowering rule** (`KC-Rnn-*.smt2`). For a fixture
//!   plan per rule, the *implementation* side is derived by symbolically
//!   executing the wasm operators that `compile_core` actually emits for
//!   `run` (host call results are fresh 32-bit variables); the
//!   *specification* side is a symbolic interpreter of the POSIX semantics
//!   over the plan, written here without the lowering code. Both produce
//!   paths `(condition, trace)`; a trace is the sequence of status-relevant
//!   host events (`call`, `expand-list`, `set-status v`, `scope-push`,
//!   `scope-pop`) and the final `return v`. The obligation asserts that some
//!   implementation path and some specification path are both possible for
//!   the same host results and their traces differ. `unsat` = equal traces
//!   for every host result. Bound: a call site runs at most `BOUND` times on
//!   a path; inputs the specification would run past it are excluded, and an
//!   implementation path that stops at the bound where the specification
//!   does not is a difference.
//! - **Memory layout** (`layout-*.smt2`), over symbolic sizes within the
//!   allocator's limit: the allocator's rounding, the in-bounds property of
//!   every allocation inside the declared memory, and each record field's
//!   alignment and containment for any element index.
//!
//! The test regenerates every `.smt2` and fails when it differs from the
//! committed one (a stale proof; it then reports ordeal's verdict on the
//! current obligation, with the counterexample if it is SAT). It solves each
//! committed query with ordeal in-process (it must be `unsat`), writes the
//! `ordeal-cert/v5` bundle to `$KAPSEL_PROOF_OUT` (a temp dir if unset),
//! reads it back, re-checks it with ordeal's trusted checker
//! (`EchoBundle::recheck`, which re-derives the bundle's SMT-LIB echo with
//! the trusted printer), and checks that echo reads back to the committed
//! `.smt2`'s assertions, so the certificate is about that file.
//! `KAPSEL_BLESS=1` rewrites the `.smt2` files.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kapsel_compile::{RULES, compile_core, layout};
use kapsel_plan::{AndOrOp, PAndOr, PPart, PPipeline, PRedirectTarget, PStage, PWord, Plan};

/// Each host call site runs at most this often on a path.
const BOUND: u32 = 3;

thread_local! {
    /// Width of the layout arithmetic, set per obligation group so that no
    /// term wraps (see `width_note`).
    static LW: std::cell::Cell<u32> = const { std::cell::Cell::new(34) };
}
fn lw() -> u32 {
    LW.with(|w| w.get())
}
fn set_lw(w: u32) {
    LW.with(|c| c.set(w));
}

// ---- terms and SMT-LIB printing ----------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum T {
    C(u64, u32),
    V(String, u32),
    Add(Box<T>, Box<T>),
    Sub(Box<T>, Box<T>),
    Shl(Box<T>, Box<T>),
    Lshr(Box<T>, Box<T>),
    BvAnd(Box<T>, Box<T>),
    Ite(Box<P>, Box<T>, Box<T>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum P {
    Const(bool),
    Eq(T, T),
    Ult(T, T),
    Ule(T, T),
    Not(Box<P>),
    And(Vec<P>),
    Or(Vec<P>),
}

fn c32(v: u32) -> T {
    T::C(v as u64, 32)
}
fn c64(v: u64) -> T {
    T::C(v & mask(lw()), lw())
}
fn v32(n: &str) -> T {
    T::V(n.into(), 32)
}
fn v64(n: &str) -> T {
    T::V(n.into(), lw())
}
fn add(a: T, b: T) -> T {
    T::Add(a.into(), b.into())
}
fn sub(a: T, b: T) -> T {
    match (&a, &b) {
        (T::C(x, w), T::C(y, _)) => T::C(x.wrapping_sub(*y) & mask(*w), *w),
        _ => T::Sub(a.into(), b.into()),
    }
}
fn mask(w: u32) -> u64 {
    if w == 64 { u64::MAX } else { (1u64 << w) - 1 }
}
fn eq(a: T, b: T) -> P {
    match (&a, &b) {
        (T::C(x, _), T::C(y, _)) => P::Const(x == y),
        _ => P::Eq(a, b),
    }
}
fn ult(a: T, b: T) -> P {
    match (&a, &b) {
        (T::C(x, _), T::C(y, _)) => P::Const(x < y),
        _ => P::Ult(a, b),
    }
}
fn ule(a: T, b: T) -> P {
    match (&a, &b) {
        (T::C(x, _), T::C(y, _)) => P::Const(x <= y),
        _ => P::Ule(a, b),
    }
}
fn not(p: P) -> P {
    match p {
        P::Const(b) => P::Const(!b),
        P::Not(q) => *q,
        q => P::Not(q.into()),
    }
}
fn and(ps: Vec<P>) -> P {
    let mut out = Vec::new();
    for p in ps {
        match p {
            P::Const(true) => {}
            P::Const(false) => return P::Const(false),
            P::And(qs) => out.extend(qs),
            q => out.push(q),
        }
    }
    match out.len() {
        0 => P::Const(true),
        1 => out.pop().unwrap(),
        _ => P::And(out),
    }
}
fn or(ps: Vec<P>) -> P {
    let mut out = Vec::new();
    for p in ps {
        match p {
            P::Const(false) => {}
            P::Const(true) => return P::Const(true),
            q => out.push(q),
        }
    }
    match out.len() {
        0 => P::Const(false),
        1 => out.pop().unwrap(),
        _ => P::Or(out),
    }
}
/// A boolean as a 32-bit 0/1 value (what `i32.eqz` and the compares push).
fn b2t(p: P) -> T {
    match p {
        P::Const(b) => c32(b as u32),
        p => T::Ite(p.into(), c32(1).into(), c32(0).into()),
    }
}
/// `t != 0` as a condition, unwrapping a 0/1 value.
fn truthy(t: T) -> P {
    match t {
        T::Ite(p, a, b) if *a == c32(1) && *b == c32(0) => *p,
        t => not(eq(t, c32(0))),
    }
}

fn smt_t(t: &T, out: &mut String) {
    match t {
        T::C(v, w) => write!(out, "(_ bv{v} {w})").unwrap(),
        T::V(n, _) => out.push_str(n),
        T::Add(a, b) => bin(out, "bvadd", a, b),
        T::Sub(a, b) => bin(out, "bvsub", a, b),
        T::Shl(a, b) => bin(out, "bvshl", a, b),
        T::Lshr(a, b) => bin(out, "bvlshr", a, b),
        T::BvAnd(a, b) => bin(out, "bvand", a, b),
        T::Ite(p, a, b) => {
            out.push_str("(ite ");
            smt_p(p, out);
            out.push(' ');
            smt_t(a, out);
            out.push(' ');
            smt_t(b, out);
            out.push(')');
        }
    }
}
fn bin(out: &mut String, op: &str, a: &T, b: &T) {
    write!(out, "({op} ").unwrap();
    smt_t(a, out);
    out.push(' ');
    smt_t(b, out);
    out.push(')');
}
fn smt_p(p: &P, out: &mut String) {
    let pair = |out: &mut String, op: &str, a: &T, b: &T| {
        write!(out, "({op} ").unwrap();
        smt_t(a, out);
        out.push(' ');
        smt_t(b, out);
        out.push(')');
    };
    match p {
        P::Const(b) => out.push_str(if *b { "true" } else { "false" }),
        P::Eq(a, b) => pair(out, "=", a, b),
        P::Ult(a, b) => pair(out, "bvult", a, b),
        P::Ule(a, b) => pair(out, "bvule", a, b),
        P::Not(q) => {
            out.push_str("(not ");
            smt_p(q, out);
            out.push(')');
        }
        P::And(qs) | P::Or(qs) => {
            out.push_str(if matches!(p, P::And(_)) {
                "(and"
            } else {
                "(or"
            });
            for q in qs {
                out.push_str("\n  ");
                smt_p(q, out);
            }
            out.push(')');
        }
    }
}
fn vars_t(t: &T, out: &mut BTreeMap<String, u32>) {
    match t {
        T::C(..) => {}
        T::V(n, w) => {
            out.insert(n.clone(), *w);
        }
        T::Add(a, b) | T::Sub(a, b) | T::Shl(a, b) | T::Lshr(a, b) | T::BvAnd(a, b) => {
            vars_t(a, out);
            vars_t(b, out);
        }
        T::Ite(p, a, b) => {
            vars_p(p, out);
            vars_t(a, out);
            vars_t(b, out);
        }
    }
}
fn vars_p(p: &P, out: &mut BTreeMap<String, u32>) {
    match p {
        P::Const(_) => {}
        P::Eq(a, b) | P::Ult(a, b) | P::Ule(a, b) => {
            vars_t(a, out);
            vars_t(b, out);
        }
        P::Not(q) => vars_p(q, out),
        P::And(qs) | P::Or(qs) => qs.iter().for_each(|q| vars_p(q, out)),
    }
}

/// Concrete evaluation, used to check that the layout formulas compute what
/// the Rust allocator computes.
fn eval(t: &T, env: &BTreeMap<&str, u64>) -> u64 {
    let w = |t: &T| match t {
        T::C(_, w) | T::V(_, w) => *w,
        _ => 64,
    };
    let m = mask(w(t).max(32));
    match t {
        T::C(v, _) => *v,
        T::V(n, _) => env[n.as_str()],
        T::Add(a, b) => eval(a, env).wrapping_add(eval(b, env)) & m,
        T::Shl(a, b) => (eval(a, env) << eval(b, env)) & m,
        T::Lshr(a, b) => eval(a, env) >> eval(b, env),
        T::Sub(a, b) => eval(a, env).wrapping_sub(eval(b, env)) & m,
        T::BvAnd(a, b) => eval(a, env) & eval(b, env),
        T::Ite(..) => unreachable!("not used by the layout formulas"),
    }
}

/// One SMT-LIB obligation: header comments, declarations, one assertion.
fn script(header: &[String], assertion: &P) -> String {
    let mut vars = BTreeMap::new();
    vars_p(assertion, &mut vars);
    let mut s = String::new();
    for h in header {
        writeln!(s, "; {h}").unwrap();
    }
    s.push_str("(set-logic QF_BV)\n");
    for (n, w) in &vars {
        writeln!(s, "(declare-const {n} (_ BitVec {w}))").unwrap();
    }
    s.push_str("(assert ");
    smt_p(assertion, &mut s);
    s.push_str(")\n(check-sat)\n");
    s
}

// ---- traces --------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    /// A host `call` at static site k.
    Call(u32),
    /// `expand-list` at static site k.
    Expand(u32),
    SetStatus(T),
    ScopePush,
    ScopePop,
    Return(T),
    /// The path reached the call-site bound.
    Bound,
}

#[derive(Debug)]
struct Run {
    cond: Vec<P>,
    trace: Vec<Ev>,
}

/// The input variable for the `iter`-th run of site `k`.
fn call_var(k: u32, iter: u32) -> T {
    v32(&format!("c{k}_{iter}"))
}
fn count_var(k: u32, iter: u32) -> T {
    v32(&format!("n{k}_{iter}"))
}

// ---- implementation side: symbolic execution of the emitted wasm --------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Op {
    Const(u32),
    LocalGet(u32),
    LocalSet(u32),
    Eqz,
    GeU,
    Sub,
    Add,
    Load(u32),
    Store,
    Block,
    Loop,
    If,
    Else,
    End,
    Br(u32),
    BrIf(u32),
    Return,
    Host(&'static str),
}

/// Decodes `run`'s body from a core module emitted by `compile_core`.
fn decode_run(module: &[u8]) -> Vec<Op> {
    use wasmparser::{Operator, Parser, Payload, TypeRef};
    let mut imports: Vec<&'static str> = Vec::new();
    let mut ops = Vec::new();
    for p in Parser::new(0).parse_all(module) {
        match p.unwrap() {
            Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.unwrap();
                    assert!(matches!(i.ty, TypeRef::Func(_)));
                    let name: &'static str = match i.name {
                        "pipe" => "pipe",
                        "call" => "call",
                        "set-var" => "set-var",
                        "set-status" => "set-status",
                        "expand-list" => "expand-list",
                        "bind-item" => "bind-item",
                        "scope-push" => "scope-push",
                        "scope-pop" => "scope-pop",
                        "site" => "site",
                        other => panic!("unknown import {other}"),
                    };
                    imports.push(name);
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut r = body.get_operators_reader().unwrap();
                while !r.eof() {
                    ops.push(match r.read().unwrap() {
                        Operator::I32Const { value } => Op::Const(value as u32),
                        Operator::LocalGet { local_index } => Op::LocalGet(local_index),
                        Operator::LocalSet { local_index } => Op::LocalSet(local_index),
                        Operator::I32Eqz => Op::Eqz,
                        Operator::I32GeU => Op::GeU,
                        Operator::I32Sub => Op::Sub,
                        Operator::I32Add => Op::Add,
                        Operator::I32Load { memarg } => Op::Load(memarg.offset as u32),
                        Operator::I32Store { .. } => Op::Store,
                        Operator::Block { .. } => Op::Block,
                        Operator::Loop { .. } => Op::Loop,
                        Operator::If { .. } => Op::If,
                        Operator::Else => Op::Else,
                        Operator::End => Op::End,
                        Operator::Br { relative_depth } => Op::Br(relative_depth),
                        Operator::BrIf { relative_depth } => Op::BrIf(relative_depth),
                        Operator::Return => Op::Return,
                        Operator::Call { function_index } => {
                            Op::Host(imports[function_index as usize])
                        }
                        other => panic!("operator outside the modelled set: {other:?}"),
                    });
                }
            }
            _ => {}
        }
    }
    ops
}

#[derive(Clone, Copy)]
struct Frame {
    is_loop: bool,
    start: usize,
    end: usize,
}

#[derive(Clone)]
struct WState {
    cond: Vec<P>,
    stack: Vec<T>,
    locals: BTreeMap<u32, T>,
    mem: BTreeMap<u32, T>,
    ip: usize,
    ctl: Vec<Frame>,
    iters: BTreeMap<u32, u32>,
    trace: Vec<Ev>,
}

/// All paths through `run`, forking at every `if` / `br_if` whose condition
/// is not a constant.
fn exec_wasm(ops: &[Op]) -> Vec<Run> {
    // Matching `else` / `end` for every block, loop and if; site numbers.
    let mut end_of = vec![0; ops.len()];
    let mut else_of = vec![None; ops.len()];
    let mut open = Vec::new();
    let mut site_of = vec![0u32; ops.len()];
    let mut sites = 0;
    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Block | Op::Loop | Op::If => open.push(i),
            Op::Else => else_of[*open.last().unwrap()] = Some(i),
            Op::End => {
                if let Some(o) = open.pop() {
                    end_of[o] = i;
                }
            }
            Op::Host("call") | Op::Host("expand-list") => {
                site_of[i] = sites;
                sites += 1;
            }
            _ => {}
        }
    }

    let mut done = Vec::new();
    let mut work = vec![WState {
        cond: vec![],
        stack: vec![],
        locals: BTreeMap::new(),
        mem: BTreeMap::new(),
        ip: 0,
        ctl: vec![],
        iters: BTreeMap::new(),
        trace: vec![],
    }];
    'paths: while let Some(mut s) = work.pop() {
        loop {
            let op = ops[s.ip].clone();
            let pop = |s: &mut WState| s.stack.pop().expect("stack underflow");
            match op {
                Op::Const(v) => s.stack.push(c32(v)),
                Op::LocalGet(l) => {
                    let v = s.locals.get(&l).cloned().unwrap_or(c32(0));
                    s.stack.push(v);
                }
                Op::LocalSet(l) => {
                    let v = pop(&mut s);
                    s.locals.insert(l, v);
                }
                Op::Eqz => {
                    let a = pop(&mut s);
                    s.stack.push(b2t(eq(a, c32(0))));
                }
                Op::GeU => {
                    let b = pop(&mut s);
                    let a = pop(&mut s);
                    s.stack.push(b2t(not(ult(a, b))));
                }
                Op::Sub => {
                    let b = pop(&mut s);
                    let a = pop(&mut s);
                    s.stack.push(sub(a, b));
                }
                Op::Add => {
                    let b = pop(&mut s);
                    let a = pop(&mut s);
                    s.stack.push(match (&a, &b) {
                        (T::C(x, _), T::C(y, _)) => c32((x + y) as u32),
                        _ => add(a, b),
                    });
                }
                Op::Load(off) => {
                    let T::C(addr, _) = pop(&mut s) else {
                        panic!("symbolic load address")
                    };
                    let v = s
                        .mem
                        .get(&(addr as u32 + off))
                        .cloned()
                        .expect("load of unset cell");
                    s.stack.push(v);
                }
                Op::Store => {
                    let v = pop(&mut s);
                    let T::C(addr, _) = pop(&mut s) else {
                        panic!("symbolic store address")
                    };
                    s.mem.insert(addr as u32, v);
                }
                Op::Block | Op::Loop => s.ctl.push(Frame {
                    is_loop: op == Op::Loop,
                    start: s.ip,
                    end: end_of[s.ip],
                }),
                Op::If => {
                    let c = truthy(pop(&mut s));
                    let frame = Frame {
                        is_loop: false,
                        start: s.ip,
                        end: end_of[s.ip],
                    };
                    let mut taken = s.clone();
                    taken.ctl.push(frame);
                    taken.ip += 1;
                    let mut skipped = s;
                    match else_of[frame.start] {
                        Some(e) => {
                            skipped.ctl.push(frame);
                            skipped.ip = e + 1;
                        }
                        None => skipped.ip = frame.end + 1,
                    }
                    fork(&mut work, taken, skipped, c);
                    continue 'paths;
                }
                Op::Else => {
                    let f = s.ctl.pop().unwrap();
                    s.ip = f.end + 1;
                    continue;
                }
                Op::End => {
                    if s.ctl.pop().is_none() {
                        let v = pop(&mut s);
                        s.trace.push(Ev::Return(v));
                        done.push(Run {
                            cond: s.cond,
                            trace: s.trace,
                        });
                        continue 'paths;
                    }
                }
                Op::Br(d) => {
                    branch(&mut s, d);
                    continue;
                }
                Op::BrIf(d) => {
                    let c = truthy(pop(&mut s));
                    let mut taken = s.clone();
                    branch(&mut taken, d);
                    let mut fall = s;
                    fall.ip += 1;
                    fork(&mut work, taken, fall, c);
                    continue 'paths;
                }
                Op::Return => {
                    let v = pop(&mut s);
                    s.trace.push(Ev::Return(v));
                    done.push(Run {
                        cond: s.cond,
                        trace: s.trace,
                    });
                    continue 'paths;
                }
                Op::Host(name) => match name {
                    "pipe" => s.stack.push(v32("pipe_handle")),
                    "call" | "expand-list" => {
                        let k = site_of[s.ip];
                        let it = s.iters.entry(k).or_insert(0);
                        if *it >= BOUND {
                            s.trace.push(Ev::Bound);
                            done.push(Run {
                                cond: s.cond,
                                trace: s.trace,
                            });
                            continue 'paths;
                        }
                        let i = *it;
                        *it += 1;
                        if name == "call" {
                            s.stack.truncate(s.stack.len() - 8);
                            s.trace.push(Ev::Call(k));
                            s.stack.push(call_var(k, i));
                        } else {
                            let T::C(ret, _) = pop(&mut s) else {
                                panic!("symbolic retptr")
                            };
                            s.stack.truncate(s.stack.len() - 2);
                            s.mem.insert(ret as u32, v32("list_handle"));
                            s.mem.insert(ret as u32 + 4, count_var(k, i));
                            s.trace.push(Ev::Expand(k));
                        }
                    }
                    "set-status" => {
                        let v = pop(&mut s);
                        s.trace.push(Ev::SetStatus(v));
                    }
                    "set-var" | "bind-item" => {
                        s.stack.truncate(s.stack.len() - 4);
                    }
                    "scope-push" => s.trace.push(Ev::ScopePush),
                    "scope-pop" => s.trace.push(Ev::ScopePop),
                    // `site(id)` only names the next call's command for run-error
                    // reports: no status, no trace event.
                    "site" => {
                        pop(&mut s);
                    }
                    other => panic!("{other}"),
                },
            }
            s.ip += 1;
        }
    }
    done
}

fn branch(s: &mut WState, d: u32) {
    let at = s.ctl.len() - 1 - d as usize;
    let f = s.ctl[at];
    if f.is_loop {
        s.ctl.truncate(at + 1);
        s.ip = f.start + 1;
    } else {
        s.ctl.truncate(at);
        s.ip = f.end + 1;
    }
}

fn fork(work: &mut Vec<WState>, mut yes: WState, mut no: WState, c: P) {
    match c {
        P::Const(true) => work.push(yes),
        P::Const(false) => work.push(no),
        c => {
            no.cond.push(not(c.clone()));
            yes.cond.push(c);
            work.push(no);
            work.push(yes);
        }
    }
}

// ---- specification side: POSIX semantics over the plan ------------------------

/// Static host-call sites in the order the plan's commands occur, captures
/// before the command that uses them (POSIX: command words, redirections,
/// assignments), loop items before the loop.
fn number_sites(plan: &Plan) -> BTreeMap<usize, u32> {
    fn list(l: &[PAndOr], m: &mut BTreeMap<usize, u32>) {
        for ao in l {
            pipe(&ao.first, m);
            for (_, p) in &ao.rest {
                pipe(p, m);
            }
        }
    }
    fn pipe(p: &PPipeline, m: &mut BTreeMap<usize, u32>) {
        for s in &p.stages {
            match s {
                PStage::Call(c) => {
                    for w in capture_words(c) {
                        captures_of(w, m);
                    }
                    if c.tool.is_some() {
                        let k = m.len() as u32;
                        m.insert(c as *const _ as usize, k);
                    }
                }
                PStage::For { items, body, .. } => {
                    items.iter().for_each(|w| captures_of(w, m));
                    let k = m.len() as u32;
                    m.insert(s as *const _ as usize, k);
                    list(body, m);
                }
                PStage::While {
                    condition, body, ..
                } => {
                    list(condition, m);
                    list(body, m);
                }
            }
        }
    }
    fn captures_of(w: &PWord, m: &mut BTreeMap<usize, u32>) {
        for p in &w.parts {
            if let PPart::Capture { body, .. } = p {
                list(body, m);
            }
        }
    }
    let mut m = BTreeMap::new();
    list(&plan.body, &mut m);
    m
}

/// The words of a command whose `$(...)` parts run before it, in POSIX
/// expansion order.
fn capture_words(c: &kapsel_plan::Call) -> Vec<&PWord> {
    if c.tool.is_none() {
        return c.assignments.iter().map(|(_, w)| w).collect();
    }
    let mut v: Vec<&PWord> = c.argv.iter().collect();
    for r in &c.redirects {
        if let PRedirectTarget::Word { word } | PRedirectTarget::Heredoc { body: word } = &r.target
        {
            v.push(word);
        }
    }
    v.extend(c.assignments.iter().map(|(_, w)| w));
    v
}

#[derive(Clone)]
struct SState {
    cond: Vec<P>,
    trace: Vec<Ev>,
    /// `$?` as the shell tracks it.
    status: T,
    iters: BTreeMap<u32, u32>,
    /// Subshell scopes open.
    depth: u32,
    /// `exit n` unwinding to the innermost scope.
    exiting: Option<T>,
    /// The plan finished (top-level `exit`).
    finished: bool,
    bounded: bool,
}

impl SState {
    fn live(&self) -> bool {
        self.exiting.is_none() && !self.finished && !self.bounded
    }
}

struct Spec<'p> {
    sites: BTreeMap<usize, u32>,
    _plan: &'p Plan,
}

impl Spec<'_> {
    fn split(s: SState, c: P) -> (Option<SState>, Option<SState>) {
        match c {
            P::Const(true) => (Some(s), None),
            P::Const(false) => (None, Some(s)),
            c => {
                let mut y = s.clone();
                let mut n = s;
                n.cond.push(not(c.clone()));
                y.cond.push(c);
                (Some(y), Some(n))
            }
        }
    }

    fn list(&self, s: SState, l: &[PAndOr]) -> Vec<SState> {
        let mut states = vec![s];
        for ao in l {
            states = states
                .into_iter()
                .flat_map(|s| self.and_or(s, ao))
                .collect();
        }
        states
    }

    /// `a && b`: b runs when `$?` is 0; `a || b`: when it is not.
    fn and_or(&self, s: SState, ao: &PAndOr) -> Vec<SState> {
        let mut states = self.pipeline(s, &ao.first);
        for (op, p) in &ao.rest {
            let mut next = Vec::new();
            for s in states {
                if !s.live() {
                    next.push(s);
                    continue;
                }
                let zero = eq(s.status.clone(), c32(0));
                let run = if matches!(op, AndOrOp::And) {
                    zero
                } else {
                    not(zero)
                };
                let (y, n) = Self::split(s, run);
                if let Some(y) = y {
                    next.extend(self.pipeline(y, p));
                }
                next.extend(n);
            }
            states = next;
        }
        states
    }

    /// Stages in order, each a subshell when there are two or more; `$?` is
    /// the last stage's, inverted by `!`, and published with set-status.
    fn pipeline(&self, s: SState, p: &PPipeline) -> Vec<SState> {
        if !s.live() {
            return vec![s];
        }
        let mut states = vec![s];
        for st in &p.stages {
            states = states
                .into_iter()
                .flat_map(|s| {
                    if p.stages.len() >= 2 {
                        self.subshell(s, |s| self.stage(s, st))
                    } else {
                        self.stage(s, st)
                    }
                })
                .collect();
        }
        for s in &mut states {
            if s.live() {
                if p.negated {
                    s.status = b2t(eq(s.status.clone(), c32(0)));
                }
                s.trace.push(Ev::SetStatus(s.status.clone()));
            }
        }
        states
    }

    /// A subshell: its state changes are discarded, `exit n` inside ends it
    /// with status n.
    fn subshell(&self, mut s: SState, body: impl Fn(SState) -> Vec<SState>) -> Vec<SState> {
        if !s.live() {
            return vec![s];
        }
        s.trace.push(Ev::ScopePush);
        s.depth += 1;
        body(s)
            .into_iter()
            .map(|mut o| {
                if o.bounded {
                    return o;
                }
                o.depth -= 1;
                if let Some(n) = o.exiting.take() {
                    o.status = n;
                }
                o.trace.push(Ev::ScopePop);
                o
            })
            .collect()
    }

    fn captures(&self, s: SState, words: &[&PWord]) -> Vec<SState> {
        let mut states = vec![s];
        for w in words {
            for p in &w.parts {
                if let PPart::Capture { body, .. } = p {
                    states = states
                        .into_iter()
                        .flat_map(|s| self.subshell(s, |s| self.list(s, body)))
                        .collect();
                }
            }
        }
        states
    }

    fn next_iter(s: &mut SState, k: u32) -> Option<u32> {
        let it = s.iters.entry(k).or_insert(0);
        if *it >= BOUND {
            s.bounded = true;
            s.trace.push(Ev::Bound);
            return None;
        }
        *it += 1;
        Some(*it - 1)
    }

    fn stage(&self, s: SState, st: &PStage) -> Vec<SState> {
        if !s.live() {
            return vec![s];
        }
        match st {
            PStage::Call(c) if c.tool.is_some() => {
                let k = self.sites[&(c as *const _ as usize)];
                let words = capture_words(c);
                let mut out = Vec::new();
                for mut s in self.captures(s, &words) {
                    if !s.live() {
                        out.push(s);
                        continue;
                    }
                    let Some(i) = Self::next_iter(&mut s, k) else {
                        out.push(s);
                        continue;
                    };
                    s.trace.push(Ev::Call(k));
                    let r = call_var(k, i);
                    // 256 + n: the built-in was `exit n`.
                    let (ex, ok) = Self::split(s, not(ult(r.clone(), c32(256))));
                    if let Some(mut e) = ex {
                        let n = sub(r.clone(), c32(256));
                        if e.depth == 0 {
                            e.trace.push(Ev::Return(n));
                            e.finished = true;
                        } else {
                            e.exiting = Some(n);
                        }
                        out.push(e);
                    }
                    if let Some(mut o) = ok {
                        o.status = r;
                        out.push(o);
                    }
                }
                out
            }
            PStage::Call(c) => {
                // `X=v`: status 0, or the last substitution's status.
                let mut s = s;
                s.status = c32(0);
                self.captures(s, &capture_words(c))
            }
            PStage::For { items, body, .. } => {
                let k = self.sites[&(st as *const _ as usize)];
                let words: Vec<&PWord> = items.iter().collect();
                let mut out = Vec::new();
                for mut s in self.captures(s, &words) {
                    if !s.live() {
                        out.push(s);
                        continue;
                    }
                    let Some(i) = Self::next_iter(&mut s, k) else {
                        out.push(s);
                        continue;
                    };
                    s.trace.push(Ev::Expand(k));
                    let n = count_var(k, i);
                    out.extend(self.for_iter(s, &n, 0, c32(0), body));
                }
                out
            }
            PStage::While {
                until,
                condition,
                body,
            } => self.while_iter(s, *until, condition, body, c32(0)),
        }
    }

    /// Iteration `i` of a for loop over `n` items; `last` is the last body
    /// status (0 when the body never ran).
    fn for_iter(&self, s: SState, n: &T, i: u32, last: T, body: &[PAndOr]) -> Vec<SState> {
        let (more, stop) = Self::split(s, ult(c32(i), n.clone()));
        let mut out = Vec::new();
        if let Some(mut st) = stop {
            st.status = last;
            out.push(st);
        }
        if let Some(m) = more {
            if i > BOUND * 4 {
                let mut m = m;
                m.bounded = true;
                m.trace.push(Ev::Bound);
                out.push(m);
                return out;
            }
            for b in self.list(m, body) {
                if b.live() {
                    let st = b.status.clone();
                    out.extend(self.for_iter(b, n, i + 1, st, body));
                } else {
                    out.push(b);
                }
            }
        }
        out
    }

    /// `while`: the body runs while the condition's status is 0 (`until`:
    /// while it is not). Status: the last body's, 0 when it never ran.
    fn while_iter(
        &self,
        s: SState,
        until: bool,
        cond: &[PAndOr],
        body: &[PAndOr],
        last: T,
    ) -> Vec<SState> {
        let mut out = Vec::new();
        for c in self.list(s, cond) {
            if !c.live() {
                out.push(c);
                continue;
            }
            let zero = eq(c.status.clone(), c32(0));
            let go = if until { not(zero) } else { zero };
            let (more, stop) = Self::split(c, go);
            if let Some(mut st) = stop {
                st.status = last.clone();
                out.push(st);
            }
            if let Some(m) = more {
                for b in self.list(m, body) {
                    if b.live() {
                        let st = b.status.clone();
                        out.extend(self.while_iter(b, until, cond, body, st));
                    } else {
                        out.push(b);
                    }
                }
            }
        }
        out
    }
}

fn exec_spec(plan: &Plan) -> Vec<(Run, bool)> {
    let spec = Spec {
        sites: number_sites(plan),
        _plan: plan,
    };
    let start = SState {
        cond: vec![],
        trace: vec![],
        status: c32(0),
        iters: BTreeMap::new(),
        depth: 0,
        exiting: None,
        finished: false,
        bounded: false,
    };
    spec.list(start, &plan.body)
        .into_iter()
        .map(|mut s| {
            assert!(s.exiting.is_none(), "exit escaped every scope");
            if !s.finished && !s.bounded {
                s.trace.push(Ev::Return(s.status.clone()));
            }
            (
                Run {
                    cond: s.cond,
                    trace: s.trace,
                },
                s.bounded,
            )
        })
        .collect()
}

/// `traces differ` as a condition: true when the shapes differ, else the
/// disjunction of the value differences.
fn differ(a: &[Ev], b: &[Ev]) -> P {
    if a.len() != b.len() {
        return P::Const(true);
    }
    let mut diffs = Vec::new();
    for (x, y) in a.iter().zip(b) {
        match (x, y) {
            (Ev::SetStatus(u), Ev::SetStatus(v)) | (Ev::Return(u), Ev::Return(v)) => {
                // Kept even when the terms are identical: the solver, not the
                // generator, decides every value comparison.
                diffs.push(P::Not(P::Eq(u.clone(), v.clone()).into()));
            }
            (x, y) if x == y => {}
            _ => return P::Const(true),
        }
    }
    or(diffs)
}

/// The status-logic obligation for one plan: some implementation path and
/// some in-bound specification path, both possible, with different traces.
fn status_obligation(plan: &Plan, ops: &[Op]) -> (P, usize, usize) {
    let imp = exec_wasm(ops);
    let spec = exec_spec(plan);
    let mut disj = Vec::new();
    for i in &imp {
        for (s, bounded) in &spec {
            if *bounded {
                continue;
            }
            let d = differ(&i.trace, &s.trace);
            if d == P::Const(false) {
                continue;
            }
            let mut c = i.cond.clone();
            c.extend(s.cond.iter().cloned());
            c.push(d);
            disj.push(and(c));
        }
    }
    (or(disj), imp.len(), spec.len())
}

// ---- the obligations -------------------------------------------------------------

/// Rules proven, each with the fixture whose emitted code the proof is about.
const PROVEN: &[(&str, &str)] = &[
    ("KC-R01", "seq"),
    ("KC-R02", "and"),
    ("KC-R03", "or"),
    ("KC-R04", "single"),
    ("KC-R05", "pipe3"),
    ("KC-R06", "not"),
    ("KC-R07", "redirects"),
    ("KC-R08", "assign_capture"),
    ("KC-R09", "for"),
    ("KC-R10", "while"),
    ("KC-R11", "until"),
    ("KC-R12", "capture"),
    ("KC-R19", "stage_exit"),
    ("KC-R20", "sub_exit"),
    ("KC-R21", "exit"),
];

/// Rules with no status arithmetic: they lay out argument data only.
const NO_ARITHMETIC: &[(&str, &str)] = &[
    (
        "KC-R13",
        "redirect-path: lays out a redirect record, computes and tests no status",
    ),
    (
        "KC-R14",
        "redirect-dup: lays out a redirect record, computes and tests no status",
    ),
    (
        "KC-R15",
        "heredoc: lays out a redirect record, computes and tests no status",
    ),
    (
        "KC-R16",
        "part-lit: lays out a part variant, computes and tests no status",
    ),
    (
        "KC-R17",
        "part-var: lays out a part variant, computes and tests no status",
    ),
    (
        "KC-R18",
        "part-glob: lays out a part variant, computes and tests no status",
    ),
    (
        "KC-R22",
        "call-site: passes a compile-time constant to `site`, a host call with no result; \
         the symbolic executor models it as a no-op, so every status proof covers plans \
         that contain it",
    ),
];

struct Obligation {
    name: String,
    claim: String,
    text: String,
}

fn fixture(name: &str) -> Plan {
    let path = root().join(format!("testdata/kapsel/compile/{name}.plan.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn proofs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("proofs")
}

fn status_obligations() -> Vec<Obligation> {
    PROVEN
        .iter()
        .map(|(id, fx)| {
            let rule = RULES.iter().find(|r| r.id == *id).unwrap();
            let plan = fixture(fx);
            let ops = decode_run(&compile_core(&plan).unwrap());
            let (p, ni, ns) = status_obligation(&plan, &ops);
            let claim = format!(
                "{id} {}: the status logic of the wasm emitted for fixture `{fx}` equals the \
                 shell semantics",
                rule.name
            );
            let header = vec![
                format!(
                    "kapsel-compile status-logic obligation {id} {} ({})",
                    rule.name, rule.shape
                ),
                format!("fixture: testdata/kapsel/compile/{fx}.plan.json"),
                format!(
                    "implementation: {ni} paths through the wasm `run` that compile_core emits; \
                     specification: {ns} paths of the POSIX semantics"
                ),
                format!(
                    "inputs: cK_I = result of the I-th run of host call site K, nK_I = expand-list \
                     count; each site runs at most {BOUND} times"
                ),
                "unsat: no host results make the two traces (call, expand-list, set-status, \
                 scope-push, scope-pop, return) differ"
                    .into(),
            ];
            Obligation {
                name: format!("{id}-{}", rule.name),
                claim,
                text: script(&header, &p),
            }
        })
        .collect()
}

/// The allocator's rounding as a term: `(len + a - 1) & !(a - 1)`.
fn align_up_t(len: T, a: u64) -> T {
    T::BvAnd(add(len, c64(a - 1)).into(), c64(!(a - 1)).into())
}

/// `pages_for(total)` as a term: `(total + PAGE - 1) >> PAGE_SHIFT`, at least 1.
fn pages_t(total: T) -> T {
    let p = T::Lshr(
        add(total, c64(layout::PAGE - 1)).into(),
        c64(layout::PAGE_SHIFT as u64).into(),
    );
    T::Ite(eq(p.clone(), c64(0)).into(), c64(1).into(), p.into())
}

/// `c * x` for a constant `c`, written as the sum of `x << k` over the set
/// bits of `c` (the same number; it keeps the bit-blasted circuit small).
fn mul_const(c: u64, x: &T) -> T {
    let mut acc: Option<T> = None;
    for k in 0..64 {
        if c >> k & 1 == 1 {
            let term = if k == 0 {
                x.clone()
            } else {
                T::Shl(x.clone().into(), c64(k).into())
            };
            acc = Some(match acc {
                None => term,
                Some(a) => add(a, term),
            });
        }
    }
    acc.unwrap_or(c64(0))
}

/// `x` is a multiple of the power of two `a`.
fn aligned(x: T, a: u64) -> P {
    eq(T::BvAnd(x.into(), c64(a - 1).into()), c64(0))
}

/// The header line stating a layout obligation's width and why no term wraps
/// at it (nothing wraps in the allocator's u64 arithmetic either).
fn width_note(largest: &str) -> String {
    format!(
        "arithmetic at {} bits: every input is below 2^32 and every intermediate below {largest}, \
         so nothing wraps",
        lw()
    )
}

fn layout_obligations() -> Vec<Obligation> {
    let base = layout::DATA_BASE as u64;
    let max_end = layout::MAX_END;
    let mut out = Vec::new();
    set_lw(34);
    // L1: rounding meets the alignment, never moves backwards, and moves by
    // less than the alignment.
    for a in [1u64, 4] {
        let len = v64("len");
        let start = align_up_t(len.clone(), a);
        let claim = and(vec![
            aligned(start.clone(), a),
            ule(len.clone(), start.clone()),
            ult(sub(start, len.clone()), c64(a)),
        ]);
        let p = and(vec![ule(len, c64(max_end)), not(claim)]);
        out.push(Obligation {
            name: format!("layout-align-{a}"),
            claim: format!("align_up(len, {a}) is a multiple of {a}, >= len, < len + {a}"),
            text: script(
                &[
                    format!("kapsel-compile layout obligation: allocator rounding to {a}"),
                    "start = (len + a - 1) & !(a - 1), the formula of layout::align_up".into(),
                    format!("for every len <= {max_end}"),
                    width_note("2^33 (len + 3)"),
                ],
                &p,
            ),
        });
    }
    // L2a: an accepted allocation at `start` (aligned per L1) ends inside the
    // segment and below 2^32, its address keeps the alignment, and the
    // return area sits below the data segment.
    {
        let (start, size, total) = (v64("start"), v64("size"), v64("total"));
        let addr = add(c64(base), start.clone());
        let end = add(start.clone(), size.clone());
        let accepted = and(vec![
            ule(start.clone(), c64(max_end)),
            ule(size.clone(), c64(u32::MAX as u64)),
            ule(total.clone(), c64(max_end)),
            ule(add(c64(base), end.clone()), c64(max_end)),
            ule(end, total.clone()),
        ]);
        let claim = and(vec![
            ule(add(addr.clone(), size.clone()), add(c64(base), total)),
            ule(add(addr.clone(), size), c64(1 << 32)),
            or(vec![not(aligned(start, 4)), aligned(addr, 4)]),
            aligned(c64(layout::RET_AREA as u64), 4),
            ule(
                c64((layout::RET_AREA + layout::RET_AREA_SIZE) as u64),
                c64(base),
            ),
        ]);
        out.push(Obligation {
            name: "layout-bounds".into(),
            claim: "an accepted allocation (ptr, len) ends inside the data segment, below 2^32".into(),
            text: script(
                &[
                    "kapsel-compile layout obligation: allocations stay inside the data segment".into(),
                    format!(
                        "ptr = {base} + start (start = align_up(len, a), aligned by layout-align-*); \
                         accepted when {base} + start + size <= {max_end} (the KC-003 check)"
                    ),
                    format!("total = final segment length >= start + size; segment = [{base}, {base} + total)"),
                    "with layout-pages: ptr + size <= DATA_BASE + total <= memory size".into(),
                    width_note("2^34 (DATA_BASE + start + size)"),
                ],
                &and(vec![accepted, not(claim)]),
            ),
        });
    }
    // L2b: the declared memory covers the whole segment and fits wasm32.
    {
        let x = v64("x");
        let mem = T::Shl(
            pages_t(x.clone()).into(),
            c64(layout::PAGE_SHIFT as u64).into(),
        );
        let claim = and(vec![
            ule(x.clone(), mem.clone()),
            ule(mem.clone(), c64(1 << 32)),
            ule(c64(layout::PAGE), mem),
        ]);
        out.push(Obligation {
            name: "layout-pages".into(),
            claim: "pages_for(DATA_BASE + total) pages cover the segment and fit 2^32 bytes".into(),
            text: script(
                &[
                    "kapsel-compile layout obligation: declared memory covers the data segment"
                        .into(),
                    format!(
                        "memory = pages_for(x) << {}, pages_for(x) = max(1, (x + {}) >> {}), x = \
                         DATA_BASE + total <= {max_end}",
                        layout::PAGE_SHIFT,
                        layout::PAGE - 1,
                        layout::PAGE_SHIFT
                    ),
                    width_note("2^33 (x + PAGE - 1, the memory size)"),
                ],
                &and(vec![ule(x, c64(max_end)), not(claim)]),
            ),
        });
    }
    let max_list = layout::MAX_LIST as u64;
    let strides: BTreeSet<u64> = layout::RECORDS.iter().map(|r| r.size as u64).collect();
    // L3a: per stride s, for i < n <= MAX_LIST: s*i is 4-aligned and
    // s*i + s <= s*n (the element offset `stride * i` the lowering computes,
    // in u32, cannot wrap below the limit).
    set_lw(22);
    for &s in &strides {
        let (n, i) = (v64("n"), v64("i"));
        let (x, y) = (mul_const(s, &i), mul_const(s, &n));
        let p = and(vec![
            ule(n.clone(), c64(max_list)),
            ult(i, n),
            not(and(vec![aligned(x.clone(), 4), ule(add(x, c64(s)), y)])),
        ]);
        out.push(Obligation {
            name: format!("layout-offset-{s}"),
            claim: format!("for i < n <= {max_list}: {s}*i is 4-aligned and {s}*i + {s} <= {s}*n"),
            text: script(
                &[
                    format!(
                        "kapsel-compile layout obligation: element offsets of a stride-{s} list"
                    ),
                    format!(
                        "offset = {s} * i (layout lowering: base + STRIDE * i); for every i < n <= \
                         MAX_LIST = {max_list}"
                    ),
                    format!("{s} * x is written as a sum of shifts of x over the set bits of {s}"),
                    width_note(&format!("2^21 ({s} * {max_list} + {s})")),
                ],
                &p,
            ),
        });
    }
    // L3b: per stride s, an element at offset x (4-aligned, x + s <= y = s*n
    // per layout-offset-s) of a list at a 4-aligned base that fits below
    // 2^32 starts 4-aligned and ends inside the list.
    set_lw(34);
    for &s in &strides {
        let (b, x, y) = (v64("base"), v64("offset"), v64("size"));
        let elem = add(b.clone(), x.clone());
        let list_end = add(b.clone(), y.clone());
        let p = and(vec![
            aligned(b.clone(), 4),
            ule(b, c64(1 << 32)),
            aligned(x.clone(), 4),
            ule(add(x, c64(s)), y.clone()),
            ule(y, c64(s * max_list)),
            ule(list_end.clone(), c64(1 << 32)),
            not(and(vec![
                aligned(elem.clone(), 4),
                ule(add(elem, c64(s)), list_end),
            ])),
        ]);
        out.push(Obligation {
            name: format!("layout-stride-{s}"),
            claim: format!("element of a stride-{s} list starts 4-aligned and ends inside the list"),
            text: script(
                &[
                    format!("kapsel-compile layout obligation: list elements of stride {s}"),
                    "elem = base + offset, list = [base, base + size); base 4-aligned \
                     (layout-bounds), the list below 2^32"
                        .into(),
                    format!(
                        "hypotheses from layout-offset-{s}: offset 4-aligned, offset + {s} <= size, \
                         size = {s} * n <= {s} * MAX_LIST"
                    ),
                    width_note("2^34 (base + size + s)"),
                ],
                &p,
            ),
        });
    }
    // L3c: per record, every field of an element that starts 4-aligned and
    // ends inside the list (layout-stride-*) is aligned and inside the list.
    for r in layout::RECORDS {
        let (e, end) = (v64("elem"), v64("list_end"));
        let mut claims = Vec::new();
        for f in r.fields {
            let at = add(e.clone(), c64(f.offset as u64));
            claims.push(aligned(at.clone(), f.align as u64));
            claims.push(ule(add(at, c64(f.size as u64)), end.clone()));
        }
        let p = and(vec![
            aligned(e.clone(), r.align as u64),
            ule(e.clone(), c64(1 << 32)),
            ule(end.clone(), c64(1 << 32)),
            ule(add(e, c64(r.size as u64)), end),
            not(and(claims)),
        ]);
        out.push(Obligation {
            name: format!("layout-{}", r.name),
            claim: format!(
                "every field of a `{}` element is aligned and inside the list",
                r.name
            ),
            text: script(
                &[
                    format!(
                        "kapsel-compile layout obligation: `{}` fields (stride {}, align {})",
                        r.name, r.size, r.align
                    ),
                    format!(
                        "fields: {}",
                        r.fields
                            .iter()
                            .map(|f| format!("{}@{}:{}/{}", f.name, f.offset, f.size, f.align))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    format!(
                        "for every element start aligned to {} with start + {} <= list end <= 2^32 \
                         (layout-stride-{})",
                        r.align, r.size, r.size
                    ),
                    width_note("2^33 (elem + offset + size)"),
                ],
                &p,
            ),
        });
    }
    out
}

// ---- solving and certificates -------------------------------------------------

enum Verdict {
    Unsat { bundle: String, solve: Duration },
    Sat(Vec<(String, u128)>),
    Other(String),
}

fn solve(text: &str, claim: &str) -> Verdict {
    use ordeal::query::TermCheckResult;
    let t = Instant::now();
    match ordeal::smtlib::solve_str_with_term(text) {
        Ok((Some(TermCheckResult::Unsat(cert)), _)) => {
            let solve = t.elapsed();
            let echo = ordeal::EchoCertificate::new(ordeal::EchoSubject::Unsat(cert))
                .expect("the trusted printer echoes the obligation");
            let attests = ordeal::cert_bundle::Attests {
                kind: "qf_bv_validity".into(),
                claim: claim.into(),
                standards: vec![],
            };
            Verdict::Unsat {
                bundle: echo.to_cert_v5(&attests),
                solve,
            }
        }
        Ok((Some(TermCheckResult::Sat(_)), _)) => match ordeal::smtlib::solve_str(text) {
            Ok(ordeal::smtlib::Outcome {
                result: Some(ordeal::CheckResult::Sat(m)),
                ..
            }) => Verdict::Sat(m.assignments),
            other => Verdict::Other(format!("sat, but no model: {other:?}")),
        },
        other => Verdict::Other(format!("{other:?}")),
    }
}

/// Re-checks a committed bundle with ordeal's trusted checker and returns
/// the echo text the recheck re-derived.
fn recheck(bundle: &str) -> Result<(Duration, String), String> {
    let t = Instant::now();
    let b = ordeal::EchoCertificate::from_cert_v5(bundle).map_err(|e| format!("{e:?}"))?;
    b.recheck().map_err(|e| format!("{e:?}"))?;
    Ok((t.elapsed(), b.certificate.echo.text.clone()))
}

/// The assertions of an SMT-LIB script as ordeal reads them.
fn read_assertions(text: &str) -> String {
    match ordeal::smtlib::assertions_at_check_sat(text) {
        Ok((Some(a), _)) => format!("{a:?}"),
        other => panic!("ordeal does not read the script: {other:?}"),
    }
}

/// Where certificates are written: `$KAPSEL_PROOF_OUT`, else a per-process
/// directory under the system temp dir. Never `proofs/`: certificates are
/// not committed (CI uploads them, a release attaches them as a signed asset).
fn cert_dir() -> PathBuf {
    let dir = match std::env::var_os("KAPSEL_PROOF_OUT") {
        Some(d) => PathBuf::from(d),
        None => std::env::temp_dir().join(format!("kapsel-compile-proofs-{}", std::process::id())),
    };
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// For each obligation: the committed `.smt2` equals the one generated from
/// the current lowering (else a stale proof, reported with ordeal's verdict
/// on the current obligation); ordeal solves it (`unsat`) and the
/// certificate is written to [`cert_dir`]; the written certificate is read
/// back, re-checked with ordeal's trusted checker, and its echo (re-derived
/// by the trusted printer during the recheck) reads back to the committed
/// `.smt2`'s assertions. `KAPSEL_BLESS=1` rewrites the `.smt2` files.
fn check_all(obligations: Vec<Obligation>) {
    let bless = std::env::var("KAPSEL_BLESS").is_ok_and(|v| v == "1");
    std::fs::create_dir_all(proofs_dir()).unwrap();
    let out = cert_dir();
    let mut failures = Vec::new();
    for o in obligations {
        let smt_path = proofs_dir().join(format!("{}.smt2", o.name));
        let cert_path = out.join(format!("{}.cert.json", o.name));
        let verdict = solve(&o.text, &o.claim);
        if bless && matches!(verdict, Verdict::Unsat { .. }) {
            std::fs::write(&smt_path, &o.text).unwrap();
        }
        let committed = std::fs::read_to_string(&smt_path).unwrap_or_default();
        let (bundle, solve_time) = match verdict {
            Verdict::Unsat { bundle, solve } if committed == o.text => (bundle, solve),
            v => {
                let now = match v {
                    Verdict::Unsat { .. } => "unsat (re-bless with KAPSEL_BLESS=1)".to_string(),
                    Verdict::Sat(m) => format!("SAT, ordeal counterexample {m:?}"),
                    Verdict::Other(e) => format!("no verdict: {e}"),
                };
                let what = if committed == o.text {
                    "the obligation is not proven".to_string()
                } else {
                    format!(
                        "stale proof, {} differs from the current lowering",
                        smt_path.display()
                    )
                };
                failures.push(format!(
                    "{}: {what}; the current obligation is {now}",
                    o.name
                ));
                continue;
            }
        };
        std::fs::write(&cert_path, &bundle).unwrap();
        let cert = std::fs::read_to_string(&cert_path).unwrap();
        match recheck(&cert) {
            Ok((rt, echo)) => {
                if read_assertions(&echo) != read_assertions(&committed) {
                    failures.push(format!(
                        "{}: the certificate is about another query",
                        o.name
                    ));
                    continue;
                }
                eprintln!(
                    "proof {}: unsat, {} bytes smt2, {} bytes certificate, solve {solve_time:?}, \
                     recheck {rt:?}",
                    o.name,
                    committed.len(),
                    cert.len()
                );
            }
            Err(e) => failures.push(format!("{}: certificate does not re-check: {e}", o.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- tests ------------------------------------------------------------------------

#[test]
fn status_logic_proofs() {
    check_all(status_obligations());
}

#[test]
fn layout_proofs() {
    check_all(layout_obligations());
}

#[test]
fn every_rule_is_proven_or_listed() {
    for r in RULES {
        let proven = PROVEN.iter().any(|(id, _)| *id == r.id);
        let listed = NO_ARITHMETIC.iter().any(|(id, _)| *id == r.id);
        assert!(
            proven ^ listed,
            "{} ({}) must be proven or listed, exactly once",
            r.id,
            r.name
        );
    }
    assert_eq!(PROVEN.len() + NO_ARITHMETIC.len(), RULES.len());
}

#[test]
fn no_unknown_files_under_proofs() {
    let mut expected = BTreeSet::new();
    for o in status_obligations().into_iter().chain(layout_obligations()) {
        expected.insert(format!("{}.smt2", o.name));
    }
    let found: BTreeSet<String> = std::fs::read_dir(proofs_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".smt2"))
        .collect();
    assert_eq!(
        found, expected,
        "proofs/ holds queries no obligation produces, or misses some"
    );
}

#[test]
fn layout_formulas_compute_what_the_allocator_computes() {
    for len in [0u64, 1, 2, 3, 4, 5, 15, 16, 17, 4095, u32::MAX as u64 - 3] {
        for a in [1u64, 4] {
            let env = BTreeMap::from([("len", len)]);
            assert_eq!(
                eval(&align_up_t(v64("len"), a), &env),
                layout::align_up(len, a)
            );
        }
    }
    for total in [0u64, 1, 16, 65_535, 65_536, 65_537, 1 << 32] {
        let env = BTreeMap::from([("t", total)]);
        let p = match pages_t(v64("t")) {
            T::Ite(_, _, p) => eval(&p, &env).max(1),
            _ => unreachable!(),
        };
        assert_eq!(p, layout::pages_for(total));
    }
}

#[test]
fn layout_records_match_the_canonical_abi() {
    let mut resolve = wit_parser::Resolve::new();
    resolve
        .push_str("kapsel-shell.wit", kapsel_compile::SHELL_WIT)
        .unwrap();
    let mut sa = wit_parser::SizeAlign::default();
    sa.fill(&resolve).unwrap();
    let iface = resolve
        .interfaces
        .iter()
        .find(|(_, i)| i.name.as_deref() == Some("host"))
        .unwrap()
        .1;
    let ty = |n: &str| wit_parser::Type::Id(iface.types[n]);
    let size_align = |t: &wit_parser::Type| {
        (
            sa.size(t).size_wasm32() as u32,
            sa.align(t).align_wasm32() as u32,
        )
    };
    let find = |n: &str| layout::RECORDS.iter().find(|r| r.name == n).unwrap();
    for (rec, wit) in [
        ("part", ty("part")),
        ("word", ty("word")),
        ("redirect", ty("redirect")),
    ] {
        let r = find(rec);
        assert_eq!((r.size, r.align), size_align(&wit), "{rec}");
    }
    let tuple = wit_parser::Type::String;
    let (ss, _) = size_align(&tuple);
    let (ws, wa) = size_align(&ty("word"));
    let asg = find("assignment");
    assert_eq!(asg.size, (ss + ws).next_multiple_of(wa));
    // Field offsets from wit-parser's record and variant layout.
    let redirect = match &resolve.types[iface.types["redirect"]].kind {
        wit_parser::TypeDefKind::Record(r) => r,
        _ => panic!(),
    };
    let offs: Vec<u32> = sa
        .field_offsets(redirect.fields.iter().map(|f| &f.ty))
        .into_iter()
        .map(|(o, _)| o.size_wasm32() as u32)
        .collect();
    assert_eq!(
        offs,
        [
            layout::REDIR_FD,
            layout::REDIR_OP,
            layout::REDIR_TARGET_PTR,
            layout::REDIR_TARGET_FD
        ]
    );
    let part = match &resolve.types[iface.types["part"]].kind {
        wit_parser::TypeDefKind::Variant(v) => v,
        _ => panic!(),
    };
    let payload = sa.payload_offset(part.tag(), part.cases.iter().map(|c| c.ty.as_ref()));
    assert_eq!(payload.size_wasm32() as u32, layout::PART_STR_PTR);
    assert_eq!(layout::PART_CAP_PIPE, layout::PART_STR_PTR);
}

/// Negative control: negation lowered as `0 -> 0` (the `i32.eqz` replaced by
/// a constant 0). The obligation must be SAT, with ordeal's counterexample.
#[test]
fn negative_control_wrong_negation_is_refuted() {
    let plan = fixture("not");
    let mut ops = decode_run(&compile_core(&plan).unwrap());
    let i = ops
        .iter()
        .position(|o| *o == Op::Eqz)
        .expect("the negation's i32.eqz");
    // [.., local.get status, i32.eqz, local.set status] -> push 0 instead.
    ops[i - 1] = Op::Const(0);
    ops.remove(i);
    let (p, _, _) = status_obligation(&plan, &ops);
    let text = script(&["negative control".into()], &p);
    match solve(&text, "negative control") {
        Verdict::Sat(model) => {
            eprintln!("negative control: SAT, counterexample {model:?}");
            assert!(!model.is_empty());
        }
        Verdict::Unsat { .. } => panic!("a wrong negation was proven"),
        Verdict::Other(e) => panic!("no verdict: {e}"),
    }
}
