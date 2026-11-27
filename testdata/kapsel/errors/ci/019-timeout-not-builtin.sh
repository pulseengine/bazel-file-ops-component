# expect: refuse KS-R009
# source: smithy data: timeout is missing on macOS (command not found)
timeout 120 cargo test
