# What awk was measured to do: print fields, select line ranges.
cat > ps.txt <<'EOF'
  PID TTY           TIME CMD
  101 ttys000    0:00.10 -zsh
  202 ttys001    0:02.50 cargo test --workspace
EOF
cat > build.txt <<'EOF'
compiling a
warning: unused variable
  --> src/lib.rs:3:9
compiling b
error[E0308]: mismatched types
  --> src/main.rs:7:5
done
EOF
field 1 ps.txt
field 1,4 ps.txt
field NF ps.txt
printf 'root:x:0:0:root:/root:/bin/sh\n' | field 1,7 -d : -o ' -> '
lines 2:3 ps.txt
lines '$' ps.txt
lines '/^error/,/-->/' build.txt
lines 2:3 build.txt | field 2
