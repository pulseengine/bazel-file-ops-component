# expect: run-error exit 2 | sed: can't read s/a/b/: No such file or directory
# source: this session and smithy data: BSD sed needs -i with an empty suffix, GNU sed reads it as the script
sed -i "" "s/a/b/" file.txt
