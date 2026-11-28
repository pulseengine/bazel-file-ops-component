# expect: warn KS-R013
# source: fleet scripts: [ $n -eq 0 ] with empty n fails with "unary operator expected"
n=
[ $n -eq 0 ] && echo zero
