# expect: run-error exit 1 | sed: can't read s/a/b/: No such file or directory
# source: this session and smithy data: BSD sed needs -i with an empty suffix, GNU sed reads it as the script
# note: GNU sed exits 2 on an unreadable input file; kapsel exits 1 on the -i path (known difference, pinned here)
sed -i "" "s/a/b/" file.txt
