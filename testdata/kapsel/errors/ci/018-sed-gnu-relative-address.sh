# expect: run-error exit 2 | sed: can't read file.txt: No such file or directory
# source: smithy data: sed -n "N,+20p" is GNU-only; BSD sed fails with "invalid command code ,"
# note: kapsel's sed accepts N,+M, so only the missing input fails here (the address parses)
sed -n "3,+2p" file.txt
