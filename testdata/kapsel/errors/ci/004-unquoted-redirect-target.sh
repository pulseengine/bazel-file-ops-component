# expect: warn KS-R012; warn KS-R017
# source: kapsel release.yml: an unquoted expansion as a redirect target splits or globs if the variable has spaces
echo "## Release Summary" >> $GITHUB_STEP_SUMMARY
