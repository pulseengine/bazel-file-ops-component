# expect: plan; note grep -c exits 1 on zero matches
# source: this session: grep -vc printed 0 and returned 1, which a caller read as failure
printf "completed\n" | grep -vc "^completed"
