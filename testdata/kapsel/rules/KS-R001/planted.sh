grep -n foo missing.txt || true
grep -n foo missing.txt || :
grep -q x missing.txt || true | cat
grep -q x missing.txt || ! false
grep -q x missing.txt || exit 0
grep -q x missing.txt || test 1
grep -q x missing.txt || [ -n x ]
grep -q x missing.txt || for f in a; do :; done
