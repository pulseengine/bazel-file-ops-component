# kapsel-compile: known-wrong and declined shapes

This table is pinned. `known_md_counts_are_pinned` in `tests/compile.rs` fails when either count
moves, in either direction, until this file, the `KNOWN_WRONG` / `DECLINED` tables in
`src/lib.rs` and the pinned numbers in the test change together.

Known-wrong count: 4
Declined count: 8

## Known wrong (compiled, differs from POSIX `sh`)

The WIT (`kapsel:shell/host@0.1.0`) has no subshell scope, so the compiler cannot isolate a
substitution or a pipeline stage from the session. Fixing these needs a WIT addition (e.g. a
scope enter/leave pair), which the compiler workstream does not make on its own.

### KW-1: `$(...)` is not a subshell
Assignments and `$?` changes inside the body reach the session: `echo $(X=2); echo $X` prints
`2`, dash prints an empty line. Pinned by `known_wrong_kw1_capture_assignment_reaches_session`.

### KW-2: `exit n` inside `$(...)` stops the whole plan
`echo $(exit 5); echo after` returns 5 and never prints `after`. In dash only the substitution
exits. Pinned by `known_wrong_kw2_exit_in_capture_stops_plan`.

### KW-3: captures run before the host expands the command's words
`false; echo $? $(true)` prints `0`; dash prints `1`, because it expands `$?` before running the
substitution. Pinned by `known_wrong_kw3_status_in_same_command_sees_capture`.

### KW-4: pipeline stages are not subshells
Assignments in a stage persist, and `exit 3 | cat; echo after` returns 3 without running `cat`
or `echo`. Pinned by `known_wrong_kw4_exit_in_pipeline_stage_stops_plan`.

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
