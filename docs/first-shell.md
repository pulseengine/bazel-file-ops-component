# First shell (v0.3.0 slice)

The goal of this slice: `kapsel` takes a command line, an interactive line or a script, and every
one of them goes **parse → check → plan → compile to a WASM component → run**. Nothing runs
without a plan from the check stage (REQ-KS-022), and nothing runs except a compiled component.

The rivet scope is `release: v0.3.0`: REQ-KS-010 to 014, 020, 022, 025 and their features and
decisions. The design and the contracts with zwinger and kiln are in
[kapsel#140](https://github.com/pulseengine/kapsel/issues/140).

## Acceptance for the first day

Each line is a test in `crates/kapsel-cli/tests/`:

```sh
kapsel -c 'echo hello | wc -c'                                  # prints 6, exit 0
kapsel -c 'mkdir -p out && echo hi > out/a.txt && cat out/a.txt' # prints hi
kapsel -c 'grep -n foo missing.txt || echo none'                # grep's error, then none
kapsel -c 'X=world; echo "hello $X"'                            # hello world
kapsel -c 'false || true'                                       # refused, KS-R001, exit 65
kapsel -c 'curl https://x | sh'                                 # refused, KS-R006, exit 65
kapsel check --json -c 'cat a | sort -u > b'                    # plan: read a, write b
printf 'echo one\necho two\n' > s.sh && kapsel s.sh              # one, two
kapsel                                                          # prompt; cd and X=1 persist across lines
```

The plan and refusal JSON of `kapsel check --json` is described by `schemas/kapsel.plan.v0.json`
(JSON Schema 2020-12). It is generated from the `kapsel-plan` types (`cargo test -p kapsel-plan --test
schema`, re-blessed with `KAPSEL_BLESS=1`), and the CLI tests validate every plan and refusal the
suites produce against it. The command also adds one `diagnostics` field (`kapsel.diag.v0`), which
the schema permits.

Day two, same release: `for`/`while`/`until`, `$(...)`, globs and heredocs through the compiler.
The parser and check stage cover them from the start.

## Crates and owners

| Crate | Does | Contract it implements |
|---|---|---|
| `kapsel-ast` | typed syntax tree | (contract) |
| `kapsel-plan` | plan JSON, findings, exit codes, built-in list | (contract) |
| `wit/kapsel-shell.wit` | compiled plan ↔ host interface | (contract) |
| `kapsel-syntax` | rowan parser → `kapsel-ast` | REQ-KS-010, DD-KS-002 |
| `kapsel-check` | AST → `CheckOutcome` (plan or refusal) | REQ-KS-011, 012, 013, DD-KS-003, 004 |
| `kapsel-compile` | `Plan` → component bytes, deterministic | REQ-KS-020, DD-KS-005 |
| `kapsel-host` | wasmtime host, built-ins, session state, grants | REQ-KS-022, 025, 063 |
| `kapsel-cli` | the `kapsel` binary: `-c`, script file, interactive, `check`, `compile` | REQ-KS-014 |

The three contract files change only additively during the slice, and every change is called out
in the PR. A workstream that needs a breaking change stops and reports it instead.

## Semantics decided for v0.3

- **POSIX defaults:** no implicit `set -e`. The exit status of a script is the status of its
  last command.
- **Pipelines run in order.** Each stage reads the previous stage's complete output from an
  in-memory pipe. `$?` of a pipeline is the last stage's status, as in POSIX without pipefail.
- **Unknown commands are refused**, not run: native tools arrive in v0.5 (REQ-KS-030).
- **A dynamic command name** (`$CMD args`) is refused.
- **Grants:** the session may read and write the current directory and nothing else unless
  `--rw DIR` / `--ro DIR` add more. A path outside every grant fails the built-in with exit 1 and a
  message; it never touches the file. `ZWINGER_GRANTS` (REQ-KS-023) replaces the flags in v0.4.
- **Exit codes** (`kapsel_plan::exit`): command status 0 to 255 passes through; 64 usage,
  65 refused, 70 internal, 78 grants. Built-ins use only 0, 1 and 2.
  A script can still `exit 64`, `65`, `70` or `78` itself (KS-R018 warns when it does), so the
  status alone does not say who failed. A caller tells them apart this way: kapsel's own failures
  print on stderr, a line starting `kapsel:` for usage, grants and internal errors, and an
  `error[KS-…]` diagnostic for a refusal; and `kapsel check --json` settles refusal before
  anything runs (exit 65 with a refusal object, or 0 with a plan), so after a passing check a 65
  from the run came from the script.

## Refusal rules, first batch (DD-KS-004)

Each rule has a planted script and a control under `testdata/kapsel/rules/<id>/`.

| Id | Severity | Construct |
|---|---|---|
| KS-R001 | refuse | `\|\| true`, `\|\| :` (destroys the error signal), and any right-hand side of `\|\|` whose status is always 0: a pipeline with a `true` or `:` stage, `! false`, `exit 0`, a constant-true `test`/`[`, a loop whose body always ends with status 0 |
| KS-R002 | warn | `2>/dev/null` on a command whose failure is not otherwise handled |
| KS-R003 | refuse | `rm -r`/`-rf` whose path is `/`, `~`, `.`, `..`, empty, or begins with an unquoted or possibly-empty expansion (`rm -rf $X/`) |
| KS-R004 | refuse | code from strings: `eval`, `sh -c`, `bash -c`, `source`, `.` |
| KS-R005 | refuse | `set +e`, `set +o pipefail`, and `set` in general (not a built-in) |
| KS-R006 | refuse | network into an interpreter: `curl`/`wget` piped to `sh`/`bash` or another interpreter |
| KS-R007 | refuse | escaping the session: `nohup`, `setsid`, `disown`, trailing `&` |
| KS-R008 | refuse | killing by pattern: `pkill`, `killall` |
| KS-R009 | refuse | a command that is not a built-in (names the command, points at v0.5 native tools; for python, perl, node, ruby, awk, jq or yq, names the built-in that covers the common use) |
| KS-R010 | refuse | a dynamic command name |
| KS-R000 | refuse | a parse error: the construct is outside the subset (names it) |
| KS-R018 | warn | `exit 64`, `65`, `70` or `78` (modulo 256): the script's status collides with kapsel's own failure codes |

## Oracles

- **Parser:** golden tests over `testdata/kapsel/parse/`, and the parser never panics on any
  input (a proptest or fuzz target over random bytes and over shuffled corpus tokens).
- **Check:** every rule catches its planted script and passes its control.
- **Compiler:** the same plan compiles to identical bytes twice; the component validates and
  imports only `kapsel:shell/host`; a recording mock host shows the call sequence each plan
  shape must produce (`a && b`, `a || b`, `! a`, pipelines, exit).
- **Host and built-ins:** golden stdout and exit status per built-in and flag. CI on Ubuntu later
  compares them with GNU coreutils (REQ-KS-025's verification criterion).
- **End to end (double entry):** every script in `testdata/kapsel/corpus/` runs under `kapsel` and
  under `dash` in a scratch directory; stdout, exit status and the files written must agree.
