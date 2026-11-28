# Workflow scanner, Phase 0 report

Phase 0 of the dogfooding rollout (REQ-KS-015, FEAT-KS-011, DD-KS-014): run
`kapsel check` over every `run:` block of a repository's workflows, offline, and
measure the gap before any gate uses the result. The scanner is advisory: it exits 0
unless the parser crashed.

```
kapsel-scan --rule run-blocks [--root DIR]... [--repos-in DIR] [--json] [--verbose]
```

- `--root` may repeat; `--repos-in DIR` scans each subdirectory of `DIR` as a repository.
  With more than one repository a `TOTAL` section follows.
- A block is `plan` (checked, possibly with warnings), `refused` (with rule ids and
  constructs), `not-shell` (an effective `shell:` that is not `bash`, `sh` or `kapsel`,
  with or without a `{0}` template, so `pwsh`, `python`, `cmd` are skipped), or `crash`
  (a panic caught with `catch_unwind`; it must never happen and is the only exit-1 case).
- The effective shell is the step's `shell:`, else the job's `defaults.run.shell`, else the
  workflow's, else `pwsh` for a job whose literal `runs-on` names a Windows label, else bash.
- Each `${{ ... }}` is replaced by a word of the same byte length (`xxxx`), so a
  finding's offset in the checked text is its offset in the YAML value. The scalar's lines
  are matched back to the YAML lines (literal `|`, folded `>`, plain and quoted), so a
  finding prints `file:line:column` of the original YAML. For a folded `>` line, which
  joins several YAML lines, the finding points at the first of those lines. An expression that spans lines
  becomes a backslash-newline continuation so it stays one word.
- Block counts are of steps with `run:`. A `run:` key that is not a step (`outputs.run`,
  `with.run`, `defaults.run`) is not a block. A file whose `run:` keys cannot be paired
  with its parsed steps is listed under `errors` and counted in no class.

## Fleet result (kapsel at 533534a plus this change)

Snapshots of the main-branch workflows of 26 pulseengine repositories (the snapshot
directory also holds an empty `.github` directory and two text files, which are not
counted) and, separately, kapsel's own workflows.

| Set | run: blocks | plan | refused | not-shell | crash |
|---|---:|---:|---:|---:|---:|
| fleet (26 repositories) | 2913 | 53 | 2855 | 5 | **0** |
| kapsel's own workflows | 64 | 3 | 61 | 0 | **0** |
| total | 2977 | 56 | 2916 | 5 | **0** |

The stretch target (docs/plan.md, v0.3.0) is met: every `run:` block is classified and the
crash count is 0. Every explicit `shell:` in the snapshots is `bash`; the 5 not-shell
blocks are shell-less steps of jobs on a literal Windows `runs-on`, which GitHub runs with
pwsh. A `runs-on` that is an expression (`${{ matrix.os }}`) is undecided and keeps the bash
default, so on a mixed matrix a Windows step can still be counted as shell.

### Top refused constructs (fleet; parse refusals, KS-R000)

| Construct | Count |
|---|---:|
| `if` | 886 |
| `syntax` (follow-on errors after an unsupported construct, mostly `case`, `(` and `)`) | 612 |
| `param-operator` (`${x:-d}`, `${x%%..}`) | 519 |
| `brace-group` | 401 |
| `arithmetic` | 162 |
| `case` | 94 |
| `subshell` | 58 |
| `here-string` | 57 |
| `function-definition` | 46 |
| `process-substitution` | 37 |
| `array-assignment` | 32 |
| `compound-redirect` | 25 |
| `double-bracket` | 18 |
| `tilde` | 12 |
| `background` | 11 |

### Refusal rules (fleet; findings, a block can hold several)

| Rule | Meaning | Count |
|---|---|---:|
| KS-R000 | construct outside the shell subset | 2976 |
| KS-R009 | not a built-in command (`cargo`, `python3`, `python`, `pip`, `sudo` lead) | 2931 |
| KS-R005 | `set` is not a built-in (`set -euo pipefail`) | 429 |
| KS-R010 | dynamic command name | 91 |
| KS-R001 | `\|\| true` or `\|\| :` | 64 |
| KS-R004 | code from strings (`eval`, `bash -c`, `source`) | 23 |
| KS-R007 | background job or `nohup` | 11 |
| KS-R003 | recursive `rm` on a path that can be root, home or cwd | 5 |
| KS-R008 | kill by pattern | 2 |
| KS-R006 | network fetch into an interpreter | 1 |

### Warnings in blocks that planned or refused (fleet)

| Rule | Meaning | Count |
|---|---|---:|
| KS-R017 | path starts with a variable that may be empty | 384 |
| KS-R014 | `cd X; cmd` | 97 |
| KS-R011 | `cmd \|\| echo ...` as the last step | 22 |
| KS-R012 | unquoted expansion in a redirect target | 16 |
| KS-R002 | `2>/dev/null` hiding a failure | 7 |
| KS-R016 | pipeline status hides an earlier stage | 3 |

### Reading the numbers

Almost all of the 2855 refusals are the v0.3.0 subset boundary: `cargo`, `python3` and
other native tools wait for v0.5 (REQ-KS-030), and `if`, `case`, parameter operators and
brace groups are outside the first shell. The rules the plan names as the point of the
scanner are KS-R001 (64 findings), KS-R003 (5) and KS-R011 (22 warnings). Rerun after each
parser addition to watch the plan count rise.

Reproduce with `kapsel-scan --rule run-blocks --repos-in <dir of repository checkouts> --json`.
The snapshots are not committed.
