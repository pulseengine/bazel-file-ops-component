# expect: warn KS-R015
# source: smithy data, the commonest single failure (268 measured): zsh "no matches found: --include=*.rs"
grep -rn foo --include=*.rs .
