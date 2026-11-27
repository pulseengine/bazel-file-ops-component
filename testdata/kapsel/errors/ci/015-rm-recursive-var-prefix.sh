# expect: refuse KS-R003
# source: destructive expansion: if BUILD_DIR is empty or unset, the path loses its root
rm -rf "$BUILD_DIR/sub"
