# kapsel

*kapsel: German for capsule, [ˈkapsl̩], "KAP-sel"; nerd reading "kap-shell".*

kapsel is a shell for AI agents and CI that checks every command before it runs, then runs it as a signed WebAssembly component.

## Vision

The goal is a WASM capability shell for AI agents running under [zwinger](https://github.com/pulseengine/zwinger): every command is understood before it runs, sees only the directories it was granted, and refuses the constructs that hide failures. Native tools such as `cargo` and `git` are to join through a narrow, checked bridge, and Bazel builds through kapsel's JSON-config mode ([design](https://github.com/pulseengine/kapsel/issues/140)).

## Why use it

- **Nothing runs without a plan.** Each `-c` command, script or interactive line is parsed and checked first. Unsupported syntax and 9 rules for harmful constructs, such as `|| true`, `curl … | sh` and `rm -rf "$X/"`, refuse it with a message that says where and what to do instead, and a refused input runs nothing; 9 more rules warn. [Refusal rules and their planted tests](testdata/kapsel/rules), [run-path test](crates/kapsel-cli/tests/acceptance.rs), [check-runs-nothing test](crates/kapsel-cli/tests/check_runs_nothing.rs)
- **What runs is sealed.** An accepted command compiles to a WASM component. It is signed with [sigil](https://github.com/pulseengine/sigil) over the hashes of its source and of its normalized plan, and every module, fresh or cached, runs only after that signature verifies. The exit-status logic of 15 of the compiler's 21 lowering rules (one fixture plan each, each call site run up to three times) and its memory layout are checked with [ordeal](https://github.com/pulseengine/ordeal): 29 queries are re-solved and their certificates re-checked on every test run. [Proofs](crates/kapsel-compile/proofs/README.md), [sealing](crates/kapsel-seal)
- **It behaves like `sh` where it accepts.** On every pull request to `main`, 36 scripts run under kapsel and under dash with GNU coreutils on Linux, and stdout, exit status and the files left behind must match. [Corpus](testdata/kapsel/corpus), [CI workflow](.github/workflows/rust.yml)

## Quick start

You need Rust 1.100 (`rustup` reads `rust-toolchain.toml`).

```sh
cargo build --release -p kapsel-cli
./target/release/kapsel -c 'echo hello | wc -c'      # prints 6
./target/release/kapsel -c 'false || true'           # refused: KS-R001, exit 65
```

`kapsel` with no arguments opens an interactive prompt; `kapsel check --json -c '…'` prints the plan, including what the command reads and writes, without running anything. The JSON-config component is described in [its reference](docs/json-config.md).

## Status

| Part | State |
|---|---|
| Shell: parser, check stage, compiler, sigil sealing, wasmtime host | works for a POSIX subset: pipes, redirects, `&&`/`||`, quoting, variables, globs, heredocs, `$(...)`, `for`/`while`/`until` |
| `if`, `case`, `{ }`, `${x:-y}`, `$(( ))` | refused today; planned for v0.4.0 |
| Built-ins | 38, including `edit`, `json`, `yaml`, `toml`, `field` and `lines`, which replace common python, perl or awk one-liners |
| Native tools (`cargo`, `git`, …) | refused today; planned |
| Grants handed over by zwinger, running on kiln | planned for v0.4.0 |
| Workflow scanner | classifies every `run:` block of a repository offline; [Phase 0 report](docs/scanner-phase0.md) |
| JSON-config component `file_ops_component.wasm` | works; built from Rust from v0.3.0 and compared with the released TinyGo v0.2.2 component on every pull request, apart from listed, counted differences |
| Windows | not tested |

## Where to find more

- [The first shell: semantics, rules, exit codes](docs/first-shell.md)
- [Release plan](docs/plan.md)
- [JSON config, CLI subcommands and release assets](docs/json-config.md)
- [Building and testing](docs/development.md)
- [Security policy and reporting](SECURITY.md)
- [Design and interface contracts](https://github.com/pulseengine/kapsel/issues/140)

## Licence

Apache-2.0. Issues and pull requests are welcome.

---

<div align="center">

<sub>Part of <a href="https://github.com/pulseengine">PulseEngine</a> &mdash; a WebAssembly Component Model engine for safety-critical systems</sub>

</div>
