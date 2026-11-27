//! `kapsel`: check, compile and run shell commands as WASM components.
//!
//! Every input path goes parse -> check -> plan -> compile -> run
//! (REQ-KS-022). Exit codes are in `kapsel_plan::exit` (REQ-KS-014).

fn main() {
    eprintln!("kapsel: not implemented yet");
    std::process::exit(kapsel_plan::exit::INTERNAL);
}
