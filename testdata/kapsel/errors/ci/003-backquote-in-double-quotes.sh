# expect: refuse KS-R000; note backquote
# source: a release.yml summary line written in this session: backquotes meant as Markdown ran as command substitution
echo "- Pre-rename path: `${REGISTRY}/${LEGACY_IMAGE_NAME}`" >> "$GITHUB_STEP_SUMMARY"
