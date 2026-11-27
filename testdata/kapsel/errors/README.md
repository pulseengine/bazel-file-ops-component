# Error corpus

Wrong inputs that kapsel must refuse, warn about, or fail on with a clear message. Each case
was a real mistake, reduced to the construct that went wrong.

- `ci/`: shell in CI `run:` blocks and in agent sessions that failed, or worse, passed while
  broken. Sources are the kapsel CI fixes in #143, mistakes made while working on kapsel, and
  smithy's classification of failed agent shell calls: 428,316 calls on 2 machines, 11,551
  failed (2026-11-27).

Each file starts with `# expect:` and then one of:
- `refuse <rule>`: the check stage refuses it;
- `warn <rule>`: the check stage warns;
- `plan`: the check stage accepts it;
- `run-error ...`: the check stage accepts it and the run fails with a named error.

A test in kapsel-check reads these lines and fails on any case whose outcome differs. The
rendered diagnostics are golden-tested by kapsel-diag. A message must say what is wrong,
where, and what to do instead.

Rules first proposed by this corpus, to be added to the check stage:

| Id | Severity | Construct |
|---|---|---|
| KS-R011 | warn | `cmd \|\| echo ...` as the last step: the failure becomes success |
| KS-R012 | warn | an unquoted expansion as a redirect target |
| KS-R013 | warn | an unquoted expansion as an operand of `test` / `[` |
| KS-R014 | warn | `cd X;` followed by more commands: they run in the old directory if cd fails |
| KS-R015 | warn | an unquoted glob in an option value (`--include=*.rs`) |

Never commit raw transcript text or CI logs here: only minimized constructs with paths and
names replaced.
