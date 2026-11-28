echo a-b-c | sed 's/-/+/g'
echo aaa | sed 's/a/b/'
printf 'x1\nx2\nz3\n' | sed -n 's/x/y/p'
echo abc12 | sed -E 's/([a-z]+)([0-9]+)/\2\1/'
