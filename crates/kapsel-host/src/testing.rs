//! Helpers that drive the host without a component. For tests; hidden from docs.

use crate::Session;
use crate::shell::{ROp, Shell, WPart, WRedirect, WWord};

/// A word part (mirrors `kapsel:shell/host` `part`).
pub enum TPart<'a> {
    Lit(&'a str, bool),
    Var(&'a str, bool),
    Glob(&'a str),
    Captured(u32, bool),
}

fn tword(w: &[TPart]) -> WWord {
    w.iter()
        .map(|p| match p {
            TPart::Lit(t, q) => WPart::Lit(t.to_string(), *q),
            TPart::Var(n, q) => WPart::Var(n.to_string(), *q),
            TPart::Glob(g) => WPart::Glob(g.to_string()),
            TPart::Captured(h, q) => WPart::Captured(*h, *q),
        })
        .collect()
}

impl Session {
    fn sh(&mut self) -> &mut Shell {
        self.shell_mut()
    }
    pub fn t_scope_push(&mut self) {
        self.sh().scope_push()
    }
    pub fn t_scope_pop(&mut self) {
        self.sh().scope_pop().expect("scope_pop")
    }
    pub fn t_pipe(&mut self) -> u32 {
        self.sh().new_pipe()
    }
    pub fn t_pipe_write(&mut self, h: u32, bytes: &[u8]) {
        self.sh().pipe_write(h, bytes)
    }
    pub fn t_pipe_read(&mut self, h: u32) -> Vec<u8> {
        self.sh().pipe_read(h)
    }
    pub fn t_expand(&mut self, words: &[Vec<TPart>]) -> Vec<String> {
        let sh = self.sh();
        let mut out = Vec::new();
        for w in words {
            out.extend(sh.expand_fields(&tword(w)).expect("expand"));
        }
        out
    }
    pub fn t_set_var(&mut self, name: &str, value: &[TPart]) {
        self.sh().set_var(name, &tword(value)).expect("set_var");
    }
    /// `redirs`: (fd, op, target, target fd); op is one of read, write,
    /// append, dup-out, dup-in, heredoc.
    pub fn t_call(&mut self, argv: &[Vec<TPart>], redirs: &[(u32, &str, Vec<TPart>, u32)], stdin: u32, stdout: u32) -> u32 {
        let reds: Vec<WRedirect> = redirs
            .iter()
            .map(|(fd, op, t, tfd)| WRedirect {
                fd: *fd,
                op: match *op {
                    "read" => ROp::Read,
                    "write" => ROp::Write,
                    "append" => ROp::Append,
                    "dup-out" => ROp::DupOut,
                    "dup-in" => ROp::DupIn,
                    _ => ROp::Heredoc,
                },
                target: tword(t),
                target_fd: *tfd,
            })
            .collect();
        let w: Vec<WWord> = argv.iter().map(|a| tword(a)).collect();
        let st = self.sh().call(&w, &[], &reds, stdin, stdout).expect("call");
        self.sh().status = if st >= 256 { (st - 256) as i32 } else { st as i32 };
        st
    }
}
