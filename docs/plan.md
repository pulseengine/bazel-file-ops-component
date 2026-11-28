# Release plan

The plan lives in rivet: each requirement, feature, design decision and
verification in `artifacts/` carries a `release:` field. This page summarizes
it. The artifacts are the source of truth, and readiness is a query:

```sh
rivet release list                 # every release with its ready count
rivet release status v0.3.0        # what is not yet verified in v0.3.0
rivet validate                     # the trace, including gaps
```

The design, and the interface contracts with zwinger, kiln and
rules_wasm_component, are in [kapsel#140](https://github.com/pulseengine/kapsel/issues/140).

## Releases

| Release | Theme | Scope (rivet IDs) |
|---|---|---|
| v0.2.1, v0.2.2 | Path confinement of the JSON-config mode (shipped) | REQ-KS-001 to 005 |
| v0.3.0 | **first shell**: check a command, compile it to a WASM component and run it | REQ-KS-010 to 019, 020, 022, 025 |
| v0.4.0 | **sealed and composed**: cache, grants from zwinger, the `command` WIT, kiln | REQ-KS-021, 023, 024, 026 |
| v0.5.0 | **native**: allowlisted native tools, git through the broker, kapsel in CI | REQ-KS-030 to 035 |
| v1.0.0 | **protected**: every kapsel step earns the word | REQ-KS-040, 041 |
| backlog | gale and synth, Bazel under kapsel, the second batch of built-ins | REQ-KS-050 to 052 |

### v0.3.0: first shell
Owner decision 2026-11-27: every line, script and `-c` command is compiled and run from the first release, so the compiler, the executor and the first built-ins moved here from v0.4.0. The working plan for the slice is [first-shell.md](first-shell.md).
- **Architecture as in spar and rivet** (DD-KS-018): rowan (the Miri-sound fork) and salsa 0.28.5; parse, check, compile and the analysis are tracked queries.
- **Error reporting** (REQ-KS-060): rustc-style diagnostics with help and notes, `kapsel explain`, golden-tested.
- **Similar commands** (REQ-KS-061): retries without change, inconsistent flags, near-miss paths (KS-S001 to S005).
- **Error corpus** (REQ-KS-062): real CI failures and agent mistakes, each with its expected outcome.
- **Compiler after synth** (DD-KS-019): decline loudly, a translation validator on every compile, pinned fixtures.
- **Compiler** (REQ-KS-020, DD-KS-005), **executor** (REQ-KS-022) and **built-ins** (REQ-KS-025), against `wit/kapsel-shell.wit`.
- **Interactive mode** (REQ-KS-019): each line is checked, compiled and run; the session persists.
- **Parser** (REQ-KS-010, DD-KS-002): a POSIX subset on rowan, with everything else refused.
- **Check stage** (REQ-KS-011, DD-KS-003): returns a plan or a refusal and runs nothing.
- **Harmful constructs** (REQ-KS-012, DD-KS-004): refused or warned, each with a planted test.
- **Plan as versioned JSON** (REQ-KS-013), for `zwinger explain -- <command>`.
- **CLI** (REQ-KS-014): `-c` and script files, with defined exit codes.
- **Workflow scanner** (REQ-KS-015, DD-KS-014): Phase 0 of the dogfooding rollout. It runs advisory beside CI for a release before anything gates on it.
- **Refusal corpus** as a plant pack (REQ-KS-016).
- **JSON-config mode** accepts `workspace_dir: "."` (REQ-KS-017, DD-KS-011). This is Phase 1, so rules_wasm_component can drop its Go wrapper.
- **Rust toolchain** under the fleet exit-code rule, on covalent 2026.11.28 or later (REQ-KS-018).

**Stretch target:** kapsel check classifies every `run:` block in every pulseengine workflow with no parser crash, and every refusal rule has a planted script and control. The measure is the scanner report's crash count and the plant pack's rule coverage.

### v0.4.0: sealed and composed
- **Wider subset** (REQ-KS-027, owner 2026-11-28): `if`, `${x:-y}` operators, `{ }`, `$(( ))` proven with ordeal, and `case`, the five most-refused constructs in the v0.3.0 fleet scan (2,977 run: blocks, 56 accepted).
- **Cache** (REQ-KS-021): keyed and signed; grants never compiled in.
- **Executor and host** (REQ-KS-022, REQ-KS-023, DD-KS-007): ZWINGER_GRANTS, identity preopens, exit 78.
- **The `command` WIT** (REQ-KS-024, DD-KS-006): built-ins move out of the host into composed components.
- **Differential wasmtime and kiln** over a pinned corpus (REQ-KS-026, DD-KS-015).

**Stretch target:** the workflow scanner accepts at least half of the fleet's run: blocks that use no native tool, up from 56 of 2,977 in v0.3.0, with zero wasmtime/kiln disagreements on the corpus. The measure is the scanner report on the v0.3.0 snapshot and the differential job's mismatch count.

### v0.5.0: native
- **native-exec on wasmtime** (REQ-KS-030), matching kiln's SR-130, plus the per-tool host profile schema (REQ-KS-031, DD-KS-008, DD-KS-009).
- **Per-child Landlock** on Linux (REQ-KS-032).
- **The git broker** (REQ-KS-033, DD-KS-010).
- **Wrappers** checked as what they wrap (REQ-KS-034).
- **`shell: kapsel {0}`** under zwinger in kapsel's and zwinger's CI (REQ-KS-035, DD-KS-016).

**Stretch target:** every git write in kapsel's and zwinger's CI goes through the broker, and no native child receives an env name outside its profile. The measure is the broker log against the git object count, and the host's refusal log.

### v1.0.0: protected
- **The five conditions** for a protected step (REQ-KS-040, DD-KS-017).
- **The Rust JSON-config mode** replaces the TinyGo component after an equivalence test (REQ-KS-041).

**Stretch target:** every CI step in kapsel and zwinger that can run on kapsel does, and every one of them is protected. The measure is the scanner report against the workflow inventory.

## How work moves
- An item is done when its rivet artifacts are `verified`: a test that `verifies` it passes in CI.
- A release is cut when `rivet release status` says cuttable, through the release-execution procedure.
- **Moving scope** is a logged decision (`rivet release move <ID> <release>`), never a silent edit.
- **A new gate** (the scanner, a required check) first replays recent history and runs advisory beside the current process for at least one release.
- **Design decisions name a borrowed method:** each one for a feature names the verb it serves and two fields outside software that already do that verb, and borrows one method, so a reviewer can check it was used.
