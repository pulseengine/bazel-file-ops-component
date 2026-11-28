# kapsel-compile: known-wrong and declined shapes

This table is pinned. `known_md_counts_are_pinned` in `tests/compile.rs` fails when either count
moves, in either direction, until this file, the `KNOWN_WRONG` / `DECLINED` tables in
`src/lib.rs` and the pinned numbers in the test change together.

Known-wrong count: 0
Declined count: 7

## Known wrong (compiled, differs from POSIX `sh`)

None. KW-1 to KW-4 (no subshell scope for `$(...)` and pipeline stages) were fixed when the WIT
gained `scope-push` / `scope-pop`; they are now the `subshell_*` behaviour tests.

## Declined (KC-001 unsupported-construct)

Each is refused with `code = "KC-001 unsupported-construct"`, the construct name and, for calls
and redirects, the call's span. The check stage should never produce these; the compiler refuses
them rather than guess.

### KD-1: pipeline with no stages
Construct `pipeline`. Test `kd1_empty_pipeline`.

### KD-2: built-in call with an empty argv
Construct `call`. Test `kd2_empty_argv`.

### KD-3: built-in call whose argv[0] is not a literal
A dynamic command name. Construct `call`. Test `kd3_dynamic_command_name`.

### KD-4: command with no tool but with argv words
Construct `call`. Test `kd4_no_tool_with_argv`.

### KD-5: command with no tool and no assignments
With or without redirects: the check stage lowers a redirect-only `> f` to the `:` built-in.
Construct `call`. Tests `kd5_no_tool_no_assignments`, `kd5_redirect_without_tool_or_assignments`.

KD-6 (command with no tool but with redirects, e.g. `X=1 > f`) is no longer declined: rule
KC-R23 `assign-redirect` lowers it (`set-var` each assignment, then `:` with the redirects).
Tests `assign_with_redirect_persists_then_calls_colon`, `assign_redirect_order_and_persistence`,
`assign_redirect_failure_keeps_the_variable`.

### KD-7: redirect whose target kind does not match its operator
For example `Write` with an fd target or `DupOut` with a word. Construct `redirect`. Test
`kd7_redirect_target_kind_mismatch`.

### KD-8: `while`/`until` with an empty condition
Construct `while`. Test `kd8_while_empty_condition`.

## Other failure codes

- `KC-002 plan-schema`: `schema` is not `kapsel.plan.v0`. Test `kc002_wrong_schema`.
- `KC-003 limit-exceeded`: more than 50 000 locals, argument data beyond a 32-bit memory, or a list
  (argv, parts of a word, assignments, redirects, loop items) longer than 65 536 elements.
  Tests `kc003_too_many_locals`, `kc003_list_too_long`.
- `KC-900 validator-mismatch`: the translation validator's skeleton of the component differs
  from the plan's. Tests `kc900_*`.
- `KC-901 encoder-failure`: wit-parser or wit-component failed. Not reachable from a core
  module the lowering produces; it has no test.
