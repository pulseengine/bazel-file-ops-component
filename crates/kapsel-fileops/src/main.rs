//! `kapsel-fileops`: the JSON-config file-operations command (REQ-KS-041).
//! Built for `wasm32-wasip2`; see the library for the contract.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = kapsel_fileops::cli::main_with(&args);
    // REQ-KS-018: exit through std::process::exit on WASI.
    std::process::exit(code);
}
