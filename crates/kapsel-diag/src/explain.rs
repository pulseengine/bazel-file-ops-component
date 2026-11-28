//! Long-form explanations per rule id (`kapsel explain KS-R001`, like
//! `rustc --explain`). Each one says what the rule catches, why, and what to
//! do instead.

/// Every rule id with an explanation, in order.
pub const RULE_IDS: &[&str] = &[
    "KS-R000", "KS-R001", "KS-R002", "KS-R003", "KS-R004", "KS-R005", "KS-R006", "KS-R007",
    "KS-R008", "KS-R009", "KS-R010", "KS-R011", "KS-R012", "KS-R013", "KS-R014", "KS-R015",
    "KS-R016", "KS-R017", "KS-R018", "KS-S001", "KS-S002", "KS-S003", "KS-S004", "KS-S005",
];

/// The ids of run errors (REQ-KS-060): `run:<class>`, one per error class
/// of the host (`kapsel_host::ErrorClass`). A run error is not a rule hit:
/// the script ran and a built-in failed. The id is shown as
/// `error[run:grants]`, so it never reads as kapsel's own `error[KS-…]`.
pub const RUN_ERROR_IDS: &[&str] = &[
    "run:grants",
    "run:symlink",
    "run:not-found",
    "run:permission",
    "run:usage",
    "run:regex",
    "run:edit-no-match",
    "run:parse",
    "run:io",
    "run:failed",
];

/// The long-form explanation of `rule_id`, or `None` for an unknown id.
/// Ids are matched case-insensitively (`ks-r001` works).
pub fn explain(rule_id: &str) -> Option<&'static str> {
    let id = rule_id.trim().to_ascii_uppercase();
    if let Some(text) = explain_run(&rule_id.trim().to_ascii_lowercase()) {
        return Some(text);
    }
    Some(match id.as_str() {
        "KS-R000" => R000,
        "KS-R001" => R001,
        "KS-R002" => R002,
        "KS-R003" => R003,
        "KS-R004" => R004,
        "KS-R005" => R005,
        "KS-R006" => R006,
        "KS-R007" => R007,
        "KS-R008" => R008,
        "KS-R009" => R009,
        "KS-R010" => R010,
        "KS-R011" => R011,
        "KS-R012" => R012,
        "KS-R013" => R013,
        "KS-R014" => R014,
        "KS-R015" => R015,
        "KS-R016" => R016,
        "KS-R017" => R017,
        "KS-R018" => R018,
        "KS-S001" => S001,
        "KS-S002" => S002,
        "KS-S003" => S003,
        "KS-S004" => S004,
        "KS-S005" => S005,
        _ => return None,
    })
}

const R000: &str = "\
KS-R000: construct outside kapsel's subset (refuse)

kapsel parses a POSIX subset: pipes, redirection, `&&` and `||`, `;` and
newlines, quoting, parameters, globs, heredocs, `$(...)`, and
`for`/`while`/`until` loops. Anything else is refused before it can run, and
the error names the construct, for example:

    echo `date`          # backquote
    (cd sub && make)     # subshell
    f() { echo hi; }     # function-definition
    if true; then :; fi  # if
    echo ${X:-default}   # param-operator

Malformed input (an unterminated quote, a `|` with nothing after it) is
reported with construct `syntax` and what the parser expected.

What to do instead:
- backquotes: use `$(...)`.
- subshell `( ... )`: run the commands in sequence; use `cd` and `cd -` or
  absolute paths instead of a subshell's private directory.
- `if`: use `&&` and `||` (`test -f a && cat a`).
- `${X:-default}`: assign the default first (`X=${X}`, then test it), or
  pass the value explicitly.
- syntax errors: the `expected ... found ...` note says what is missing.
";

const R001: &str = "\
KS-R001: `|| true` or `|| :` discards a failure (refuse)

`cmd || true` makes every failure of `cmd` look like success. The error
signal that would stop a script, or tell the caller something went wrong,
is destroyed, and nothing records that it happened.

    false || true
    rm stale.lock || :

Any right-hand side of `||` whose status is always 0 does the same, and is
refused the same way; the message names the construct:

    grep -q x f || true | cat     # a pipeline with `true` or `:` as a stage
    grep -q x f || ! false        # `!` of something that always fails
    grep -q x f || exit 0         # also `exit 256`, and a bare `exit` after `echo`
    grep -q x f || test 1         # a test of constants: `[ 1 ]`, `[ -n x ]`
    grep -q x f || for i in a; do :; done   # a loop whose body ends in status 0

`cmd || exit 1`, `cmd || false` and `cmd || echo none` (KS-R011, a warning)
are not KS-R001: their status can be non-zero.

What to do instead:
- Drop the `|| true` and let the command fail.
- Stop on the failure: `cmd || exit 1`.
- If failure is expected and harmless, test for the condition instead:
  `test -e stale.lock && rm stale.lock`.
- If only a specific status is fine, check it: `grep -q x f; test $? -le 1`.
";

const R002: &str = "\
KS-R002: `2>/dev/null` hides errors that nothing else handles (warn)

Sending standard error to /dev/null on a command whose failure is not
otherwise handled means that when it fails, the reason is gone and the
script carries on as if nothing happened.

    grep foo a.txt 2>/dev/null

What to do instead:
- Keep standard error, or redirect it to a log file you keep: `2>>err.log`.
- If you silence errors, handle the status: `grep foo a.txt 2>/dev/null ||
  echo \"no match or no file\" >&2`.
- To test for a file, use `test -e a.txt` instead of silencing the error.
";

const R003: &str = "\
KS-R003: dangerous `rm -r` target (refuse)

`rm -r` or `rm -rf` whose path is `/`, `~`, `.`, `..`, empty, or begins
with an unquoted or possibly-empty expansion. `rm -rf $DIR/` with `DIR`
unset is `rm -rf /`.

    rm -rf $BUILD/
    rm -rf .

What to do instead:
- Name the directory literally: `rm -r build`.
- If it must come from a variable, check it first and quote it:
  `test -n \"$BUILD\" && rm -r \"$BUILD\"`.
";

const R004: &str = "\
KS-R004: code from strings (refuse)

`eval`, `sh -c`, `bash -c`, `source` and `.` run code that the check stage
never saw, so none of its rules apply to it and the plan cannot say what it
does.

    eval \"$CMD\"
    sh -c \"rm $F\"

What to do instead:
- Write the command directly in the script, so it is parsed and checked.
- To choose between commands, use `&&`/`||` with `test`, or a `for` loop
  over literal names.
- To share commands between scripts, run the other script with `kapsel
  other.sh`; it gets its own check.
";

const R005: &str = "\
KS-R005: `set` changes shell options (refuse)

`set +e`, `set +o pipefail` and `set` in general change how failures
propagate in ways the plan cannot track; kapsel does not implement `set`
as a built-in.

    set +e

What to do instead:
- kapsel uses POSIX defaults (no implicit `set -e`). Handle each failure
  where it happens with `&&` and `||`.
- To pass values, use assignments (`X=1`) and `export`.
";

const R006: &str = "\
KS-R006: network content piped into an interpreter (refuse)

`curl ... | sh` runs whatever the server sends, unseen and unchecked, with
your permissions. A truncated download runs half a script.

    curl https://example.com/install.sh | sh

What to do instead:
- Download to a file, read it, then run it as a script kapsel can check:
  `curl -o install.sh https://...`, then `kapsel install.sh`.
- Verify a published checksum or signature before running anything.
";

const R007: &str = "\
KS-R007: escaping the session (refuse)

`nohup`, `setsid`, `disown` and a trailing `&` start work that outlives
the command or runs outside the session's control, so its effects are not
part of the plan and its exit status is lost.

    nohup sleep 100

What to do instead:
- Run the command in the foreground; its status becomes part of the plan.
- For long-running services, use a service manager outside kapsel.
";

const R008: &str = "\
KS-R008: killing by pattern (refuse)

`pkill` and `killall` match process names, which also match processes you
did not start: another user's session, an editor, the shell itself.

    pkill node

What to do instead:
- Kill a specific PID you recorded when you started the process.
- If you started it from kapsel, keep it in the foreground and stop it
  there.
";

const R009: &str = "\
KS-R009: command is not a built-in (refuse)

kapsel v0.3 runs only its built-in commands (cat, echo, printf, head,
tail, wc, ls, mkdir, cp, mv, rm, grep, sed, sort, uniq, cut, tr, tee,
date, seq, sleep, and the special built-ins). Other programs are refused,
not run: native tools arrive in v0.5 (REQ-KS-030) with explicit grants.

    jq . a.json

What to do instead:
- Use a built-in that does the job (`grep`, `sed`, `cut`, `sort`).
- Run the native tool outside kapsel until v0.5.
";

const R010: &str = "\
KS-R010: dynamic command name (refuse)

`$CMD args` decides at run time which program runs, so the check stage
cannot say what the command does or which rules apply to it.

    $CMD --version

What to do instead:
- Write the command name literally.
- To choose between a few commands, use `test` with `&&`/`||`:
  `test \"$MODE\" = fast && sort a || sort -s a`.
";

const R011: &str = "\
KS-R011: `|| echo` as the last step turns a failure into success (warn)

`cmd || echo \"failed\"` prints a message and then exits 0, so the caller
sees success. Two CI steps in kapsel#143 passed this way while the build or
signature check had failed.

    cosign verify ... || echo \"verify failed\"

What to do instead:
- Stop on failure: `cmd || exit 1`, or drop the `|| echo` and let the
  command fail with its own message.
- Keep `|| echo` only when the message is the whole report and success
  really is the right status.
";

const R012: &str = "\
KS-R012: unquoted expansion in a redirect target (warn)

`> $FILE` splits or globs when the value has spaces or wildcards, and shells
differ on whether that is an error, so the output can go to the wrong file
or nowhere.

    echo hi > $OUT

What to do instead:
- Quote the target: `> \"$OUT\"`.
";

const R013: &str = "\
KS-R013: unquoted operand in `test` / `[` (warn)

`[ $n -eq 0 ]` with an empty `n` becomes `[ -eq 0 ]`, a syntax error that
reads as false, so the wrong branch runs without any visible error.

    [ $n -eq 0 ] && echo empty

What to do instead:
- Quote the operand: `[ \"$n\" -eq 0 ]`.
";

const R014: &str = "\
KS-R014: `cd X; cmd` runs `cmd` in the old directory if `cd` fails (warn)

After `;` the next command runs whatever the status of `cd`. If the
directory is missing, `cmd` runs where you were, which matters most for
`rm`, `mv` and writes.

    cd build; rm -r *

What to do instead:
- Chain with `&&`: `cd build && rm -r *`, so the next command runs only if
  the `cd` worked.
";

const R015: &str = "\
KS-R015: unquoted glob inside an option value or a URL (warn)

`--include=*.rs` and `https://x/issues?since=1` contain glob characters.
Unquoted, the shell tries to expand them against files first; with no match
the word fails or changes. It was the commonest single failure in measured
agent shell calls (268 `no matches found`).

    grep -r --include=*.rs foo src

What to do instead:
- Quote the word so the glob reaches the program: `--include='*.rs'`,
  `'https://x/issues?since=1'`.
";

const R016: &str = "\
KS-R016: an earlier pipeline stage's failure cannot change the status (warn)

The status of a pipeline is its last stage's (no pipefail). If an earlier
stage fails, for example because its input path is missing, it prints an
error and the pipeline still exits 0 (187 measured).

    cat missing.txt | sort

What to do instead:
- Test the path first: `[ -e missing.txt ] && cat missing.txt | sort`.
- Or make the failing step the last stage, or give it the file directly:
  `sort missing.txt`.
";

const R017: &str = "\
KS-R017: a path that starts with a variable that may be empty (warn)

`\"$DIR/x\"` with `DIR` unset or empty is `/x`: the command silently works on
the filesystem root instead of the intended directory (91 measured).

    cp a.txt \"$DEST/a.txt\"

What to do instead:
- Check it first: `test -n \"$DEST\" && cp a.txt \"$DEST/a.txt\"`.
- Or use a literal relative path.
";

const R018: &str = "\
KS-R018: exit code collides with kapsel's own (warn)

kapsel exits 64 for a usage error, 65 for a refusal, 70 for an internal
error and 78 for a grants error (REQ-KS-014). A script that runs `exit 65`
gives its caller the same status as a script kapsel refused, so the status
alone cannot say who failed. Statuses are taken modulo 256: `exit 321` is 65.

    grep -q x config.txt || exit 65

How a caller tells them apart:
- kapsel's own failures also print on stderr: a line starting `kapsel:`
  (usage, grants, internal) or an `error[KS-...]` diagnostic (refused).
- `kapsel check --json` decides refusal before anything runs: it exits 65
  with a refusal object, or 0 with a plan. A caller that checks first knows
  that a later 65 came from the script.

What to do instead:
- Pick a status kapsel does not use, e.g. `exit 1`, or 2 to 63.
";

const S001: &str = "\
KS-S001: retry without change (warn)

A command that failed (non-zero exit status) is run again identically, or
with only whitespace, quoting or flag-order changes, and nothing that ran in
between could have changed its inputs: no command wrote or removed a file it
reads, changed directory, or set a variable it uses. Commands that were
refused do not count, since they never ran.

    $ grep -n foo missing.txt      # exit 2
    $ grep -n 'foo' missing.txt    # same command again

The second run will fail the same way.

What to do instead:
- Read the error from the first run; it usually names the problem.
- Change the input (create the file, fix the path) or the command, then
  run it again.
";

const S002: &str = "\
KS-S002: inconsistent flags (warn)

The same tool is used in one script with diverging flags for the same
purpose:

- the same option spelled two ways: `head -5` and `head -n 5`, `grep -r`
  and `grep -R`, `-n5` and `-n 5`, `--lines=5` and `-n 5`;
- the same input processed with different flags: `cat nums | sort -n` and
  `cat nums | sort`.

Mixed spellings make a script harder to read, and a flag present in one
place and missing in the other is often a slip that changes the result
(numeric vs. lexical order, following symlinks or not).

What to do instead:
- Pick one spelling and use it everywhere; prefer the POSIX form
  (`head -n 5`).
- If the two calls really should differ, make that visible: give them
  different inputs or outputs, or a comment.
";

const S003: &str = "\
KS-S003: near-miss path (warn)

A path is read, but nothing earlier in the script (or the session) writes
or creates it, and it is within 1 or 2 edits of a path that was written
earlier. That is usually a typo, and the read fails or picks up a stale
file.

    echo hi > out/a.txt
    cat out/a.text            # did you mean out/a.txt?

Numbered series (`log1`, `log2`) and whole-name swaps (`out/a.txt`,
`out/b.txt`) are not reported.

What to do instead:
- Fix the path to the one that was written.
- Better still, put the path in a variable once and use the variable.
";

const S004: &str = "\
KS-S004: duplicated command (warn, note-level)

An identical line is repeated with no state change in between: no command
between the two writes a file the line reads, changes directory or sets a
variable it uses, and the line itself writes nothing. The second run gives
the same result as the first. Output-only commands such as `echo` are not
reported.

    wc -l data.txt
    grep x data.txt
    wc -l data.txt

What to do instead:
- Remove the repeat.
- If you need the result twice, save it once (`wc -l data.txt > count`)
  and reuse the file.
";

const S005: &str = "\
KS-S005: destructive near-miss (refuse)

`rm` or `mv` targets a path that appears nowhere else in the script, and is
within 1 or 2 edits of a path the script uses at least twice. A typo in a
destructive command deletes or moves the wrong thing, and the real target
is left in place, so this is refused rather than warned about.

    mkdir -p results
    echo a > results/a.txt
    rm -r reslts              # did you mean results?

What to do instead:
- Fix the path to the one the script uses.
- If the unusual path really is the target, name it once more before the
  destructive command, so it is not a lone near-miss:
  `test -e reslts && rm -r reslts`.
";

fn explain_run(id: &str) -> Option<&'static str> {
    Some(match id {
        "run:grants" => E_GRANTS,
        "run:symlink" => E_SYMLINK,
        "run:not-found" => E_NOT_FOUND,
        "run:permission" => E_PERMISSION,
        "run:usage" => E_USAGE,
        "run:regex" => E_REGEX,
        "run:edit-no-match" => E_EDIT_NO_MATCH,
        "run:parse" => E_PARSE,
        "run:io" => E_IO,
        "run:failed" => E_FAILED,
        _ => return None,
    })
}

const E_GRANTS: &str = "\
run:grants: a path outside the session's grants (run error)

The session may read and write the current directory, and the directories
given with `--rw DIR` (read and write) and `--ro DIR` (read only), and
nothing else. A built-in or a redirect that names a path outside them fails
with status 1 before it touches the file:

    cat /etc/hostname
    kapsel: /etc/hostname: outside the session's grants

What to do instead:
- Run kapsel from a directory that contains the files, or
- grant the directory on kapsel's command line: `kapsel --ro /etc -c ...`
  to read, `--rw DIR` to write.
";

const E_SYMLINK: &str = "\
run:symlink: a write through a symlink, a symlink copy, or a symlink loop
(run error)

kapsel does not write through a symlink (the link could point outside the
grants), does not copy or move a symlink to another directory (a relative
target would change meaning), and cannot resolve a symlink loop.

What to do instead:
- Name the file the symlink points to, inside the grants.
- Replace the symlink with the file if the script should write it.
";

const E_NOT_FOUND: &str = "\
run:not-found: a file or directory that does not exist (run error)

The built-in names the path it could not find. When the script writes a
path within one or two edits of it, the help line asks `did you mean`.

    echo a > results.txt
    cat result.txt        # did you mean `results.txt`?

What to do instead:
- Fix the path, or create the file before the command that reads it.
- Test first when the file is optional: `test -f FILE && cat FILE`.
";

const E_PERMISSION: &str = "\
run:permission: the file system refuses access (run error)

The path is inside the grants, but its permission bits refuse the read or
write.

What to do instead:
- Check the file's mode and owner (`ls -l FILE`) and change them outside
  kapsel if the script should have access.
";

const E_USAGE: &str = "\
run:usage: an unknown flag, a missing or extra operand (run error)

kapsel's built-ins accept a documented set of flags; an unknown flag is an
error naming the flag and that set, never silently ignored. A missing or
extra operand is an error too. The help line lists the flags the built-in
supports.

What to do instead:
- Use one of the listed flags, or the built-in that covers the task
  (`json`, `yaml`, `toml`, `field`, `lines`, `edit`).
";

const E_REGEX: &str = "\
run:regex: a regular expression that does not compile (run error)

`grep`, `sed` and `edit -e` compile their pattern before they read input.
The help line names the column of the pattern where the regex engine
stopped and why (an unclosed group, a bad repetition, ...).

What to do instead:
- Fix the pattern at that column; escape a literal metacharacter with a
  backslash, or use `grep -F` for a fixed string.
";

const E_EDIT_NO_MATCH: &str = "\
run:edit-no-match: `edit` found no match, or not the count `-c` asked for
(run error)

`edit FILE -s OLD NEW` and `edit FILE -e REGEX REPL` fail when OLD or REGEX
does not occur, so a script never believes an edit happened when it did not.
The file is left unchanged.

What to do instead:
- Check the search text against the file: whitespace and case count.
- Add `--allow-none` only when no match is an expected outcome.
";

const E_PARSE: &str = "\
run:parse: input that is not valid JSON, YAML, TOML or UTF-8 (run error)

`json`, `yaml` and `toml` parse their input completely before they read or
edit it; the message names the position of the error. A VALUE that does not
parse in the format is refused too (use `--str` for a string). The file is
left unchanged.

What to do instead:
- Fix the input at the position the message names.
- Pass a string value with `--str`.
";

const E_IO: &str = "\
run:io: another operating system error (run error)

The operating system refused the operation for a reason other than a missing
file or a permission: a directory where a file was expected, a file where a
directory was expected, a full disk.

What to do instead:
- Read the reason in the message and fix the operand it names.
";

const E_FAILED: &str = "\
run:failed: a built-in rejected its input (run error)

Any other failure a built-in reports: a bad number, an invalid date, a
`test` expression it cannot evaluate. The message names what was rejected.

What to do instead:
- Fix the operand or the input the message names.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_run_error_has_an_explanation_with_a_fix_section() {
        for id in RUN_ERROR_IDS {
            let text = explain(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(text.starts_with(id), "{id} must start with its id");
            assert!(
                text.contains("What to do instead:"),
                "{id} has no fix section"
            );
            assert!(!RULE_IDS.contains(id));
        }
        assert_eq!(explain("RUN:GRANTS"), explain("run:grants"));
        assert_eq!(explain("run:nope"), None);
    }

    #[test]
    fn every_rule_has_an_explanation_with_a_fix_section() {
        for id in RULE_IDS {
            let text = explain(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(text.starts_with(id), "{id} must start with its id");
            assert!(
                text.contains("What to do instead:"),
                "{id} has no fix section"
            );
        }
        assert_eq!(explain("ks-s001"), explain("KS-S001"));
        assert_eq!(explain("KS-X999"), None);
    }
}
