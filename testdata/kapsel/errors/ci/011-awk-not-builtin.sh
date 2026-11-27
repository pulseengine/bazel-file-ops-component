# expect: refuse KS-R009
# source: this session and smithy data (BSD awk syntax errors): awk is not a built-in in v0.3
awk "BEGIN { print 1 }"
