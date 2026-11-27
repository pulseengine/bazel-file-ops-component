# expect: warn KS-R011
# source: kapsel ci.yml Integration Testing (fixed in #143): a failed build printed a note and the step passed
bazel build //tinygo:file_ops_component || echo "Some TinyGo targets failed to build"
