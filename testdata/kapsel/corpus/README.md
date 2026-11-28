# Dash corpus

Every `NNN-name.sh` here runs under `kapsel` and under `/bin/dash` in two fresh scratch
directories (`crates/kapsel-corpus/tests/corpus.rs`); stdout, exit status and the files left
behind must agree. Scripts are deterministic and numbered in order; add new ones after the last.

What the comparison judges against:

- Most built-ins are external programs under dash, so on the Linux CI runner kapsel is compared
  with GNU coreutils (REQ-KS-025). On macOS the BSD tools differ (`date -d`, `wc` padding), so
  mismatches there are printed but do not fail.
- `echo` and `printf` are dash builtins: their forms (036 and others) are judged against dash's
  POSIX builtins, not against `/usr/bin/printf` or GNU `echo`. The forms used here avoid the
  places where those differ (backslash escapes in `echo` arguments, `echo -e`).
- `date` (034) uses fixed epoch inputs with `-u -d @N`, which is GNU syntax, so its output does
  not depend on the clock or the time zone.
