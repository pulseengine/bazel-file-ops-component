# expect: refuse KS-R005
# source: this session: zsh does not word-split $p, so set -- $p assigned one argument
p="REQ-KS-050 REQ-KS-020"
set -- $p
