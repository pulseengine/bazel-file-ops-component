//! The check stage (REQ-KS-011, REQ-KS-012, REQ-KS-013, DD-KS-003, DD-KS-004).
//!
//! Owner: check workstream. Turns a parsed script into a plan or a refusal and
//! runs nothing. Rule ids are stable (`KS-R001` = `|| true` / `|| :`).
//!
//! Method (DD-KS-003, DD-KS-004): every rule is one named hazard, reviewed on
//! its own, with a planted script and a control under
//! `testdata/kapsel/rules/<id>/`. A refusal lists every finding, refuse and
//! warn, so one run shows everything the inspector saw.

use kapsel_ast as ast;
use kapsel_plan::{
    Call, CheckOutcome, Finding, Grants, PAndOr, PLAN_SCHEMA, PPart, PPipeline, PRedirect,
    PRedirectTarget, PStage, PWord, Plan, Refusal, Severity, Tool,
};
use sha2::{Digest, Sha256};

mod args;
mod status;
pub use args::{XFlag, XVal, xflags_for as builtin_xflags};
use args::{leading_literal, rm_danger, sed_in_place, spec_for, split_args, xflags_for, xsplit};

pub use kapsel_plan::BUILTINS;

pub const R000: &str = "KS-R000";
pub const R001: &str = "KS-R001";
pub const R002: &str = "KS-R002";
pub const R003: &str = "KS-R003";
pub const R004: &str = "KS-R004";
pub const R005: &str = "KS-R005";
pub const R006: &str = "KS-R006";
pub const R007: &str = "KS-R007";
pub const R008: &str = "KS-R008";
pub const R009: &str = "KS-R009";
pub const R010: &str = "KS-R010";
pub const R011: &str = "KS-R011";
pub const R012: &str = "KS-R012";
pub const R013: &str = "KS-R013";
pub const R014: &str = "KS-R014";
pub const R015: &str = "KS-R015";
pub const R016: &str = "KS-R016";
pub const R017: &str = "KS-R017";
pub const R018: &str = "KS-R018";

/// Every rule id this crate can put on a finding, in order. One list: the plant
/// pack, the explain table and the registry tests read it (REQ-KS-016).
pub const RULES: &[&str] = &[
    R000, R001, R002, R003, R004, R005, R006, R007, R008, R009, R010, R011, R012, R013, R014, R015,
    R016, R017, R018,
];

/// The exit codes kapsel itself uses for its own failures (REQ-KS-014,
/// `kapsel_plan::exit`): a script's `exit N` with one of these collides (KS-R018).
const KAPSEL_EXIT_CODES: &[(i32, &str)] = &[
    (kapsel_plan::exit::USAGE, "usage error"),
    (kapsel_plan::exit::REFUSED, "refusal"),
    (kapsel_plan::exit::INTERNAL, "internal error"),
    (kapsel_plan::exit::GRANTS, "grants error"),
];

/// The help line given when a parse error carries none of its own (KS-R000).
const R000_HELP: &str = "rewrite it with pipes, redirects, `&&`, `||`, `;`, quoting, `$(...)` and \
                         `for`/`while`/`until` only";

/// Shells that `-c` turns into code-from-strings (KS-R004).
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh"];
/// Programs that fetch from the network (KS-R006).
const FETCHERS: &[&str] = &["curl", "wget"];
/// Programs that run what they are fed (KS-R006).
const INTERPRETERS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "ksh", "python", "python3", "perl", "ruby", "node",
];

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Why each rule exists: one fixed note per rule, so the text is reviewed like code.
fn notes_for(rule: &str) -> Vec<String> {
    let n: &[&str] = match rule {
        R000 => &[
            "kapsel runs a small, fully parsed subset of POSIX sh; anything outside it is refused instead of guessed at (REQ-KS-010)",
        ],
        R001 => &[
            "`|| true` hides the exit status the next step or the caller needs to know the work failed (REQ-KS-012)",
            "any right-hand side of `||` whose status is always 0 does the same: `:`, `exit 0`, `! false`, a constant-true `test`, a pipeline with a `true` stage, a loop whose body always ends with status 0",
        ],
        R002 => &[
            "2>/dev/null appears in 21% of measured agent shell calls and hides the error the next step needs",
        ],
        R003 => &[
            "a recursive delete whose path can be empty, `/`, `~` or `.` is a never event: one unset variable removes everything",
        ],
        R004 => &[
            "code assembled from strings cannot be checked before it runs, so it would bypass every other rule",
        ],
        R005 => &[
            "kapsel fixes its status rules (no implicit `set -e`, no pipefail) so a plan means the same thing every time",
        ],
        R006 => &[
            "piping the network into an interpreter runs code nobody reviewed, with the session's grants",
        ],
        R007 => &[
            "a plan must end when kapsel ends; detached processes keep running with no check and no record",
        ],
        R008 => &["pattern kills also hit processes this session never started"],
        R009 => &[
            "every command is compiled to a WASM component and run in a sandbox; programs outside the built-in list need native tools, planned for v0.5 (REQ-KS-030)",
        ],
        R011 => &[
            "`|| echo` was how two CI steps in kapsel#143 passed while the build or signature check had failed",
        ],
        R012 => &[
            "an unquoted expansion in a redirect target splits or globs when the value has spaces or wildcards, and shells differ on whether that is an error",
        ],
        R013 => &[
            "`[ $n -eq 0 ]` with an empty `n` becomes `[ -eq 0 ]`, a syntax error that reads as false",
        ],
        R014 => &[
            "`cd X; cmd` runs `cmd` in the old directory when the cd fails (3 measured in agent calls)",
        ],
        R015 => &[
            "an unquoted glob in `--include=*.rs` was the commonest single failure in measured agent shell calls (268 `no matches found`)",
        ],
        R016 => &[
            "the status of a pipeline is its last stage, so a missing path in an earlier stage prints an error and still exits 0 (187 measured)",
        ],
        R017 => &[
            "an unset or empty variable at the start of a path silently turns `\"$DIR/x\"` into `/x` (91 measured)",
        ],
        R018 => &[
            "kapsel exits 64 (usage), 65 (refused), 70 (internal) and 78 (grants) for its own failures; a script that exits with one of them looks like kapsel failed (REQ-KS-014)",
        ],
        R010 => &[
            "the command name decides what the grant check applies to, so it must be known before the plan is accepted",
        ],
        _ => &[],
    };
    n.iter().map(|s| s.to_string()).collect()
}

fn help_for(rule: &str, name: &str) -> String {
    match rule {
        R001 => format!(
            "drop the `|| {name}` and let the command fail, or stop on the failure: `cmd || exit 1`; if the failure is expected, test for its cause first: `test -e stale.lock && rm stale.lock`"
        ),
        R002 => "drop the `2>/dev/null` so the error shows, or test the status: `cmd 2>/dev/null || echo 'cmd failed' >&2`".to_string(),
        R003 => "name a literal subpath such as `rm -rf ./build`, or start the path with a literal directory so an empty variable stays inside it: `rm -rf -- \"build/$X\"`".to_string(),
        R004 => format!("write the commands out as separate commands instead of `{name}`; if the text really is dynamic, split it into steps that each pass the check"),
        R005 => "remove `set`; kapsel runs with fixed status rules: handle each failure with `||` or `&&` where it matters".to_string(),
        R006 => "download the script to a file outside kapsel (it has no network built-in in v0.3), read it with `cat install.sh`, and run it only as a reviewed, checked script: `kapsel install.sh`".to_string(),
        R007 => format!("run `{name}`'s command in the foreground, or start it outside kapsel; nothing may outlive the plan"),
        R008 => format!("kill only the process id you started, outside kapsel (it has no `kill` built-in in v0.3); `{name}` matches by name and can hit others"),
        R009 => format!("use one of the built-ins ({}), or wait for native tools in v0.5 (REQ-KS-030)", BUILTINS.join(", ")),
        R011 => format!("stop the line on failure: `cmd || exit 1`, or drop the `|| {name}` and let the command fail; keep `|| {name}` only when the message is the whole report"),
        R012 => "quote the target: `> \"$FILE\"`".to_string(),
        R013 => "quote the operand: `[ \"$n\" -eq 0 ]`".to_string(),
        R014 => "chain with `&&`: `cd X && cmd`, so the next command only runs if the cd worked".to_string(),
        R015 => "quote the word so the glob reaches the program: `--include='*.rs'`, `'https://x/issues?since=1'`".to_string(),
        R016 => format!("test the path first (`[ -e {name} ] &&`) or make it the last stage"),
        R017 => format!("check it first: `test -n \"${name}\" &&`"),
        R010 => "write the command name literally, e.g. `cp` instead of `$CMD`; put the variable in an argument instead".to_string(),
        R018 => format!("pick a status kapsel does not use, e.g. `exit 1` instead of `{name}`; kapsel's own failures also print `kapsel:` (usage, grants, internal) or an `error[KS-…]` diagnostic (refused) on stderr, and `kapsel check --json` tells a refusal from a plan before anything runs"),
        _ => String::new(),
    }
}

/// KS-R009 for an interpreter: the built-in that covers what it is measured
/// to be used for (REQ-KS-063: 80,836 agent shell calls, 2026-11-27).
fn covering_builtin(name: &str) -> Option<&'static str> {
    let python_like = name == "node"
        || name == "ruby"
        || name
            .strip_prefix("python")
            .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'));
    Some(match name {
        "perl" => {
            "perl -pi -e: use `edit FILE -e RE REPL` (`-m` lets `.` match a newline, `-c N` requires N replacements); \
             a literal replace is `edit FILE -s OLD NEW`, a line range `lines A:B FILE`"
        }
        "awk" | "gawk" | "mawk" | "nawk" => {
            "awk '{print $N}': use `field N FILE` (`-d DELIM`, `-1` is the last field); \
             awk 'NR>=A && NR<=B': use `lines A:B FILE`; awk '/a/,/b/': use `lines /a/,/b/ FILE`"
        }
        "jq" => {
            "jq: use `json get PATH FILE` (`-r` raw strings, `-c` compact), `json set PATH VALUE FILE`, \
             `json del PATH FILE` or `json merge FILE OTHER`"
        }
        "yq" => {
            "yq: use `yaml get PATH FILE` or `yaml set PATH VALUE FILE`, which keeps comments; \
             TOML is `toml get PATH FILE` and `toml set PATH VALUE FILE`"
        }
        _ if python_like => {
            "for a file edit use `edit FILE -s OLD NEW` or `edit FILE -e RE REPL`; \
             for data `json get PATH FILE`, `json set PATH VALUE FILE`, `yaml get`, `toml get`; \
             also `base64`, `sha256sum`, `field N`, `lines A:B`"
        }
        _ => return None,
    })
}

/// Closest built-in within edit distance 2 (a swap of neighbours counts 1) (1 for names of 4 letters or less).
fn closest_builtin(name: &str) -> Option<&'static str> {
    let max = if name.chars().count() <= 4 { 1 } else { 2 };
    BUILTINS
        .iter()
        .filter(|b| b.chars().count() >= 2)
        .map(|b| (edit_distance(name, b), *b))
        .filter(|(d, _)| *d <= max)
        .min_by_key(|(d, _)| *d)
        .map(|(_, b)| b)
}

/// A positional argument and its span, as `split_args` returns them.
type ArgRef<'a> = (&'a PWord, ast::Span);

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let sub = d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn excerpt(source: &str, span: ast::Span) -> String {
    source
        .get(span.start as usize..span.end as usize)
        .unwrap_or("")
        .to_string()
}

fn finding(
    source: &str,
    rule: &str,
    severity: Severity,
    message: String,
    span: ast::Span,
) -> Finding {
    Finding {
        rule: rule.to_string(),
        severity,
        message,
        span,
        excerpt: excerpt(source, span),
        help: None,
        notes: notes_for(rule),
        related: Vec::new(),
    }
}

fn refusal(source: &str, findings: Vec<Finding>) -> CheckOutcome {
    CheckOutcome::Refused(Refusal {
        schema: PLAN_SCHEMA.to_string(),
        source_sha256: sha256_hex(source),
        findings,
    })
}

/// Checks a parsed script. `source` is the exact text it was parsed from
/// (for `source_sha256` and finding excerpts).
pub fn check(source: &str, script: &ast::Script) -> CheckOutcome {
    let mut cx = Cx {
        source,
        findings: Vec::new(),
        grants: Grants::default(),
        cd_seen: false,
    };
    let body = cx.lower_list(&script.items, false);
    let mut findings = cx.findings;
    findings.sort_by(|a, b| {
        (a.span.start, a.span.end, &a.rule).cmp(&(b.span.start, b.span.end, &b.rule))
    });
    if findings.iter().any(|f| f.severity == Severity::Refuse) {
        return refusal(source, findings);
    }
    CheckOutcome::Plan(Plan {
        schema: PLAN_SCHEMA.to_string(),
        source_sha256: sha256_hex(source),
        body,
        grants: cx.grants,
        warnings: findings,
    })
}

/// Turns parse errors into a refusal, so every input ends as a CheckOutcome.
pub fn refuse_parse_errors(source: &str, errors: &[ast::ParseError]) -> CheckOutcome {
    let findings = errors
        .iter()
        .map(|e| {
            // A trailing `&` is a background job: the more specific KS-R007.
            let (rule, message) = if e.construct == "background" {
                (
                    R007,
                    format!(
                        "a trailing `&` starts a background job that outlives its plan and escapes the session: {}",
                        e.message
                    ),
                )
            } else {
                (
                    R000,
                    format!(
                        "`{}` is outside kapsel's shell subset and is refused: {}",
                        e.construct, e.message
                    ),
                )
            };
            let mut f = finding(source, rule, Severity::Refuse, message, e.span);
            f.help = e.help.clone().or_else(|| {
                if rule == R007 {
                    return Some(help_for(R007, "&"));
                }
                Some(R000_HELP.to_string())
            });
            if !e.expected.is_empty() {
                f.notes.insert(0, format!("expected {}", e.expected.join(" or ")));
            }
            f
        })
        .collect();
    refusal(source, findings)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

struct Cx<'a> {
    source: &'a str,
    findings: Vec<Finding>,
    grants: Grants,
    /// A `cd` to something other than `.` has run: relative paths are unknown.
    cd_seen: bool,
}

impl Cx<'_> {
    fn refuse(&mut self, rule: &str, message: String, span: ast::Span) {
        let f = finding(self.source, rule, Severity::Refuse, message, span);
        self.findings.push(f);
    }

    /// Attaches the fix to the finding just reported.
    fn help(&mut self, text: impl Into<String>) {
        if let Some(f) = self.findings.last_mut() {
            f.help = Some(text.into());
        }
    }

    fn warn(&mut self, rule: &str, message: String, span: ast::Span) {
        let f = finding(self.source, rule, Severity::Warn, message, span);
        self.findings.push(f);
    }

    /// `in_condition`: the list is a loop condition, whose status is tested.
    fn lower_list(&mut self, items: &[ast::AndOrList], in_condition: bool) -> Vec<PAndOr> {
        let mut out = Vec::with_capacity(items.len());
        for (n, i) in items.iter().enumerate() {
            if n + 1 < items.len()
                && let Some(span) = ends_with_cd(i)
            {
                self.warn(
                        R014,
                        "`cd` is followed by more commands that run in the old directory if the `cd` fails"
                            .to_string(),
                        span,
                    );
                self.help(help_for(R014, ""));
            }
            out.push(self.lower_and_or(i, in_condition));
        }
        out
    }

    fn lower_and_or(&mut self, item: &ast::AndOrList, in_condition: bool) -> PAndOr {
        let n = item.rest.len();
        // A pipeline followed by `&&`/`||` has its status tested.
        let first = self.lower_pipeline(&item.first, in_condition || n > 0);
        let mut rest = Vec::with_capacity(n);
        let mut prev_end = item.first.span.end;
        for (i, (op, p)) in item.rest.iter().enumerate() {
            if *op == ast::AndOrOp::Or {
                self.check_or_true(p, prev_end);
            }
            rest.push((*op, self.lower_pipeline(p, in_condition || i + 1 < n)));
            prev_end = p.span.end;
        }
        self.check_or_echo(item);
        PAndOr { first, rest }
    }

    /// KS-R001: a right-hand side of `||` that cannot fail (`|| true`, `|| :`,
    /// `|| true | cat`, `|| ! false`, `|| exit 0`, `|| test 1`, a loop whose
    /// body ends in `:`) turns a failure into success. See `status`.
    fn check_or_true(&mut self, rhs: &ast::Pipeline, prev_end: u32) {
        let Some(why) = status::pipeline_zero(rhs, false, self.source) else {
            return;
        };
        let rhs_text = excerpt(self.source, rhs.span).trim().to_string();
        // The plain idiom (`|| true` with nothing else) keeps its short message.
        let idiom = match rhs.commands.as_slice() {
            [ast::Command::Simple(cmd)] if !rhs.negated && cmd.words.len() == 1 => {
                literal_of(&cmd.words[0]).filter(|n| n == "true" || n == ":")
            }
            _ => None,
        };
        // Widen the span back over the `||` operator when the text allows it.
        let gap = self
            .source
            .get(prev_end as usize..rhs.span.start as usize)
            .unwrap_or("");
        let start = gap
            .rfind("||")
            .map_or(rhs.span.start, |i| prev_end + i as u32);
        let message = match &idiom {
            Some(name) => format!(
                "`|| {name}` turns any failure of the command before it into success and \
                 destroys the error signal; let the command fail, or stop with `|| exit 1`"
            ),
            None => format!(
                "`|| {rhs_text}` cannot fail: {why}; like `|| true`, it turns any failure of \
                 the command before it into success and destroys the error signal"
            ),
        };
        self.refuse(R001, message, ast::Span::new(start, rhs.span.end));
        self.help(help_for(R001, idiom.as_deref().unwrap_or(&rhs_text)));
    }

    fn lower_pipeline(&mut self, p: &ast::Pipeline, handled: bool) -> PPipeline {
        let handled = handled || p.negated;
        let claimed = self.check_net_to_interpreter(p);
        let n = p.commands.len();
        let stages = p
            .commands
            .iter()
            .enumerate()
            .map(|(i, c)| {
                self.lower_command(c, handled && i + 1 == n, claimed.contains(&i), i + 1 == n)
            })
            .collect();
        PPipeline {
            negated: p.negated,
            stages,
        }
    }

    /// KS-R006: network fetch piped into an interpreter. Returns the stage
    /// indexes it accounts for, so KS-R009 does not repeat them.
    fn check_net_to_interpreter(&mut self, p: &ast::Pipeline) -> Vec<usize> {
        let names: Vec<Option<String>> = p
            .commands
            .iter()
            .map(|c| match c {
                ast::Command::Simple(s) => s.words.first().and_then(literal_of),
                _ => None,
            })
            .collect();
        let mut claimed = Vec::new();
        for (j, nj) in names.iter().enumerate() {
            let Some(interp) = nj.as_deref().filter(|n| INTERPRETERS.contains(n)) else {
                continue;
            };
            let Some(i) =
                (0..j).find(|&i| names[i].as_deref().is_some_and(|n| FETCHERS.contains(&n)))
            else {
                continue;
            };
            let fetcher = names[i].clone().unwrap_or_default();
            self.refuse(
                R006,
                format!(
                    "`{fetcher} ... | {interp}` runs code fetched from the network without \
                     anyone reading it; download to a file and review it first"
                ),
                p.span,
            );
            self.help(help_for(R006, ""));
            for k in [i, j] {
                if !claimed.contains(&k) {
                    claimed.push(k);
                }
            }
        }
        claimed
    }

    fn lower_command(
        &mut self,
        c: &ast::Command,
        handled: bool,
        claimed: bool,
        last: bool,
    ) -> PStage {
        match c {
            ast::Command::Simple(s) => PStage::Call(self.lower_simple(s, handled, claimed, last)),
            ast::Command::For(f) => {
                if contains_cd(&f.body) {
                    self.cd_seen = true;
                }
                let items = f.items.iter().map(|w| self.lower_word(w)).collect();
                let body = self.lower_list(&f.body, false);
                PStage::For {
                    var: f.var.clone(),
                    items,
                    body,
                }
            }
            ast::Command::While(w) => {
                if contains_cd(&w.condition) || contains_cd(&w.body) {
                    self.cd_seen = true;
                }
                let condition = self.lower_list(&w.condition, true);
                let body = self.lower_list(&w.body, false);
                PStage::While {
                    until: w.until,
                    condition,
                    body,
                }
            }
        }
    }

    fn lower_simple(
        &mut self,
        s: &ast::SimpleCommand,
        handled: bool,
        claimed: bool,
        last: bool,
    ) -> Call {
        let assignments: Vec<(String, PWord)> = s
            .assignments
            .iter()
            .map(|a| (a.name.clone(), self.lower_word(&a.value)))
            .collect();
        let argv: Vec<PWord> = s.words.iter().map(|w| self.lower_word(w)).collect();
        let redirects: Vec<PRedirect> = s
            .redirects
            .iter()
            .map(|r| self.lower_redirect(r, handled))
            .collect();
        // A redirect-only command (`> f`) is `:` with redirects: POSIX creates or
        // truncates the file and the status is 0.
        let redirect_only = argv.is_empty() && assignments.is_empty() && !redirects.is_empty();
        let argv = if redirect_only {
            vec![PWord::literal(":")]
        } else {
            argv
        };
        let tool = if redirect_only {
            Some(Tool::Builtin(":".to_string()))
        } else if argv.is_empty() {
            None
        } else {
            self.classify(s, &argv, claimed)
        };
        let call = Call {
            tool,
            argv,
            assignments,
            redirects,
            span: s.span,
        };
        self.check_option_globs(s);
        self.collect_grants(&call, s, last);
        call
    }

    /// Resolves `argv[0]` to a tool, reporting the most specific rule.
    fn classify(&mut self, s: &ast::SimpleCommand, argv: &[PWord], claimed: bool) -> Option<Tool> {
        let Some(name) = argv[0].as_literal() else {
            self.refuse(
                R010,
                "a dynamic command name (the command is chosen by a parameter or a command \
                 substitution) cannot be checked before it runs and is refused"
                    .to_string(),
                s.words[0].span,
            );
            self.help(help_for(R010, ""));
            return None;
        };
        let args = &argv[1..];
        let has_opt =
            |f: &dyn Fn(&str) -> bool| args.iter().filter_map(|a| a.as_literal()).any(|t| f(&t));
        let specific = match name.as_str() {
            "eval" | "source" | "." => {
                self.refuse(
                    R004,
                    format!(
                        "`{name}` runs code built from strings, which the check stage cannot \
                         see; write the commands out instead"
                    ),
                    s.span,
                );
                self.help(help_for(R004, &name));
                true
            }
            n if SHELLS.contains(&n)
                && has_opt(&|t| t.starts_with('-') && !t.starts_with("--") && t.contains('c')) =>
            {
                self.refuse(
                    R004,
                    format!(
                        "`{name} -c` runs a command string the check stage cannot see; write \
                         the commands out instead"
                    ),
                    s.span,
                );
                self.help(help_for(R004, &name));
                true
            }
            "set" => {
                let message = if has_opt(&|t| t.starts_with('+')) {
                    "`set +e` / `set +o pipefail` switches off error handling for the rest of \
                     the script, and `set` is not a kapsel built-in; kapsel's status rules \
                     are fixed"
                } else {
                    "`set` changes shell options at run time and is not a kapsel built-in; \
                     kapsel's status rules are fixed"
                };
                self.refuse(R005, message.to_string(), s.span);
                self.help(help_for(R005, ""));
                true
            }
            "pkill" | "killall" => {
                self.refuse(
                    R008,
                    format!(
                        "`{name}` kills every process matching a name or pattern, including \
                         ones this script did not start; kill by explicit id instead"
                    ),
                    s.span,
                );
                self.help(help_for(R008, &name));
                true
            }
            "nohup" | "setsid" | "disown" => {
                self.refuse(
                    R007,
                    format!(
                        "`{name}` lets a process escape the session and outlive its plan; \
                         nothing started here may run after kapsel exits"
                    ),
                    s.span,
                );
                self.help(help_for(R007, &name));
                true
            }
            _ => false,
        };
        if BUILTINS.contains(&name.as_str()) {
            if name == "test" || name == "[" {
                for w in s.words.iter().skip(1).filter(|w| unquoted_expansion(w)) {
                    self.warn(
                        R013,
                        format!("an unquoted expansion as an operand of `{name}`: an empty value removes the operand and the test fails with a syntax error, a value with spaces splits it"),
                        w.span,
                    );
                    self.help(help_for(R013, ""));
                }
            }
            if name == "exit" {
                self.check_exit_code(s);
            }
            if name == "rm" {
                let spans: Vec<ast::Span> = s.words.iter().skip(1).map(|w| w.span).collect();
                self.check_rm(args, &spans);
            }
            return Some(Tool::Builtin(name));
        }
        if !specific && !claimed {
            self.refuse(
                R009,
                format!(
                    "`{name}` is not a kapsel built-in and is refused; native tools arrive in \
                     v0.5 (REQ-KS-030)"
                ),
                s.words[0].span,
            );
            let mut h = help_for(R009, &name);
            if let Some(cover) = covering_builtin(&name) {
                h = format!("{cover}; other uses of `{name}` need native tools (v0.5, REQ-KS-030)");
            } else if let Some(b) = closest_builtin(&name) {
                h = format!("did you mean `{b}`? {h}");
            }
            self.help(h);
        }
        None
    }

    /// KS-R018: `exit N` where N (modulo 256) is one of kapsel's own codes.
    fn check_exit_code(&mut self, s: &ast::SimpleCommand) {
        let Some(word) = s.words.get(1) else {
            return;
        };
        let Some(n) = literal_of(word) else {
            return;
        };
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            return;
        }
        let Ok(v) = n.parse::<u128>() else {
            return;
        };
        let status = (v % 256) as i32;
        let Some((code, what)) = KAPSEL_EXIT_CODES.iter().find(|(c, _)| *c == status) else {
            return;
        };
        let text = excerpt(self.source, s.span).trim().to_string();
        self.warn(
            R018,
            format!(
                "`{text}` exits with {code}, the code kapsel uses for its own {what}: a caller \
                 cannot tell from the status alone whether the script or kapsel failed"
            ),
            s.span,
        );
        self.help(help_for(R018, &text));
    }

    /// KS-R003: recursive `rm` on a path that can be the root, home, or cwd.
    fn check_rm(&mut self, args: &[PWord], spans: &[ast::Span]) {
        let spec = spec_for("rm").expect("rm spec");
        let split = split_args(&spec, args, spans);
        if split.opaque {
            let at = args
                .iter()
                .zip(spans)
                .find(|(a, _)| {
                    let (h, n) = leading_literal(a);
                    h.starts_with('-') && n < a.parts.len()
                })
                .map(|(_, s)| *s)
                .unwrap_or_default();
            self.refuse(
                R003,
                "`rm` has an option written with an expansion (like `-$X`): it may be `-r`, and the operands cannot be told from options, so a recursive delete cannot be ruled out"
                    .to_string(),
                at,
            );
            self.help(help_for(R003, ""));
            return;
        }
        let recursive = split.opts.iter().any(|o| {
            o == "--recursive"
                || (o.starts_with('-')
                    && !o.starts_with("--")
                    && o.chars().any(|c| c == 'r' || c == 'R'))
        });
        if !recursive {
            return;
        }
        for (w, span) in split.positional {
            if let Some(why) = rm_danger(w) {
                self.refuse(
                    R003,
                    format!(
                        "recursive `rm` on a path that {why}; a recursive delete there can \
                         remove far more than intended, name an explicit literal subpath"
                    ),
                    span,
                );
                self.help(help_for(R003, ""));
            }
        }
    }

    fn lower_redirect(&mut self, r: &ast::Redirect, handled: bool) -> PRedirect {
        use ast::RedirectOp as Op;
        let fd = r.fd.unwrap_or(match r.op {
            Op::Read | Op::DupIn | Op::Heredoc => 0,
            _ => 1,
        });
        let target = match &r.target {
            ast::RedirectTarget::Word { word } => {
                let w = self.lower_word(word);
                if matches!(r.op, Op::Read | Op::Write | Op::Append) && unquoted_expansion(word) {
                    self.warn(
                        R012,
                        "an unquoted expansion as a redirect target splits into several words or expands as a glob if the value has spaces or wildcards"
                            .to_string(),
                        word.span,
                    );
                    self.help(help_for(R012, ""));
                }
                if fd == 2
                    && !handled
                    && matches!(r.op, Op::Write | Op::Append)
                    && w.as_literal().as_deref() == Some("/dev/null")
                {
                    self.warn(
                        R002,
                        "`2>/dev/null` hides this command's error messages and its failure is \
                         not otherwise handled; drop the redirect or test the status with `||`"
                            .to_string(),
                        r.span,
                    );
                    self.help(help_for(R002, ""));
                }
                PRedirectTarget::Word { word: w }
            }
            ast::RedirectTarget::Fd { fd } => PRedirectTarget::Fd { fd: *fd },
            ast::RedirectTarget::Heredoc { body, .. } => {
                // No field splitting or globbing happens in a heredoc body.
                let mut w = self.lower_word(body);
                for p in &mut w.parts {
                    match p {
                        PPart::Lit { quoted, .. }
                        | PPart::Var { quoted, .. }
                        | PPart::Capture { quoted, .. } => *quoted = true,
                        PPart::Glob { .. } => {}
                    }
                }
                PRedirectTarget::Heredoc { body: w }
            }
        };
        PRedirect {
            fd,
            op: r.op,
            target,
        }
    }

    fn lower_word(&mut self, w: &ast::Word) -> PWord {
        let mut parts = Vec::new();
        for p in &w.parts {
            self.lower_part(p, false, &mut parts);
        }
        PWord { parts }
    }

    fn lower_part(&mut self, p: &ast::WordPart, quoted: bool, out: &mut Vec<PPart>) {
        match p {
            ast::WordPart::Literal { text } => out.push(PPart::Lit {
                text: text.clone(),
                quoted,
            }),
            ast::WordPart::SingleQuoted { text } => out.push(PPart::Lit {
                text: text.clone(),
                quoted: true,
            }),
            ast::WordPart::DoubleQuoted { parts } => {
                if parts.is_empty() {
                    // `""` is an empty word, not no word.
                    out.push(PPart::Lit {
                        text: String::new(),
                        quoted: true,
                    });
                }
                for q in parts {
                    self.lower_part(q, true, out);
                }
            }
            ast::WordPart::Param { name } => out.push(PPart::Var {
                name: name.clone(),
                quoted,
            }),
            ast::WordPart::CommandSubst { script } => {
                let body = self.lower_list(&script.items, false);
                out.push(PPart::Capture { body, quoted });
            }
            ast::WordPart::Glob { glob } => {
                if quoted {
                    // Defensive: a glob inside quotes is plain text.
                    out.push(PPart::Lit {
                        text: glob_text(glob),
                        quoted: true,
                    });
                } else {
                    out.push(PPart::Glob {
                        pattern: glob_text(glob),
                    });
                }
            }
        }
    }

    /// KS-R011: `cmd || echo ...` as the last step turns a failure into success.
    fn check_or_echo(&mut self, item: &ast::AndOrList) {
        let Some((op, rhs)) = item.rest.last() else {
            return;
        };
        if *op != ast::AndOrOp::Or || rhs.negated || rhs.commands.len() != 1 {
            return;
        }
        let ast::Command::Simple(cmd) = &rhs.commands[0] else {
            return;
        };
        let Some(name) = cmd.words.first().and_then(literal_of) else {
            return;
        };
        if name != "echo" && name != "printf" {
            return;
        }
        let prev_end = match item.rest.len() {
            1 => item.first.span.end,
            n => item.rest[n - 2].1.span.end,
        };
        let gap = self
            .source
            .get(prev_end as usize..rhs.span.start as usize)
            .unwrap_or("");
        let start = gap
            .rfind("||")
            .map_or(rhs.span.start, |i| prev_end + i as u32);
        self.warn(
            R011,
            format!(
                "`|| {name} ...` as the last step prints a note when the command fails and then \
                 succeeds, so the failure no longer changes the exit status"
            ),
            ast::Span::new(start, rhs.span.end),
        );
        self.help(help_for(R011, &name));
    }

    /// KS-R015: an unquoted glob inside an option value or a URL.
    fn check_option_globs(&mut self, s: &ast::SimpleCommand) {
        for w in s.words.iter().skip(1) {
            let has_glob = w.parts.iter().any(|p| {
                matches!(p, ast::WordPart::Glob { glob } if !matches!(glob, ast::GlobPart::Class { .. }))
            });
            if !has_glob {
                continue;
            }
            let lit = |p: &ast::WordPart| match p {
                ast::WordPart::Literal { text } => Some(text.clone()),
                _ => None,
            };
            let first = w.parts.first().and_then(lit).unwrap_or_default();
            let what = if first.starts_with('-') && first.contains('=') {
                "an option value"
            } else if w.parts.iter().filter_map(lit).any(|t| t.contains("://")) {
                "a URL"
            } else {
                continue;
            };
            self.warn(
                R015,
                format!(
                    "an unquoted `*` or `?` in {what} is a glob: it expands if a file matches and \
                     the shell may refuse the line when none does"
                ),
                w.span,
            );
            self.help(help_for(R015, ""));
        }
    }

    // ---- grants and path rules ---------------------------------------------

    fn add(&mut self, w: &PWord, access: Access) {
        match w.as_literal() {
            Some(p) => {
                // After a `cd` the relative paths mean something else.
                if self.cd_seen && !p.starts_with('/') && !p.starts_with('~') {
                    self.grants.dynamic = true;
                }
                let list = match access {
                    Access::Read => &mut self.grants.read,
                    Access::Write => &mut self.grants.write,
                };
                if !list.contains(&p) {
                    list.push(p);
                }
            }
            None => self.grants.dynamic = true,
        }
    }

    /// Every path a built-in call reads or writes, with the span of the word
    /// it came from, and whether some option was not understood.
    fn accesses(call: &Call, s: &ast::SimpleCommand) -> (Vec<(PWord, Access, ast::Span)>, bool) {
        let mut out: Vec<(PWord, Access, ast::Span)> = Vec::new();
        let Some(Tool::Builtin(name)) = &call.tool else {
            return (out, false);
        };
        let args = &call.argv[1..];
        let spans: Vec<ast::Span> = s.words.iter().skip(1).map(|w| w.span).collect();
        if let Some(table) = xflags_for(name) {
            return Self::x_accesses(name, &xsplit(table, args, &spans));
        }
        let Some(spec) = spec_for(name) else {
            return (out, false);
        };
        let sp = split_args(&spec, args, &spans);
        let mut push = |ws: &[(&PWord, ast::Span)], a: Access| {
            for (w, span) in ws {
                out.push(((*w).clone(), a, *span));
            }
        };
        match name.as_str() {
            "cd" => {
                if let Some(first) = sp.positional.first() {
                    push(std::slice::from_ref(first), Access::Read);
                }
            }
            "cat" | "wc" | "ls" | "head" | "tail" | "cut" => push(&sp.positional, Access::Read),
            "uniq" => {
                // uniq [IN [OUT]]: the second operand is written.
                push(sp.positional.get(..1).unwrap_or(&[]), Access::Read);
                push(sp.positional.get(1..2).unwrap_or(&[]), Access::Write);
                if sp.positional.len() > 2 {
                    return (out, true);
                }
            }
            "sort" => {
                push(&sp.positional, Access::Read);
                for (k, w, span) in &sp.values {
                    if *k == 'o' || *k == 'T' {
                        out.push((w.clone(), Access::Write, *span));
                    }
                }
            }
            "grep" | "sed" => {
                // The pattern (grep) or script (sed) is the first positional
                // unless `-e`/`-f` supplied it.
                let mut skip = 1;
                for (k, w, span) in &sp.values {
                    if *k == 'e' || *k == 'f' {
                        skip = 0;
                    }
                    if *k == 'f' {
                        out.push((w.clone(), Access::Read, *span));
                    }
                }
                let files: Vec<(&PWord, ast::Span)> =
                    sp.positional.get(skip..).unwrap_or(&[]).to_vec();
                let in_place = name == "sed" && sp.opts.iter().any(|o| sed_in_place(o));
                if name == "sed" {
                    // A script can write files (`w`) or run commands (`e`);
                    // an expansion in it can be anything.
                    let mut scripts: Vec<&PWord> = sp
                        .values
                        .iter()
                        .filter(|(k, _, _)| *k == 'e')
                        .map(|(_, w, _)| w)
                        .collect();
                    if skip == 1 {
                        scripts.extend(sp.positional.first().map(|(w, _)| *w));
                    }
                    for w in scripts {
                        match w.as_literal() {
                            None => return (out, true),
                            Some(t) if args::sed_script_acts(&t) => return (out, true),
                            Some(_) => {}
                        }
                    }
                }
                for (w, span) in &files {
                    out.push(((*w).clone(), Access::Read, *span));
                    if in_place {
                        out.push(((*w).clone(), Access::Write, *span));
                    }
                }
                // BSD `sed -i ""`: the empty word is a suffix there and a
                // script here; the meaning of every later word is unclear.
                if in_place
                    && sp
                        .positional
                        .iter()
                        .any(|(w, _)| w.as_literal().as_deref() == Some(""))
                {
                    return (out, true);
                }
            }
            "cp" | "mv" => {
                let target = sp.values.iter().find(|(k, _, _)| *k == 't');
                let (sources, last): (&[ArgRef<'_>], Option<(PWord, ast::Span)>) = match target {
                    Some((_, w, span)) => (&sp.positional[..], Some((w.clone(), *span))),
                    None => match sp.positional.split_last() {
                        Some((l, srcs)) if !srcs.is_empty() => (srcs, Some(((*l.0).clone(), l.1))),
                        _ => (&[], None),
                    },
                };
                push(sources, Access::Read);
                if name == "mv" {
                    push(sources, Access::Write);
                }
                if let Some((w, span)) = last {
                    out.push((w, Access::Write, span));
                }
            }
            "mkdir" | "rm" | "tee" => push(&sp.positional, Access::Write),
            _ => {}
        }
        (out, sp.unknown)
    }

    /// Accesses of the REQ-KS-063 built-ins. Writers list their FILE as both
    /// read and written (it is read, edited, and replaced), like `sed -i`.
    fn x_accesses(name: &str, sp: &args::Split<'_>) -> (Vec<(PWord, Access, ast::Span)>, bool) {
        let mut out: Vec<(PWord, Access, ast::Span)> = Vec::new();
        let mut unknown = sp.unknown;
        let pos = &sp.positional;
        let mut add = |range: &[ArgRef<'_>], write: bool| {
            for (w, span) in range {
                out.push(((*w).clone(), Access::Read, *span));
                if write {
                    out.push(((*w).clone(), Access::Write, *span));
                }
            }
        };
        let from = |i: usize| pos.get(i..).unwrap_or(&[]);
        let at = |i: usize| pos.get(i..i + 1).unwrap_or(&[]);
        match name {
            "edit" => add(pos, true),
            "json" | "yaml" | "toml" => {
                match pos.first().map(|(w, _)| w.as_literal()) {
                    // json get PATH [FILE]
                    Some(Some(sub)) if sub == "get" => add(from(2), false),
                    // json set PATH VALUE FILE
                    Some(Some(sub)) if sub == "set" => add(at(3), true),
                    // json del PATH FILE
                    Some(Some(sub)) if sub == "del" && name != "toml" => add(at(2), true),
                    // json merge FILE OTHER
                    Some(Some(sub)) if sub == "merge" && name == "json" => {
                        add(at(1), true);
                        add(at(2), false);
                    }
                    // The host refuses an unknown subcommand and touches nothing.
                    Some(Some(_)) | None => {}
                    // A subcommand from an expansion: any of the above.
                    Some(None) => unknown = true,
                }
            }
            "field" | "lines" => add(from(1), false),
            "base64" => add(pos, false),
            "sha256sum" => {
                add(pos, false);
                // -c reads the files the checksum list names.
                if sp.opts.iter().any(|o| o == "-c" || o == "--check") {
                    unknown = true;
                }
            }
            _ => {}
        }
        for (k, w, span) in &sp.values {
            if *k == 'r' {
                out.push((w.clone(), Access::Read, *span));
            }
        }
        (out, unknown)
    }

    fn collect_grants(&mut self, call: &Call, s: &ast::SimpleCommand, last_stage: bool) {
        use ast::RedirectOp as Op;
        // Redirect targets, in source order.
        for (r, ar) in call.redirects.iter().zip(&s.redirects) {
            let access = match r.op {
                Op::Read => Access::Read,
                Op::Write | Op::Append => Access::Write,
                _ => continue,
            };
            if let (PRedirectTarget::Word { word }, ast::RedirectTarget::Word { word: aw }) =
                (&r.target, &ar.target)
            {
                // The null device is not a file the plan needs a grant for.
                if word.as_literal().as_deref() == Some("/dev/null") {
                    continue;
                }
                self.check_empty_prefix(word, aw.span);
                self.add(word, access);
            }
        }
        let (acc, unknown) = Self::accesses(call, s);
        if unknown {
            self.grants.dynamic = true;
        }
        let name = match &call.tool {
            Some(Tool::Builtin(n)) => n.clone(),
            None => return,
        };
        // `rm -r` with an empty-able prefix is already KS-R003.
        let skip_r017 = name == "rm"
            && self.findings.iter().any(|f| {
                f.rule == R003 && f.span.start >= s.span.start && f.span.end <= s.span.end
            });
        for (w, access, span) in &acc {
            if !skip_r017 {
                self.check_empty_prefix(w, *span);
            }
            // KS-R016: a non-last stage's failure cannot change the status.
            if !last_stage
                && *access == Access::Read
                && w.as_literal().is_some()
                && matches!(
                    name.as_str(),
                    "ls" | "cat" | "head" | "tail" | "wc" | "grep" | "sort"
                )
            {
                let path = w.as_literal().unwrap_or_default();
                self.warn(
                    R016,
                    format!(
                        "`{name} {path}` reads a path that may not exist in a pipeline stage that is \
                         not the last, so a failure here cannot change the pipeline's status"
                    ),
                    *span,
                );
                self.help(help_for(R016, &path));
            }
            self.add(w, *access);
        }
        if name == "cd" {
            let target = acc.first().and_then(|(w, _, _)| w.as_literal());
            if target.as_deref() != Some(".") {
                self.cd_seen = true;
            }
        }
    }

    /// KS-R017: a path whose first part is a variable that may be empty.
    fn check_empty_prefix(&mut self, w: &PWord, span: ast::Span) {
        if let Some(PPart::Var { name, .. }) = w.parts.first() {
            if name == "?" || name == "#" {
                return;
            }
            self.warn(
                R017,
                format!(
                    "the path begins with `${name}`, which may be empty: `\"${name}/x\"` becomes `/x`"
                ),
                span,
            );
            self.help(help_for(R017, name));
        }
    }
}

/// True when the list holds a `cd` anywhere, including nested loops.
fn contains_cd(items: &[ast::AndOrList]) -> bool {
    fn cmd(c: &ast::Command) -> bool {
        match c {
            ast::Command::Simple(s) => {
                s.words.first().and_then(literal_of).as_deref() == Some("cd")
            }
            ast::Command::For(f) => contains_cd(&f.body),
            ast::Command::While(w) => contains_cd(&w.condition) || contains_cd(&w.body),
        }
    }
    items.iter().any(|i| {
        std::iter::once(&i.first)
            .chain(i.rest.iter().map(|(_, p)| p))
            .any(|p| p.commands.iter().any(cmd))
    })
}

/// A `cd` whose own status is not tested: the last pipeline of the item,
/// a single `cd` command.
fn ends_with_cd(item: &ast::AndOrList) -> Option<ast::Span> {
    let last = item.rest.last().map_or(&item.first, |(_, p)| p);
    if last.negated || last.commands.len() != 1 {
        return None;
    }
    match &last.commands[0] {
        ast::Command::Simple(s)
            if s.words.first().and_then(literal_of).as_deref() == Some("cd") =>
        {
            Some(s.span)
        }
        _ => None,
    }
}

/// True when the word has an unquoted expansion that may be empty or split.
fn unquoted_expansion(w: &ast::Word) -> bool {
    w.parts.iter().any(|p| match p {
        ast::WordPart::Param { name } => name != "?" && name != "#",
        ast::WordPart::CommandSubst { .. } => true,
        _ => false,
    })
}

fn literal_of(w: &ast::Word) -> Option<String> {
    let mut out = String::new();
    for p in &w.parts {
        match p {
            ast::WordPart::Literal { text } | ast::WordPart::SingleQuoted { text } => {
                out.push_str(text)
            }
            ast::WordPart::DoubleQuoted { parts } => {
                for q in parts {
                    match q {
                        ast::WordPart::Literal { text } => out.push_str(text),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn glob_text(g: &ast::GlobPart) -> String {
    match g {
        ast::GlobPart::Star => "*".to_string(),
        ast::GlobPart::Question => "?".to_string(),
        ast::GlobPart::Class { set } => format!("[{set}]"),
    }
}

#[cfg(test)]
mod tests;
