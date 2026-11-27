# expect: run-error sed exit 2 with help naming the supported address forms
# source: smithy data: sed -n "N,+20p" is GNU-only; BSD sed fails with "invalid command code ,"
sed -n "3,+2p" file.txt
