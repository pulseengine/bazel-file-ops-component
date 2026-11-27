//! kapsel's file-operations mode: the Rust replacement for the TinyGo
//! `file_ops_component.wasm` (REQ-KS-041).
//!
//! It runs as a `wasm32-wasip2` command with the TinyGo component's
//! command-line contract, output and exit codes, and keeps its confinement
//! rules (see [`security`]) on top of WASI's preopens. The equivalence test
//! (`tests/equivalence.rs`) runs both components on one corpus.

pub mod bridge;
pub mod cli;
pub mod goerr;
pub mod gofs;
pub mod gopath;
pub mod ops;
pub mod security;
pub mod workspace;

pub use bridge::{WorkspaceInfo, process_json_config};
