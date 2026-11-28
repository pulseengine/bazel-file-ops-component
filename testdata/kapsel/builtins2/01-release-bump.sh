# A release bump without perl or python: regex edit, TOML set, checksums.
cat > Cargo.toml <<'EOF'
# Workspace manifest
[package]
name = "demo"        # crate name
version = "0.2.9"    # bumped by release

[dependencies]
serde = "1"
EOF
cat > CHANGELOG.md <<'EOF'
# Changelog

## Unreleased

- first change
EOF
toml get .package.version Cargo.toml
toml set .package.version '"0.3.0"' Cargo.toml
edit CHANGELOG.md -s '## Unreleased' '## 0.3.0' -c 1
edit Cargo.toml -e '^serde = "(\d+)"$' 'serde = "$1.0.200"'
toml get .dependencies.serde Cargo.toml
sha256sum Cargo.toml CHANGELOG.md > SHA256SUMS
sha256sum -c SHA256SUMS
