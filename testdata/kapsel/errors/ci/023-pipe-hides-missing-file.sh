# expect: warn KS-R016
# source: local miner (187 "No such file" with exit 0): the pipeline status is the last stage, so the ls error is invisible
ls missing-dir | head -n 5
