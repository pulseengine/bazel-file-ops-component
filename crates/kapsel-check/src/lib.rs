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
    Call, CheckOutcome, Finding, Grants, PAndOr, PPart, PPipeline, PRedirect, PRedirectTarget,
    PStage, PWord, Plan, Refusal, Severity, Tool, PLAN_SCHEMA,
};
use sha2::{Digest, Sha256};

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

/// Shells that `-c` turns into code-from-strings (KS-R004).
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh"];
/// Programs that fetch from the network (KS-R006).
const FETCHERS: &[&str] = &["curl", "wget"];
/// Programs that run what they are fed (KS-R006).
const INTERPRETERS: &[&str] =
    &["sh", "bash", "dash", "zsh", "ksh", "python", "python3", "perl", "ruby", "node"];

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Why each rule exists: one fixed note per rule, so the text is reviewed like code.
fn notes_for(rule: &str) -> Vec<String> {
    let n: &[&str] = match rule {
        R000 => &["kapsel runs a small, fully parsed subset of POSIX sh; anything outside it is refused instead of guessed at (REQ-KS-010)"],
        R001 => &["`|| true` hides the exit status the next step or the caller needs to know the work failed (REQ-KS-012)"],
        R002 => &["2>/dev/null appears in 21% of measured agent shell calls and hides the error the next step needs"],
        R003 => &["a recursive delete whose path can be empty, `/`, `~` or `.` is a never event: one unset variable removes everything"],
        R004 => &["code assembled from strings cannot be checked before it runs, so it would bypass every other rule"],
        R005 => &["kapsel fixes its status rules (no implicit `set -e`, no pipefail) so a plan means the same thing every time"],
        R006 => &["piping the network into an interpreter runs code nobody reviewed, with the session's grants"],
        R007 => &["a plan must end when kapsel ends; detached processes keep running with no check and no record"],
        R008 => &["pattern kills also hit processes this session never started"],
        R009 => &["every command is compiled to a WASM component and run in a sandbox; programs outside the built-in list need native tools, planned for v0.5 (REQ-KS-030)"],
        R010 => &["the command name decides what the grant check applies to, so it must be known before the plan is accepted"],
        _ => &[],
    };
    n.iter().map(|s| s.to_string()).collect()
}

fn help_for(rule: &str, name: &str) -> String {
    match rule {
        R001 => format!(
            "handle the failure explicitly: `cmd || {{ echo 'cmd failed, continuing because ...' >&2; }}`, or drop the `|| {name}`"
        ),
        R002 => "drop the `2>/dev/null` so the error shows, or test the status: `cmd 2>/dev/null || echo 'cmd failed' >&2`".to_string(),
        R003 => "quote the variable and guard it: `test -n \"$X\" && rm -rf -- \"$X/sub\"`, or name a literal subpath such as `rm -rf ./build`".to_string(),
        R004 => format!("write the commands out as separate commands instead of `{name}`; if the text really is dynamic, split it into steps that each pass the check"),
        R005 => "remove `set`; kapsel runs with fixed status rules: handle each failure with `||` or `&&` where it matters".to_string(),
        R006 => "download to a file, read it, then run it as a reviewed script: `curl -o install.sh URL`, then `cat install.sh`".to_string(),
        R007 => format!("run `{name}`'s command in the foreground, or start it outside kapsel; nothing may outlive the plan"),
        R008 => format!("find the process id you started and kill that one: `kill PID` (`{name}` matches by name and can hit others)"),
        R009 => format!("use one of the built-ins ({}), or wait for native tools in v0.5 (REQ-KS-030)", BUILTINS.join(", ")),
        R010 => "write the command name literally, e.g. `cp` instead of `$CMD`; put the variable in an argument instead".to_string(),
        _ => String::new(),
    }
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

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
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
    source.get(span.start as usize..span.end as usize).unwrap_or("").to_string()
}

fn finding(source: &str, rule: &str, severity: Severity, message: String, span: ast::Span) -> Finding {
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
    let mut cx = Cx { source, findings: Vec::new(), grants: Grants::default() };
    let body = cx.lower_list(&script.items, false);
    let mut findings = cx.findings;
    findings.sort_by(|a, b| (a.span.start, a.span.end, &a.rule).cmp(&(b.span.start, b.span.end, &b.rule)));
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
            let mut f = finding(
                source,
                R000,
                Severity::Refuse,
                format!(
                    "`{}` is outside kapsel's shell subset and is refused: {}",
                    e.construct, e.message
                ),
                e.span,
            );
            f.help = e.help.clone().or_else(|| {
                Some(
                    "rewrite it with pipes, redirects, `&&`, `||`, `;`, quoting, `$(...)` and \
                     `for`/`while`/`until` only"
                        .to_string(),
                )
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
        items.iter().map(|i| self.lower_and_or(i, in_condition)).collect()
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
        PAndOr { first, rest }
    }

    /// KS-R001: `|| true` and `|| :` turn a failure into success.
    fn check_or_true(&mut self, rhs: &ast::Pipeline, prev_end: u32) {
        if rhs.negated || rhs.commands.len() != 1 {
            return;
        }
        let ast::Command::Simple(cmd) = &rhs.commands[0] else { return };
        let Some(name) = cmd.words.first().and_then(literal_of) else { return };
        if name != "true" && name != ":" {
            return;
        }
        // Widen the span back over the `||` operator when the text allows it.
        let gap = self.source.get(prev_end as usize..rhs.span.start as usize).unwrap_or("");
        let start = gap.rfind("||").map_or(rhs.span.start, |i| prev_end + i as u32);
        self.refuse(
            R001,
            format!(
                "`|| {name}` turns any failure of the command before it into success and \
                 destroys the error signal; handle the failure explicitly, e.g. `|| echo ...`"
            ),
            ast::Span::new(start, rhs.span.end),
        );
        self.help(help_for(R001, &name));
    }

    fn lower_pipeline(&mut self, p: &ast::Pipeline, handled: bool) -> PPipeline {
        let handled = handled || p.negated;
        let claimed = self.check_net_to_interpreter(p);
        let n = p.commands.len();
        let stages = p
            .commands
            .iter()
            .enumerate()
            .map(|(i, c)| self.lower_command(c, handled && i + 1 == n, claimed.contains(&i)))
            .collect();
        PPipeline { negated: p.negated, stages }
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
            let Some(interp) = nj.as_deref().filter(|n| INTERPRETERS.contains(n)) else { continue };
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

    fn lower_command(&mut self, c: &ast::Command, handled: bool, claimed: bool) -> PStage {
        match c {
            ast::Command::Simple(s) => PStage::Call(self.lower_simple(s, handled, claimed)),
            ast::Command::For(f) => {
                let items = f.items.iter().map(|w| self.lower_word(w)).collect();
                let body = self.lower_list(&f.body, false);
                PStage::For { var: f.var.clone(), items, body }
            }
            ast::Command::While(w) => {
                let condition = self.lower_list(&w.condition, true);
                let body = self.lower_list(&w.body, false);
                PStage::While { until: w.until, condition, body }
            }
        }
    }

    fn lower_simple(&mut self, s: &ast::SimpleCommand, handled: bool, claimed: bool) -> Call {
        let assignments =
            s.assignments.iter().map(|a| (a.name.clone(), self.lower_word(&a.value))).collect();
        let argv: Vec<PWord> = s.words.iter().map(|w| self.lower_word(w)).collect();
        let redirects = s.redirects.iter().map(|r| self.lower_redirect(r, handled)).collect();
        let tool = if argv.is_empty() { None } else { self.classify(s, &argv, claimed) };
        let call = Call { tool, argv, assignments, redirects, span: s.span };
        let spans: Vec<ast::Span> = s.words.iter().map(|w| w.span).collect();
        self.collect_grants(&call, &spans);
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
        let has_opt = |f: &dyn Fn(&str) -> bool| {
            args.iter().filter_map(|a| a.as_literal()).any(|t| f(&t))
        };
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
            if let Some(b) = closest_builtin(&name) {
                h = format!("did you mean `{b}`? {h}");
            }
            self.help(h);
        }
        None
    }

    /// KS-R003: recursive `rm` on a path that can be the root, home, or cwd.
    fn check_rm(&mut self, args: &[PWord], spans: &[ast::Span]) {
        let split = split_args(args, spans, "");
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
        PRedirect { fd, op: r.op, target }
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
            ast::WordPart::Literal { text } => out.push(PPart::Lit { text: text.clone(), quoted }),
            ast::WordPart::SingleQuoted { text } => {
                out.push(PPart::Lit { text: text.clone(), quoted: true })
            }
            ast::WordPart::DoubleQuoted { parts } => {
                if parts.is_empty() {
                    // `""` is an empty word, not no word.
                    out.push(PPart::Lit { text: String::new(), quoted: true });
                }
                for q in parts {
                    self.lower_part(q, true, out);
                }
            }
            ast::WordPart::Param { name } => out.push(PPart::Var { name: name.clone(), quoted }),
            ast::WordPart::CommandSubst { script } => {
                let body = self.lower_list(&script.items, false);
                out.push(PPart::Capture { body, quoted });
            }
            ast::WordPart::Glob { glob } => {
                if quoted {
                    // Defensive: a glob inside quotes is plain text.
                    out.push(PPart::Lit { text: glob_text(glob), quoted: true });
                } else {
                    out.push(PPart::Glob { pattern: glob_text(glob) });
                }
            }
        }
    }

    // ---- grants -----------------------------------------------------------

    fn add(&mut self, w: &PWord, access: Access) {
        match w.as_literal() {
            Some(p) => {
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

    fn add_all(&mut self, ws: &[(&PWord, ast::Span)], access: Access) {
        for (w, _) in ws {
            self.add(w, access);
        }
    }

    fn collect_grants(&mut self, call: &Call, spans: &[ast::Span]) {
        use ast::RedirectOp as Op;
        // Redirect targets, in source order.
        for r in &call.redirects {
            let access = match r.op {
                Op::Read => Access::Read,
                Op::Write | Op::Append => Access::Write,
                _ => continue,
            };
            if let PRedirectTarget::Word { word } = &r.target {
                // The null device is not a file the plan needs a grant for.
                if word.as_literal().as_deref() == Some("/dev/null") {
                    continue;
                }
                self.add(word, access);
            }
        }
        let Some(Tool::Builtin(name)) = &call.tool else { return };
        let args = &call.argv[1..];
        let spans = spans.get(1..).unwrap_or(&[]);
        match name.as_str() {
            "cat" | "wc" | "ls" => {
                let sp = split_args(args, spans, "");
                self.add_all(&sp.positional, Access::Read);
            }
            "head" | "tail" => {
                let sp = split_args(args, spans, "nc");
                self.add_all(&sp.positional, Access::Read);
            }
            "cut" => {
                let sp = split_args(args, spans, "dfcb");
                self.add_all(&sp.positional, Access::Read);
            }
            "uniq" => {
                let sp = split_args(args, spans, "fsw");
                self.add_all(&sp.positional, Access::Read);
            }
            "sort" => {
                let sp = split_args(args, spans, "ktSoT");
                self.add_all(&sp.positional, Access::Read);
                for (o, w) in &sp.values {
                    if *o == 'o' {
                        self.add(w, Access::Write);
                    }
                }
            }
            "grep" | "sed" => {
                let sp = split_args(args, spans, if name == "grep" { "efmABC" } else { "ef" });
                // The pattern (grep) or script (sed) is the first positional
                // unless `-e`/`-f` supplied it.
                let mut skip = 1;
                for (o, w) in &sp.values {
                    if *o == 'e' || *o == 'f' {
                        skip = 0;
                    }
                    if *o == 'f' {
                        self.add(w, Access::Read);
                    }
                }
                let files = sp.positional.get(skip..).unwrap_or(&[]);
                self.add_all(files, Access::Read);
                if name == "sed" && sp.opts.iter().any(|o| sed_in_place(o)) {
                    self.add_all(files, Access::Write);
                }
            }
            "cp" | "mv" => {
                let sp = split_args(args, spans, "");
                if let Some((last, sources)) = sp.positional.split_last() {
                    if !sources.is_empty() {
                        self.add_all(sources, Access::Read);
                        if name == "mv" {
                            self.add_all(sources, Access::Write);
                        }
                        self.add_all(std::slice::from_ref(last), Access::Write);
                    }
                }
            }
            "mkdir" => {
                let sp = split_args(args, spans, "m");
                self.add_all(&sp.positional, Access::Write);
            }
            "rm" | "tee" => {
                let sp = split_args(args, spans, "");
                self.add_all(&sp.positional, Access::Write);
            }
            _ => {}
        }
    }
}

fn sed_in_place(o: &str) -> bool {
    o == "--in-place"
        || o.starts_with("--in-place=")
        || (o.starts_with('-')
            && !o.starts_with("--")
            && o[1..].chars().take_while(|c| c.is_ascii_alphabetic()).any(|c| c == 'i'))
}

struct Split<'a> {
    /// Non-option arguments, with the source span of the word.
    positional: Vec<(&'a PWord, ast::Span)>,
    /// Literal option arguments as written.
    opts: Vec<String>,
    /// Option letter and the separate argument that follows it (`-n 5`).
    values: Vec<(char, &'a PWord)>,
}

/// Splits builtin arguments into options and positionals. `value_chars` are
/// the short options that take a separate value (`head -n 5`). `--` ends the
/// options. A non-literal argument is always positional.
fn split_args<'a>(args: &'a [PWord], spans: &[ast::Span], value_chars: &str) -> Split<'a> {
    let span_of = |i: usize| spans.get(i).copied().unwrap_or_default();
    let mut sp = Split { positional: Vec::new(), opts: Vec::new(), values: Vec::new() };
    let mut after_dd = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let at = i;
        i += 1;
        match a.as_literal() {
            Some(t) if !after_dd && t == "--" => after_dd = true,
            Some(t) if !after_dd && t.len() > 1 && t.starts_with('-') => {
                let letters = &t[1..];
                let takes = !t.starts_with("--")
                    && letters.chars().all(|c| c.is_ascii_alphabetic())
                    && letters.chars().last().is_some_and(|c| value_chars.contains(c));
                if takes && i < args.len() {
                    let c = letters.chars().last().unwrap_or('-');
                    sp.values.push((c, &args[i]));
                    i += 1;
                }
                sp.opts.push(t);
            }
            _ => sp.positional.push((a, span_of(at))),
        }
    }
    sp
}

fn rm_danger(w: &PWord) -> Option<String> {
    let first = w.parts.iter().find(|p| !matches!(p, PPart::Lit { text, .. } if text.is_empty()));
    match first {
        Some(PPart::Var { name, .. }) => {
            return Some(format!("begins with `${name}`, which may be empty or unexpected"));
        }
        Some(PPart::Capture { .. }) => {
            return Some("begins with a command substitution, whose output may be empty".to_string());
        }
        _ => {}
    }
    let text = w.as_literal()?;
    let trimmed = text.trim_end_matches('/');
    let what = match (text.as_str(), trimmed) {
        ("", _) => "is empty",
        (_, "") => "is the filesystem root `/`",
        (_, "~") => "is the home directory `~`",
        (_, ".") => "is the current directory `.`",
        (_, "..") => "is the parent directory `..`",
        _ => return None,
    };
    Some(what.to_string())
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
mod testparse;
#[cfg(test)]
mod tests;
