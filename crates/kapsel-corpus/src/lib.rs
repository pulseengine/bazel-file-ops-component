//! Double-entry check for kapsel (docs/first-shell.md, "Oracles"): every script in
//! `testdata/kapsel/corpus/` runs under `kapsel` and under `dash` in two fresh
//! scratch directories, and stdout, exit status and the files left behind must
//! agree. The test lives in `tests/corpus.rs`.
//!
//! The comparison is authoritative on Linux, where the CI runner's dash uses
//! GNU coreutils as the built-ins are specified (REQ-KS-025). On other hosts
//! the external tools differ (BSD `wc` pads its output), so mismatches are
//! printed but do not fail the test.
