# kapsel-compile: known-wrong and declined shapes

This table is pinned. `known_md_counts_are_pinned` in `tests/compile.rs` fails when either count
moves, in either direction, until this file, the `KNOWN_WRONG` / `DECLINED` tables in
`src/lib.rs` and the pinned numbers in the test change together.

Known-wrong count: 0
Declined count: 8

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
Construct `call`. Test `kd5_no_tool_no_assignments`.

### KD-6: command with no tool but with redirects
A redirect-only command such as `> f` (which truncates `f` in POSIX) has no host call to carry
the redirect. Construct `call`. Test `kd6_redirect_only_command`.

### KD-7: redirect whose target kind does not match its operator
For example `Write` with an fd target or `DupOut` with a word. Construct `redirect`. Test
`kd7_redirect_target_kind_mismatch`.

### KD-8: `while`/`until` with an empty condition
Construct `while`. Test `kd8_while_empty_condition`.

## Other failure codes

- `KC-002 plan-schema`: `schema` is not `kapsel.plan.v0`. Test `kc002_wrong_schema`.
- `KC-003 limit-exceeded`: more than 50 000 locals, or argument data beyond a 32-bit memory.
  Test `kc003_too_many_locals`.
- `KC-900 validator-mismatch`: the translation validator's skeleton of the component differs
  from the plan's. Tests `kc900_*`.
- `KC-901 encoder-failure`: wit-parser or wit-component failed. Not reachable from a core
  module the lowering produces; it has no test.
