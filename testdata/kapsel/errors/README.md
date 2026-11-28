# Error corpus

Wrong inputs that kapsel must refuse, warn about, or fail on with a clear message. Each case
was a real mistake, reduced to the construct that went wrong.

- `ci/`: shell in CI `run:` blocks and in agent sessions that failed, or worse, passed while
  broken. Sources are the kapsel CI fixes in #143, mistakes made while working on kapsel, and
  smithy's classification of failed agent shell calls: 428,316 calls on 2 machines, 11,551
  failed (2026-11-27). A local miner over this machine's transcripts (80,810 Bash calls, 1,242 failed, plus 475 that exited 0 while their output showed an error) added cases 021 to 025.

Each file starts with `# expect:` and then one of:
- `refuse <rule>`: the check stage refuses it;
- `warn <rule>`: the check stage warns;
- `plan`: the check stage accepts it;
- `run-error exit N | MESSAGE`: the check stage accepts it and the run, in an empty scratch
  directory, exits with status N and prints MESSAGE on stderr (asserted through the real binary).

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
| KS-R015 | warn | an unquoted glob in an option value or a URL-like word (`--include=*.rs`, `...?since=`) |
| KS-R016 | warn | a non-last pipeline stage that reads a path which may not exist (`ls missing \| head`): its failure cannot change the status |
| KS-R017 | warn | a path starting with an expansion that may be empty (`"$DIR/x"` becomes `/x`); help suggests `${DIR:?}` once that operator is in the subset |

Never commit raw transcript text or CI logs here: only minimized constructs with paths and
names replaced.
