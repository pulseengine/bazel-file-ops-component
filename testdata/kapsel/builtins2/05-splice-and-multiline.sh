# Splice a file by line range and replace a block that spans lines.
cat > lib.rs <<'EOF'
fn old() {
    // TODO: remove
    println!("old");
}

fn keep() {}
EOF
cat > new.rs <<'EOF'
fn new() {
    println!("new");
}
EOF
edit lib.rs --lines 1:4 --with new.rs
cat > notes.md <<'EOF'
intro
<!-- BEGIN generated -->
stale line 1
stale line 2
<!-- END generated -->
outro
EOF
edit notes.md -m -e '<!-- BEGIN generated -->.*<!-- END generated -->' '<!-- BEGIN generated -->
fresh
<!-- END generated -->' -c 1
edit notes.md --lines 1:0 --with-text '# Notes'
cat lib.rs notes.md
