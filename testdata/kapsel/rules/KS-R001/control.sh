grep -n foo missing.txt || echo none
grep -q x missing.txt || exit 1
grep -q x missing.txt || false
