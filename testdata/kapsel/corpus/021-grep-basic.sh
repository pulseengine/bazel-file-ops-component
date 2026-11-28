printf 'Alpha one\nbeta two\nalpha three\nGamma\n' > t.txt
grep -E 'alpha|beta' t.txt
grep -n -i alpha t.txt
grep -v -i alpha t.txt
grep -c -i alpha t.txt
