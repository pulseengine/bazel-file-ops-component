//! End to end: a real component (core module from WAT, encoded with
//! wit-component against wit/kapsel-shell.wit) runs on the host. The real
//! compiler (ws/compile) produces components of the same shape.

mod common;
use common::T;

use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

/// A part of a word in the generated plan.
enum P {
    Lit(&'static str),
    Var(&'static str),
}

fn wat_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("\\{x:02x}")).collect()
}

/// Builds a component whose `run` does `set-var` for each assignment, then one
/// `call` of the words, and returns the call's status.
fn plan(sets: &[(&'static str, &'static str)], words: &[Vec<P>]) -> Vec<u8> {
    let mut data = String::new();
    let mut at: u32 = 64;
    let mut put = |data: &mut String, bytes: &[u8], at: &mut u32| -> u32 {
        let p = *at;
        data.push_str(&format!("(data (i32.const {p}) \"{}\")\n", wat_bytes(bytes)));
        *at += (bytes.len() as u32 + 3) & !3;
        p
    };
    // one part = 16 bytes: tag u8 at 0, text ptr at 4, len at 8, quoted at 12
    let part = |tag: u8, ptr: u32, len: u32| -> Vec<u8> {
        let mut v = vec![tag, 0, 0, 0];
        v.extend(ptr.to_le_bytes());
        v.extend(len.to_le_bytes());
        v.extend([1, 0, 0, 0]);
        v
    };
    let mut make_word = |data: &mut String, at: &mut u32, parts: &[P]| -> (u32, u32) {
        let mut bytes = Vec::new();
        for p in parts {
            match p {
                P::Lit(s) => {
                    let sp = put(data, s.as_bytes(), at);
                    bytes.extend(part(0, sp, s.len() as u32));
                }
                P::Var(s) => {
                    let sp = put(data, s.as_bytes(), at);
                    bytes.extend(part(1, sp, s.len() as u32));
                }
            }
        }
        (put(data, &bytes, at), parts.len() as u32)
    };
    let mut body = String::new();
    for (name, val) in sets {
        let np = put(&mut data, name.as_bytes(), &mut at);
        let (wp, wl) = make_word(&mut data, &mut at, &[P::Lit(val)]);
        body.push_str(&format!("(call $set_var (i32.const {np}) (i32.const {}) (i32.const {wp}) (i32.const {wl}))\n", name.len()));
    }
    let mut table = Vec::new();
    for w in words {
        let (p, l) = make_word(&mut data, &mut at, w);
        table.extend(p.to_le_bytes());
        table.extend(l.to_le_bytes());
    }
    let tp = put(&mut data, &table, &mut at);
    body.push_str(&format!(
        "(call $call (i32.const {tp}) (i32.const {}) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))\n",
        words.len()
    ));
    let wat = format!(
        r#"(module
  (import "kapsel:shell/host@0.1.0" "call" (func $call (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "kapsel:shell/host@0.1.0" "set-var" (func $set_var (param i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 0)
  {data}
  (func (export "run") (result i32)
    {body}))"#
    );
    let mut core = wat::parse_str(&wat).expect("wat");
    let mut resolve = Resolve::default();
    let pkg = resolve.push_file(concat!(env!("CARGO_MANIFEST_DIR"), "/../../wit/kapsel-shell.wit")).unwrap();
    let world = resolve.select_world(&[pkg], Some("plan")).unwrap();
    embed_component_metadata(&mut core, &resolve, world, StringEncoding::UTF8, false).unwrap();
    ComponentEncoder::default().module(&core).unwrap().validate(true).encode().unwrap()
}

fn run(t: &mut T, bytes: &[u8]) -> u32 {
    t.out.clear();
    t.err.clear();
    t.s.set_io(Box::new(std::io::empty()), t.out.clone(), t.err.clone());
    t.s.run_component(bytes).unwrap()
}

#[test]
fn component_runs_a_builtin_through_the_host() {
    let mut t = T::new();
    let c = plan(&[], &[vec![P::Lit("echo")], vec![P::Lit("hi from wasm")]]);
    assert_eq!(run(&mut t, &c), 0);
    assert_eq!(t.out.string(), "hi from wasm\n");
    assert_eq!(t.s.status(), 0);
}

#[test]
fn component_status_comes_back_but_the_host_leaves_dollar_question_to_the_plan() {
    let mut t = T::new();
    let c = plan(&[], &[vec![P::Lit("false")]]);
    assert_eq!(run(&mut t, &c), 1);
    assert_eq!(t.s.status(), 0); // only set-status changes `$?`
}

#[test]
fn variables_persist_across_components() {
    let mut t = T::new();
    let c1 = plan(&[("X", "world")], &[vec![P::Lit(":")]]);
    assert_eq!(run(&mut t, &c1), 0);
    assert_eq!(t.s.var("X").as_deref(), Some("world"));
    let c2 = plan(&[], &[vec![P::Lit("echo")], vec![P::Lit("hello "), P::Var("X")]]);
    assert_eq!(run(&mut t, &c2), 0);
    assert_eq!(t.out.string(), "hello world\n");
}

#[test]
fn cd_persists_across_components() {
    let mut t = T::new();
    t.write("sub/f", "x\n");
    let c = plan(&[], &[vec![P::Lit("cd")], vec![P::Lit("sub")]]);
    assert_eq!(run(&mut t, &c), 0);
    let c = plan(&[], &[vec![P::Lit("cat")], vec![P::Lit("f")]]);
    assert_eq!(run(&mut t, &c), 0);
    assert_eq!(t.out.string(), "x\n");
}

#[test]
fn exit_returns_256_plus_n_to_the_component() {
    let mut t = T::new();
    // The component returns whatever `call` returned; a compiled plan would
    // stop and return n. The host's contract is the 256 + n value.
    let c = plan(&[], &[vec![P::Lit("exit")], vec![P::Lit("7")]]);
    assert_eq!(run(&mut t, &c), 263);
}

#[test]
fn a_component_importing_wasi_does_not_instantiate() {
    let mut t = T::new();
    let bytes = wat::parse_str(
        r#"(component
  (import "wasi:cli/environment@0.2.0" (instance (export "get-arguments" (func (result u32)))))
  (core module $m (func (export "run") (result i32) i32.const 0))
  (core instance $i (instantiate $m))
  (func (export "run") (result u32) (canon lift (core func $i "run"))))"#,
    )
    .unwrap();
    let e = t.s.run_component(&bytes).unwrap_err();
    assert!(e.0.contains("wasi:cli/environment"), "{}", e.0);
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let mut t = T::new();
    assert!(t.s.run_component(b"not a component").is_err());
}
