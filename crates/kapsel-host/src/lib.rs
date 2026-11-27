//! The kapsel host (REQ-KS-022, REQ-KS-025): runs compiled plan components on
//! wasmtime and implements the `kapsel:shell/host` imports, the built-ins and
//! the session state (variables, cwd, `$?`) that persists across lines.
//!
//! Owner: host workstream.

use std::path::PathBuf;

/// Directories the session may touch. Fail closed: a path outside every grant
/// is refused with an error, and the CLI exits 78 if a grant is invalid.
#[derive(Debug, Clone, Default)]
pub struct GrantSet {
    pub read_write: Vec<PathBuf>,
    pub read_only: Vec<PathBuf>,
}

/// State that persists across compiled plans in one shell session.
pub struct Session {
    pub grants: GrantSet,
}

#[derive(Debug)]
pub struct HostError(pub String);

impl Session {
    pub fn new(grants: GrantSet) -> Result<Self, HostError> {
        Ok(Self { grants })
    }

    /// Instantiates a compiled plan component and runs its `run` export.
    /// Returns the plan's exit status.
    pub fn run_component(&mut self, component: &[u8]) -> Result<u32, HostError> {
        let _ = component;
        Err(HostError("host not implemented yet".into()))
    }
}
