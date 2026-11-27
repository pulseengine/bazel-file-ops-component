# expect: run-error sed exit 2
# source: this session and smithy data: BSD sed needs -i with an empty suffix, GNU sed reads it as the script
sed -i "" "s/a/b/" file.txt
