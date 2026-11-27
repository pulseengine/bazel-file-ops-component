# expect: warn KS-R017
# source: local miner (91 cases): $VAR/path where VAR was empty or unset became /path
cat "$CONFIG_DIR/settings.json"
