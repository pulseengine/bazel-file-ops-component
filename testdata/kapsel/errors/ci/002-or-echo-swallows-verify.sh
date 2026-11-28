# expect: warn KS-R011
# source: kapsel release.yml Verify Signatures (fixed in #143): signature verification could never fail the release
cosign verify "$IMAGE_TAG" || echo "Note: Signature verification may require specific conditions"
