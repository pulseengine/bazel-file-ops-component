# expect: refuse KS-R001
# source: fleet CI: || true kept a failing lint step green (1,309 + 853 calls use || true, smithy 2026-11-27)
cargo clippy -- -D warnings || true
