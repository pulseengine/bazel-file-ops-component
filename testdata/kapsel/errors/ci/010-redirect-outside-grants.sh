# expect: run-error exit 1 | kapsel: /tmp/x/out.txt: outside the session's grants
# source: this session: writing under /tmp/x failed because the directory belonged to another user
echo data > /tmp/x/out.txt
