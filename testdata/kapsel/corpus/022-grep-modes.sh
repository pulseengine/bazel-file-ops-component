printf 'a1 b22 c333\n' > n.txt
printf 'one\ntwo\n' > w.txt
grep -o -E '[0-9]+' n.txt
grep -q two w.txt; echo $?
grep -q three w.txt; echo $?
grep -l two w.txt n.txt
mkdir d
echo hit > d/a.txt
echo miss > d/b.txt
grep -r hit d
grep -r -l hit d
