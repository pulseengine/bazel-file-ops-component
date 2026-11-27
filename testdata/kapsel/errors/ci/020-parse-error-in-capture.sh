# expect: refuse KS-R000 with the span inside $(...)
# source: smithy data: zsh "parse error in command substitution"
x=$(grep "a" f.txt | grep "b"
