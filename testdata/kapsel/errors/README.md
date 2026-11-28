# Error corpus

Wrong inputs that kapsel must refuse, warn about, or fail on with a clear message. Each case
was a real mistake, reduced to the construct that went wrong.

- `ci/`: shell in CI `run:` blocks and in agent sessions that failed, or worse, passed while
  broken. Sources are the kapsel CI fixes in #143, mistakes made while working on kapsel, and
  smithy's classification of failed agent shell calls: 428,316 calls on 2 machines, 11,551
  failed (2026-11-27). A local miner over this machine's transcripts (80,810 Bash calls, 1,242 failed, plus 475 that exited 0 while their output showed an error) added cases 021 to 025.

Each file starts with an `# expect:` line that states the check stage's **full** outcome:
clauses separated by `;`, each one of
- `refuse <rule>`: a refuse finding with that rule id;
- `warn <rule>`: a warn finding with that rule id;
- `plan`: the check stage accepts it; alone, with no finding at all;
- `note <text>`: free text, ignored by the test (for example which construct a KS-R000 names);
- `run-error exit N | MESSAGE`, alone on the line: the check stage accepts it with no finding,
  and the run, in an empty scratch directory, exits with status N and prints MESSAGE on stderr
  (asserted through the real binary).

The outcome is refused exactly when there is a `refuse` clause, and the set of (severity, rule)
pairs the check stage reports must equal the set the line names: a missing or an extra finding
fails. For example `bazel build //x || echo failed` is `# expect: refuse KS-R009; warn KS-R011`
(bazel is not a built-in), not `warn KS-R011`. A rule id takes no further words; put them in a
`note` clause.

A test in kapsel-check (`error_corpus_matches_expectations`) reads these lines and fails on any
case whose outcome differs. The
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
