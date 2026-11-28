# expect: run-error grants exit 1
# source: this session: writing under /tmp/x failed because the directory belonged to another user
echo data > /tmp/x/out.txt
