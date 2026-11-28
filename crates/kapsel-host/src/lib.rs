//! The kapsel host (REQ-KS-022, REQ-KS-025): runs compiled plan components on
//! wasmtime and implements the `kapsel:shell/host` imports, the built-ins and
//! the session state (variables, cwd, `$?`) that persists across lines.
//!
//! Handles: a pipe handle is the index + 1 into the run's pipe table; 0 is the
//! session's own stream. A `captured` part names the pipe its `$(...)` body
//! wrote to. An unknown handle traps the run. No WASI is linked: a component
//! may import only `kapsel:shell/host`.
//!
//! Owner: host workstream.

use std::io::{Read, Write};
use std::path::PathBuf;

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store};

mod builtins;
mod grants;
mod shell;

pub use shell::SharedBuf;
use shell::{ROp, Shell, Sink, WPart, WRedirect, WWord};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/kapsel-shell.wit",
        world: "plan",
        imports: { default: trappable },
    });
}
use bindings::kapsel::shell::host as wit;

/// Directories the session may touch. Fail closed: a path outside every grant
/// is refused with an error, and the CLI exits 78 if a grant is invalid.
#[derive(Debug, Clone, Default)]
pub struct GrantSet {
    pub read_write: Vec<PathBuf>,
    pub read_only: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct HostError(pub String);

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HostError {}

/// State that persists across compiled plans in one shell session.
pub struct Session {
    pub grants: GrantSet,
    engine: Engine,
    shell: Option<Shell>,
}

struct HostState {
    shell: Shell,
}

fn word(w: &[wit::Part]) -> WWord {
    w.iter()
        .map(|p| match p {
            wit::Part::Lit(l) => WPart::Lit(l.text.clone(), l.quoted),
            wit::Part::Var(v) => WPart::Var(v.name.clone(), v.quoted),
            wit::Part::Glob(g) => WPart::Glob(g.clone()),
            wit::Part::Captured(c) => WPart::Captured(c.pipe, c.quoted),
        })
        .collect()
}

fn trap(e: HostError) -> wasmtime::Error {
    wasmtime::Error::msg(e.0)
}

impl wit::Host for HostState {
    fn pipe(&mut self) -> wasmtime::Result<u32> {
        Ok(self.shell.new_pipe())
    }

    fn call(
        &mut self,
        argv: Vec<wit::Word>,
        assignments: Vec<(String, wit::Word)>,
        redirects: Vec<wit::Redirect>,
        stdin: u32,
        stdout: u32,
    ) -> wasmtime::Result<u32> {
        let argv: Vec<WWord> = argv.iter().map(|w| word(w)).collect();
        let assigns: Vec<(String, WWord)> = assignments
            .iter()
            .map(|(n, w)| (n.clone(), word(w)))
            .collect();
        let reds: Vec<WRedirect> = redirects
            .iter()
            .map(|r| WRedirect {
                fd: r.fd,
                op: match r.op {
                    wit::RedirectOp::Read => ROp::Read,
                    wit::RedirectOp::Write => ROp::Write,
                    wit::RedirectOp::Append => ROp::Append,
                    wit::RedirectOp::DupOut => ROp::DupOut,
                    wit::RedirectOp::DupIn => ROp::DupIn,
                    wit::RedirectOp::Heredoc => ROp::Heredoc,
                },
                target: word(&r.target),
                target_fd: r.target_fd,
            })
            .collect();
        self.shell
            .call(&argv, &assigns, &reds, stdin, stdout)
            .map_err(trap)
    }

    fn set_var(&mut self, name: String, value: wit::Word) -> wasmtime::Result<()> {
        self.shell.set_var(&name, &word(&value)).map_err(trap)
    }

    fn scope_push(&mut self) -> wasmtime::Result<()> {
        self.shell.scope_push();
        Ok(())
    }

    fn scope_pop(&mut self) -> wasmtime::Result<()> {
        self.shell.scope_pop().map_err(trap)
    }

    fn set_status(&mut self, status: u32) -> wasmtime::Result<()> {
        self.shell.status = status as i32;
        Ok(())
    }

    fn expand_list(&mut self, words: Vec<wit::Word>) -> wasmtime::Result<(u32, u32)> {
        let ws: Vec<WWord> = words.iter().map(|w| word(w)).collect();
        self.shell.expand_list(&ws).map_err(trap)
    }

    fn bind_item(&mut self, name: String, list: u32, index: u32) -> wasmtime::Result<()> {
        self.shell.bind_item(&name, list, index).map_err(trap)
    }
}

impl Session {
    /// A session with the process environment as exported variables and the
    /// process working directory. Streams are the process's own.
    pub fn new(grants: GrantSet) -> Result<Self, HostError> {
        let cwd =
            std::env::current_dir().map_err(|e| HostError(format!("current directory: {e}")))?;
        let mut s = Self::with_env(grants, cwd, std::env::vars())?;
        s.set_io(
            Box::new(std::io::stdin()),
            std::io::stdout(),
            std::io::stderr(),
        );
        Ok(s)
    }

    /// Set when the last run executed `exit n`: the session should end with n.
    pub fn exit_requested(&self) -> Option<i32> {
        self.shell.as_ref().expect("session shell").exit_request
    }

    /// A session with an explicit working directory and environment.
    pub fn with_env(
        grants: GrantSet,
        cwd: PathBuf,
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, HostError> {
        let g = grants::Grants::new(&grants)?;
        let cwd = grants::resolve(&PathBuf::from("/"), &cwd)
            .map_err(|e| HostError(format!("cwd: {}", e.reason())))?;
        let mut sh = Shell::new(g, cwd.clone());
        for (k, v) in env {
            sh.exported.insert(k.clone());
            sh.vars.insert(k, v);
        }
        sh.vars.insert("PWD".into(), cwd.display().to_string());
        let engine = Engine::default();
        Ok(Self {
            grants,
            engine,
            shell: Some(sh),
        })
    }

    /// Replaces the session streams (tests capture output through these).
    pub fn set_io(
        &mut self,
        stdin: Box<dyn Read>,
        stdout: impl Write + 'static,
        stderr: impl Write + 'static,
    ) {
        let sh = self.shell.as_mut().expect("session shell");
        sh.stdin = std::rc::Rc::new(std::cell::RefCell::new(stdin));
        sh.stdout = Sink::new(stdout);
        sh.stderr = Sink::new(stderr);
    }

    pub fn cwd(&self) -> PathBuf {
        self.shell.as_ref().expect("session shell").cwd.clone()
    }

    pub fn var(&self, name: &str) -> Option<String> {
        self.shell
            .as_ref()
            .expect("session shell")
            .vars
            .get(name)
            .cloned()
    }

    pub fn is_exported(&self, name: &str) -> bool {
        self.shell
            .as_ref()
            .expect("session shell")
            .exported
            .contains(name)
    }

    /// The last command's status (`$?`).
    pub fn status(&self) -> i32 {
        self.shell.as_ref().expect("session shell").status
    }

    /// Runs one built-in directly with literal arguments, outside any
    /// component. For tests of the built-ins; returns the call's status.
    #[doc(hidden)]
    pub fn call_direct(&mut self, argv: &[&str]) -> u32 {
        let sh = self.shell.as_mut().expect("session shell");
        let w: Vec<WWord> = argv.iter().map(|a| shell::lit(a)).collect();
        let st = sh.call(&w, &[], &[], 0, 0).expect("call_direct");
        // what a compiled plan does after each pipeline
        sh.status = if st >= 256 {
            (st - 256) as i32
        } else {
            st as i32
        };
        st
    }

    /// Instantiates a compiled plan component and runs its `run` export.
    /// Returns the plan's exit status.
    pub fn run_component(&mut self, component: &[u8]) -> Result<u32, HostError> {
        let comp = Component::new(&self.engine, component)
            .map_err(|e| HostError(format!("component: {e:#}")))?;
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        wit::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |s| s)
            .map_err(|e| HostError(format!("linker: {e:#}")))?;
        let mut shell = self
            .shell
            .take()
            .ok_or_else(|| HostError("session is poisoned by an earlier failure".into()))?;
        shell.begin_run();
        let mut store = Store::new(&self.engine, HostState { shell });
        let result = (|| {
            let plan = bindings::Plan::instantiate(&mut store, &comp, &linker)
                .map_err(|e| HostError(format!("instantiate: {e:#}")))?;
            plan.call_run(&mut store)
                .map_err(|e| HostError(format!("run: {e:#}")))
        })();
        // `$?` is whatever the plan last passed to `set-status`; the host
        // never updates it on its own. Per-run pipes, lists and scopes are
        // dropped here (the next `begin_run` would clear them too).
        let mut shell = store.into_data().shell;
        shell.end_run();
        self.shell = Some(shell);
        result
    }
}

/// The built-ins this host implements: exactly `kapsel_plan::BUILTINS`.
pub const BUILTINS: &[&str] = kapsel_plan::BUILTINS;

#[doc(hidden)]
pub mod testing;
#[doc(hidden)]
pub use testing::TPart;

impl Session {
    pub(crate) fn shell_mut(&mut self) -> &mut Shell {
        self.shell.as_mut().expect("session shell")
    }
}
