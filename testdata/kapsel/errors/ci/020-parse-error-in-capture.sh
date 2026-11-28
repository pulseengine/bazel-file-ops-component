# expect: refuse KS-R000; note the span is inside $(...)
# source: smithy data: zsh "parse error in command substitution"
x=$(grep "a" f.txt | grep "b"
