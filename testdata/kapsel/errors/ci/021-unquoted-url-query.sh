# expect: warn KS-R015
# source: local miner (97 "no matches found"): an unquoted ? in a URL is a glob; zsh aborted, POSIX passes it through only if nothing matches
echo https://example.invalid/issues?since=2026
