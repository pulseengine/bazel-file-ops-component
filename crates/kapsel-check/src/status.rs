//! KS-R001's question about the right-hand side of `||`: can its status be
//! anything but 0? If it cannot, the `||` turns every failure of the command
//! before it into success, whatever the spelling (`|| true`, `|| true | cat`,
//! `|| ! false`, `|| exit 0`, `|| test 1`, `|| X=1`, `|| export X`, a loop
//! whose body ends in `:`).
//!
//! The analysis is syntactic and only says "always 0" when the constructs make
//! it so by their definition; anything it cannot decide counts as "can fail"
//! here (the other rules and the run still apply). `test` / `[` and `exit`'s
//! operand are not re-implemented: they go through `kapsel_plan::sem`, the
//! code the host runs them with.

use kapsel_ast as ast;
use kapsel_plan::sem::{self, TestArg};

use crate::literal_of;

fn text(src: &str, span: ast::Span) -> &str {
    src.get(span.start as usize..span.end as usize)
        .unwrap_or("")
        .trim()
}

/// The literal words of a simple command, or None if any word expands.
fn literal_words(s: &ast::SimpleCommand) -> Option<Vec<String>> {
    s.words.iter().map(literal_of).collect()
}

fn simple(c: &ast::Command) -> Option<&ast::SimpleCommand> {
    match c {
        ast::Command::Simple(s) => Some(s),
        _ => None,
    }
}

fn name_of(s: &ast::SimpleCommand) -> Option<String> {
    s.words.first().and_then(literal_of)
}

/// `true` or `:`, with any arguments.
fn is_true(s: &ast::SimpleCommand) -> Option<String> {
    name_of(s).filter(|n| n == "true" || n == ":")
}

/// A command whose status is 0 in practice although it is not a constant
/// (`echo`, `printf`): enough for a bare `exit` after it to exit 0.
fn is_echo_like(s: &ast::SimpleCommand) -> bool {
    matches!(name_of(s).as_deref(), Some("echo" | "printf"))
}

/// True when a command substitution's body writes nothing to standard output
/// by definition: only `true`, `:`, `false`, `exit`, `test`, `[`, `export`
/// with operands, and assignments, with no redirects. `$(false)` expands to
/// nothing.
fn silent(script: &ast::Script) -> bool {
    script.items.iter().all(|i| {
        std::iter::once(&i.first)
            .chain(i.rest.iter().map(|(_, p)| p))
            .all(|p| {
                p.commands.iter().all(|c| match c {
                    ast::Command::Simple(s) if s.redirects.is_empty() => {
                        match name_of(s).as_deref() {
                            None => s.words.is_empty(),
                            Some("true" | ":" | "false" | "exit" | "test" | "[") => true,
                            Some("export") => s.words.len() > 1,
                            Some(_) => false,
                        }
                    }
                    _ => false,
                })
            })
    })
}

/// A word as a sequence of pieces whose value is known before the run, and
/// expansions that are not. None when the word has an unquoted expansion or
/// glob that can split into several fields or none (an unquoted silent
/// substitution expands to nothing and is dropped).
struct Pieces {
    /// The known text before the first unknown expansion.
    lead: String,
    /// All known text.
    known: String,
    /// Any quoted part: the word is one field even if its value is empty.
    quoted: bool,
    /// Any expansion whose value only the run knows (all inside quotes).
    unknown: bool,
}

fn pieces(w: &ast::Word) -> Option<Pieces> {
    let mut p = Pieces {
        lead: String::new(),
        known: String::new(),
        quoted: false,
        unknown: false,
    };
    fn lit(p: &mut Pieces, t: &str) {
        if !p.unknown {
            p.lead.push_str(t);
        }
        p.known.push_str(t);
    }
    for part in &w.parts {
        match part {
            ast::WordPart::Literal { text } => lit(&mut p, text),
            ast::WordPart::SingleQuoted { text } => {
                p.quoted = true;
                lit(&mut p, text)
            }
            ast::WordPart::DoubleQuoted { parts } => {
                p.quoted = true;
                for q in parts {
                    match q {
                        ast::WordPart::Literal { text } => lit(&mut p, text),
                        ast::WordPart::CommandSubst { script } if silent(script) => {}
                        _ => p.unknown = true,
                    }
                }
            }
            ast::WordPart::CommandSubst { script } if silent(script) => {}
            _ => return None,
        }
    }
    Some(p)
}

/// How `test` sees a word, when that is known before the run: no field (an
/// unquoted `$(false)`), its value, or a value that is one non-empty field
/// (its expansions are inside double quotes, so it neither splits nor globs,
/// and it has a literal character). None when the run decides even that.
enum Arg {
    Known(String),
    /// Non-empty, and its value starts with `lead`.
    NonEmpty {
        lead: String,
    },
}

fn test_arg(w: &ast::Word) -> Option<Vec<Arg>> {
    let p = pieces(w)?;
    if !p.unknown {
        if p.known.is_empty() && !p.quoted {
            return Some(vec![]);
        }
        return Some(vec![Arg::Known(p.known)]);
    }
    if p.known.is_empty() {
        return None;
    }
    Some(vec![Arg::NonEmpty { lead: p.lead }])
}

/// The forms a non-empty value starting with `lead` can take, as far as
/// `test` can tell them apart: each operator it may be, a `-` word that is
/// no operator (two characters, or longer), and a word that is neither.
fn forms(lead: &str) -> Vec<TestArg<'static>> {
    let mut v: Vec<TestArg<'static>> = sem::TEST_UNARY
        .iter()
        .chain(sem::TEST_BINARY)
        .chain(sem::TEST_OTHER)
        .filter(|op| op.starts_with(lead))
        .map(|op| TestArg::Known(op))
        .collect();
    if lead.is_empty() || lead == "-" {
        v.push(TestArg::Dash { short: true });
    }
    if lead.is_empty() || lead.starts_with('-') {
        v.push(TestArg::Dash { short: false });
    } else {
        v.push(TestArg::NonEmpty {
            may_be_operator: false,
        });
    }
    if lead.is_empty() {
        v.push(TestArg::NonEmpty {
            may_be_operator: false,
        });
    }
    v
}

/// What is known before the run about a `test` / `[` command's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFold {
    True,
    False,
    Malformed,
    /// False or malformed, depending on the expansions.
    Fails,
}

/// The status of a `test` / `[` command when it is known before the run:
/// the shared evaluator, run once for every form each expansion can take.
fn test_status(s: &ast::SimpleCommand) -> Option<TestFold> {
    let bracket = match name_of(s)?.as_str() {
        "test" => false,
        "[" => true,
        _ => return None,
    };
    let args: Vec<Vec<Arg>> = s.words[1..].iter().map(test_arg).collect::<Option<_>>()?;
    let args: Vec<Arg> = args.into_iter().flatten().collect();
    let choices: Vec<Vec<TestArg>> = args
        .iter()
        .map(|a| match a {
            Arg::Known(v) => vec![TestArg::Known(v.as_str())],
            Arg::NonEmpty { lead } if lead.chars().next().is_some_and(sem::operator_free_start) => {
                vec![TestArg::NonEmpty {
                    may_be_operator: false,
                }]
            }
            Arg::NonEmpty { lead } => forms(lead),
        })
        .collect();
    let combos: usize = choices.iter().map(Vec::len).product();
    if combos > 4096 {
        return None;
    }
    let (mut zero, mut one, mut two) = (false, false, false);
    for mut i in 0..combos {
        let pick: Vec<TestArg> = choices
            .iter()
            .map(|c| {
                let a = c[i % c.len()];
                i /= c.len();
                a
            })
            .collect();
        // File tests (`-e`, `-f`, ...) are answered by the run.
        match sem::eval_test(&pick, bracket, &mut |_, _| None).status()? {
            0 => zero = true,
            1 => one = true,
            _ => two = true,
        }
    }
    Some(match (zero, one, two) {
        (true, false, false) => TestFold::True,
        (false, true, false) => TestFold::False,
        (false, false, true) => TestFold::Malformed,
        (false, true, true) => TestFold::Fails,
        _ => return None,
    })
}

fn test_why(s: &ast::SimpleCommand, value: &str) -> String {
    if literal_words(s).is_some() {
        format!("tests constants and is always {value}")
    } else {
        format!(
            "is always {value}: every argument is known before the run (an expansion quoted next \
             to literal text is one non-empty word, and a substitution of a command that prints \
             nothing, like `$(false)`, expands to nothing)"
        )
    }
}

/// What is known about the status before a command: a bare `exit` exits
/// with it. The right-hand side of `||` runs only after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prev {
    Zero,
    Fails,
    Unknown,
}

/// `exit N` where N, read as the host reads it, is 0 modulo 256, or a bare
/// `exit` when the status before it is known to be 0.
fn exit_zero(s: &ast::SimpleCommand, prev: Prev, src: &str) -> Option<String> {
    let words = literal_words(s)?;
    if words.first().map(String::as_str) != Some("exit") {
        return None;
    }
    let t = text(src, s.span);
    match words.get(1..) {
        Some([]) if prev == Prev::Zero => Some(format!(
            "`{t}` with no operand exits with the status of the command before it, which is 0"
        )),
        Some([n]) if sem::exit_status(n) == Some(0) => Some(if n == "0" {
            format!("`{t}` always exits with status 0")
        } else {
            format!(
                "`{t}` exits with status 0 (the operand reads as the integer {}, and an exit \
                 status is taken modulo 256)",
                n.trim().parse::<i64>().unwrap_or(0)
            )
        }),
        _ => None,
    }
}

/// `exit` that certainly exits with a non-zero status: an operand that is
/// not 0 modulo 256 or not a number (status 2), or a bare `exit` after a
/// failure. (`exit 1 2` does not exit: too many arguments.)
fn exit_nonzero(s: &ast::SimpleCommand, prev: Prev) -> bool {
    let Some(words) = literal_words(s) else {
        return false;
    };
    match words.as_slice() {
        [e] => e == "exit" && prev == Prev::Fails,
        [e, n] => e == "exit" && sem::exit_status(n) != Some(0),
        _ => false,
    }
}

/// Running `c` certainly exits with a non-zero status.
fn command_exits_nonzero(c: &ast::Command, prev: Prev, src: &str) -> bool {
    match c {
        ast::Command::Simple(s) => exit_nonzero(s, prev),
        ast::Command::While(w) => list_exits(&w.condition, src) == Some(false),
        ast::Command::For(f) => some_item(f) && list_exits(&f.body, src) == Some(false),
    }
}

/// The last command substitution of a word list, in expansion order.
fn last_subst<'a>(parts: &'a [ast::WordPart], last: &mut Option<&'a ast::Script>) {
    for p in parts {
        match p {
            ast::WordPart::CommandSubst { script } => *last = Some(script),
            ast::WordPart::DoubleQuoted { parts } => last_subst(parts, last),
            _ => {}
        }
    }
}

/// `X=1 Y=$Z` with no command and no redirect: status 0 when it has no
/// command substitution, else the status of the last substitution (POSIX
/// 2.9.1), which is 0 only when that body always ends with status 0
/// (`X=$(true)`; `X=$(false)` and `X=$(cmd)` can fail).
fn assign_only_zero(s: &ast::SimpleCommand, src: &str) -> Option<String> {
    if !s.words.is_empty() || s.assignments.is_empty() || !s.redirects.is_empty() {
        return None;
    }
    let t = text(src, s.span);
    let mut last = None;
    for a in &s.assignments {
        last_subst(&a.value.parts, &mut last);
    }
    match last {
        None => Some(format!(
            "`{t}` only assigns variables, with no command substitution, so its status is 0"
        )),
        Some(script) => list_zero(&script.items, src).map(|why| {
            format!(
                "`{t}` only assigns variables, and its status is that of its last command \
                 substitution, which is always 0 ({why})"
            )
        }),
    }
}

/// `export NAME[=VALUE]...` whose operands all name a literal, valid name and
/// cannot split into more words (every expansion in a value is inside double
/// quotes): status 0. A substitution in a quoted value does not change
/// `export`'s status. `export 1bad` fails; `export X=$V` can split into an
/// invalid name.
fn export_zero(s: &ast::SimpleCommand, src: &str) -> Option<String> {
    let (name, ops) = s.words.split_first()?;
    if literal_of(name).as_deref() != Some("export") || ops.is_empty() || !s.redirects.is_empty() {
        return None;
    }
    let valid = ops.iter().all(|o| {
        let Some(p) = pieces(o) else {
            return false;
        };
        let name = match p.lead.split_once('=') {
            Some((n, _)) => n,
            None if !p.unknown => p.known.as_str(),
            None => return false,
        };
        sem::valid_name(name)
    });
    valid.then(|| {
        format!(
            "`{}` exports valid literal names and always has status 0",
            text(src, s.span)
        )
    })
}

/// `export` with an operand whose name is literal and not a valid name
/// (`export 1bad`): status 1 whatever the other operands do.
fn export_fails(s: &ast::SimpleCommand) -> bool {
    let Some((name, ops)) = s.words.split_first() else {
        return false;
    };
    literal_of(name).as_deref() == Some("export")
        && s.redirects.is_empty()
        && ops.iter().any(|o| {
            pieces(o).is_some_and(|p| {
                let name = match p.lead.split_once('=') {
                    Some((n, _)) => n,
                    None if !p.unknown => p.known.as_str(),
                    None => return false,
                };
                !name.is_empty() && !name.starts_with('-') && !sem::valid_name(name)
            })
        })
}

/// An assignment-only command whose last command substitution always fails.
fn assign_only_fails(s: &ast::SimpleCommand, src: &str) -> bool {
    if !s.words.is_empty() || s.assignments.is_empty() || !s.redirects.is_empty() {
        return false;
    }
    let mut last = None;
    for a in &s.assignments {
        last_subst(&a.value.parts, &mut last);
    }
    last.is_some_and(|script| list_fails(&script.items, src).is_some())
}

/// A `for` list that expands to no items: empty, or only unquoted
/// substitutions of commands that print nothing (`for i in $(false)`).
fn no_items(f: &ast::ForLoop) -> bool {
    f.items
        .iter()
        .all(|w| pieces(w).is_some_and(|p| !p.unknown && !p.quoted && p.known.is_empty()))
}

/// A `for` list with at least one item: a quoted word, or one with literal
/// text and no unquoted expansion, is exactly one item.
fn some_item(f: &ast::ForLoop) -> bool {
    f.items
        .iter()
        .any(|w| pieces(w).is_some_and(|p| p.quoted || !p.known.is_empty()))
}

/// Why the loop's body never runs.
fn never_runs(c: &ast::Command, src: &str) -> Option<String> {
    match c {
        ast::Command::For(f) if no_items(f) => Some(format!(
            "`{}` has no items, so its body never runs and its status is 0",
            text(src, f.span)
        )),
        // `while <always fails>` / `until <always 0>`.
        ast::Command::While(w) if w.until => list_zero(&w.condition, src),
        ast::Command::While(w) => list_fails(&w.condition, src),
        _ => None,
    }
}

/// Why running `c` certainly ends the script with status 0 (`exit 0`,
/// `while exit 0; ...`, a `for` with an item whose body does).
fn command_exits_zero(c: &ast::Command, prev: Prev, src: &str) -> Option<String> {
    match c {
        ast::Command::Simple(s) => exit_zero(s, prev, src),
        ast::Command::While(w) => list_exits_zero(&w.condition, src).map(|why| {
            format!(
                "the `{}` loop's condition exits with status 0 ({why})",
                if w.until { "until" } else { "while" }
            )
        }),
        ast::Command::For(f) if some_item(f) => list_exits_zero(&f.body, src).map(|why| {
            format!(
                "the `for` loop runs its body at least once, and it exits with status 0 ({why})"
            )
        }),
        ast::Command::For(_) => None,
    }
}

/// True when running `c` (as a pipeline of one) may run an `exit` that ends
/// the script or the enclosing capture.
fn command_may_exit(c: &ast::Command, src: &str) -> bool {
    match c {
        ast::Command::Simple(s) => name_of(s).as_deref() == Some("exit"),
        ast::Command::For(f) => never_runs(c, src).is_none() && list_may_exit(&f.body, src),
        ast::Command::While(w) => {
            list_may_exit(&w.condition, src)
                || (never_runs(c, src).is_none() && list_may_exit(&w.body, src))
        }
    }
}

/// Why `c` always has status 0, given the status before it.
fn command_zero(c: &ast::Command, prev: Prev, src: &str) -> Option<String> {
    if let Some(why) = command_exits_zero(c, prev, src) {
        return Some(why);
    }
    match c {
        ast::Command::Simple(s) => {
            if let Some(n) = is_true(s) {
                return Some(format!("`{n}` always has status 0"));
            }
            if test_status(s) == Some(TestFold::True) {
                return Some(format!("`{}` {}", text(src, s.span), test_why(s, "true")));
            }
            assign_only_zero(s, src).or_else(|| export_zero(s, src))
        }
        ast::Command::For(f) => {
            if let Some(why) = never_runs(c, src) {
                return Some(why);
            }
            list_zero(&f.body, src).map(|why| {
                format!(
                    "the `for` loop's body always ends with status 0 ({why}), and a loop with \
                     no items has status 0 too"
                )
            })
        }
        ast::Command::While(w) => {
            let kw = if w.until { "until" } else { "while" };
            if let Some(why) = never_runs(c, src) {
                return Some(format!(
                    "the `{kw}` loop's body never runs ({why}), so the loop's status is 0"
                ));
            }
            // A condition that may exit can end the script with any status.
            if list_may_exit(&w.condition, src) {
                return None;
            }
            list_zero(&w.body, src).map(|why| {
                format!(
                    "the `{kw}` loop's body always ends with status 0 ({why}), and a loop whose \
                     body does not run has status 0 too"
                )
            })
        }
    }
}

/// Why `c`, as the last stage of a pipeline (`multi`: of two or more, where
/// it is its own scope), always has a non-zero status.
fn command_fails(c: &ast::Command, prev: Prev, multi: bool, src: &str) -> Option<String> {
    // The stage ends with the exit's status (a bare `exit`: the status
    // before the pipeline).
    if multi && command_exits_nonzero(c, prev, src) {
        return Some(
            "the last stage exits with a non-zero status, which ends only the stage".to_string(),
        );
    }
    match c {
        ast::Command::Simple(s) => {
            let t = text(src, s.span);
            if name_of(s).as_deref() == Some("false") {
                return Some("`false` always has status 1".to_string());
            }
            match test_status(s) {
                Some(TestFold::False) => return Some(format!("`{t}` {}", test_why(s, "false"))),
                Some(TestFold::Fails) => {
                    return Some(format!(
                        "`{t}` is false or not well-formed for every value its expansions can take"
                    ));
                }
                Some(TestFold::Malformed) => {
                    return Some(format!(
                        "`{t}` is not a well-formed test and always has status 2"
                    ));
                }
                _ => {}
            }
            if export_fails(s) {
                return Some(format!("`{t}` exports a name that is not valid and fails"));
            }
            if assign_only_fails(s, src) {
                return Some(format!(
                    "`{t}` only assigns variables, and its last command substitution always fails"
                ));
            }
            None
        }
        ast::Command::For(f) if some_item(f) => list_fails(&f.body, src).map(|why| {
            format!("the `for` loop runs its body, which always ends with a failure ({why})")
        }),
        _ => None,
    }
}

/// KS-R001's question for the right-hand side of `||` (which runs only after
/// a failure): why it cannot fail. Besides a status that is always 0, a
/// pipeline with `true` or `:` as a stage is refused here: the stage does no
/// work whose failure could be reported, whatever the other stages do. (Its
/// status is not always 0, so inside a loop body it does not count as 0.)
pub(crate) fn rhs_zero(p: &ast::Pipeline, src: &str) -> Option<String> {
    if !p.negated && p.commands.len() > 1 {
        for c in &p.commands {
            if let Some(n) = simple(c).and_then(is_true) {
                return Some(format!(
                    "the pipeline `{}` has `{n}` as a stage: it does no work whose failure the \
                     status could report, and the stages after it read nothing",
                    text(src, p.span)
                ));
            }
        }
    }
    pipeline_zero(p, Prev::Fails, src)
}

/// Why the pipeline `p` always has status 0, given the status before it.
fn pipeline_zero(p: &ast::Pipeline, prev: Prev, src: &str) -> Option<String> {
    if p.negated {
        // `! exit 0` exits before the `!` applies.
        if let [c] = p.commands.as_slice()
            && let Some(why) = command_exits_zero(c, prev, src)
        {
            return Some(why);
        }
        let multi = p.commands.len() > 1;
        return command_fails(p.commands.last()?, prev, multi, src).map(|why| {
            format!(
                "`{}` negates a command that always fails ({why})",
                text(src, p.span)
            )
        });
    }
    // In a pipeline of two or more the last stage is its own scope: an `exit`
    // there ends the stage with its status (a bare one: the status before
    // the pipeline), which is the pipeline's.
    command_zero(p.commands.last()?, prev, src)
}

/// Why the pipeline always has a non-zero status (and does not exit).
fn pipeline_fails(p: &ast::Pipeline, prev: Prev, src: &str) -> Option<String> {
    let multi = p.commands.len() > 1;
    if !multi && command_exits_zero(&p.commands[0], prev, src).is_some() {
        return None;
    }
    if p.negated {
        return pipeline_zero(
            &ast::Pipeline {
                negated: false,
                commands: p.commands.clone(),
                span: p.span,
            },
            prev,
            src,
        );
    }
    command_fails(p.commands.last()?, prev, multi, src)
}

/// What is known about a pipeline's status.
fn pipeline_prev(p: &ast::Pipeline, prev: Prev, src: &str) -> Prev {
    if pipeline_zero(p, prev, src).is_some() {
        Prev::Zero
    } else if pipeline_fails(p, prev, src).is_some() {
        Prev::Fails
    } else {
        Prev::Unknown
    }
}

/// Why an and-or list always has status 0.
fn and_or_zero(item: &ast::AndOrList, prev: Prev, src: &str) -> Option<String> {
    let mut why = pipeline_zero(&item.first, prev, src);
    for (op, p) in &item.rest {
        why = match op {
            // `a && b` runs b only after a succeeded: 0 only if both are.
            ast::AndOrOp::And => why.and_then(|_| pipeline_zero(p, Prev::Zero, src)),
            // `a || b`: 0 if a always is (b never runs), else b decides.
            ast::AndOrOp::Or => why.or_else(|| pipeline_zero(p, Prev::Fails, src)),
        };
    }
    why
}

/// One and-or list, run in order: whether it certainly exits with status 0,
/// whether it may exit, and what is known about its status after.
struct Walk {
    exits_zero: Option<String>,
    exits_nonzero: bool,
    may_exit: bool,
    after: Prev,
}

fn walk(item: &ast::AndOrList, prev: Prev, src: &str) -> Walk {
    let mut state = prev;
    let mut may_exit = false;
    let pipes =
        std::iter::once((None, &item.first)).chain(item.rest.iter().map(|(op, p)| (Some(*op), p)));
    for (op, p) in pipes {
        // Whether the pipeline runs: certainly, maybe, or never; and the
        // status before it when it runs.
        let (certain, before) = match (op, state) {
            (None, _) => (true, prev),
            (Some(ast::AndOrOp::And), Prev::Fails) | (Some(ast::AndOrOp::Or), Prev::Zero) => {
                continue;
            }
            (Some(ast::AndOrOp::And), s) => (s == Prev::Zero, Prev::Zero),
            (Some(ast::AndOrOp::Or), s) => (s == Prev::Fails, Prev::Fails),
        };
        if let [c] = p.commands.as_slice() {
            if certain && let Some(why) = command_exits_zero(c, before, src) {
                return Walk {
                    exits_zero: Some(why),
                    exits_nonzero: false,
                    may_exit: true,
                    after: Prev::Zero,
                };
            }
            if certain && command_exits_nonzero(c, before, src) {
                return Walk {
                    exits_zero: None,
                    exits_nonzero: true,
                    may_exit: true,
                    after: Prev::Fails,
                };
            }
            may_exit |= command_may_exit(c, src);
        }
        let after = pipeline_prev(p, before, src);
        state = if certain {
            after
        } else {
            // Skipped, the status is the one that skipped it.
            match (op, after) {
                (Some(ast::AndOrOp::And), Prev::Fails) => Prev::Fails,
                (Some(ast::AndOrOp::Or), Prev::Zero) => Prev::Zero,
                _ => Prev::Unknown,
            }
        };
    }
    // `echo` / `printf` count as 0 (in practice), for a bare `exit` after them.
    if item.rest.is_empty()
        && !item.first.negated
        && let [ast::Command::Simple(s)] = item.first.commands.as_slice()
        && is_echo_like(s)
    {
        state = Prev::Zero;
    }
    Walk {
        exits_zero: None,
        exits_nonzero: false,
        may_exit,
        after: state,
    }
}

/// True when running the list may run an `exit` that ends the script (or
/// the enclosing capture). A stage of a pipeline of two or more is its own
/// scope, so an `exit` there ends only the stage; a loop body that never
/// runs and a pipeline that `&&` / `||` skip run no `exit`.
fn list_may_exit(items: &[ast::AndOrList], src: &str) -> bool {
    let mut prev = Prev::Unknown;
    for item in items {
        let w = walk(item, prev, src);
        if w.may_exit {
            return true;
        }
        prev = w.after;
    }
    false
}

/// Why running the list certainly ends the script with status 0.
fn list_exits_zero(items: &[ast::AndOrList], src: &str) -> Option<String> {
    let mut prev = Prev::Unknown;
    for item in items {
        let w = walk(item, prev, src);
        if w.exits_zero.is_some() || w.may_exit {
            return w.exits_zero;
        }
        prev = w.after;
    }
    None
}

/// Whether running the list certainly exits: Some(true) with status 0,
/// Some(false) with a non-zero status; None when it may not exit.
fn list_exits(items: &[ast::AndOrList], src: &str) -> Option<bool> {
    let mut prev = Prev::Unknown;
    for item in items {
        let w = walk(item, prev, src);
        if w.exits_zero.is_some() {
            return Some(true);
        }
        if w.exits_nonzero {
            return Some(false);
        }
        if w.may_exit {
            return None;
        }
        prev = w.after;
    }
    None
}

/// Why a command list (a loop body, a capture) always ends with status 0: an
/// item that certainly exits with status 0 ends it, or no item may exit and
/// the last one always has status 0.
fn list_zero(items: &[ast::AndOrList], src: &str) -> Option<String> {
    let mut prev = Prev::Unknown;
    let mut why = None;
    for item in items {
        let w = walk(item, prev, src);
        // Nothing after an unconditional exit runs; `cat a || exit 1; true`
        // can end with status 1.
        if w.exits_zero.is_some() || w.may_exit {
            return w.exits_zero;
        }
        why = and_or_zero(item, prev, src);
        prev = w.after;
    }
    why
}

/// Why a command list (a loop condition, a body) always ends with a non-zero
/// status, without exiting.
fn list_fails(items: &[ast::AndOrList], src: &str) -> Option<String> {
    let mut prev = Prev::Unknown;
    for item in items {
        let w = walk(item, prev, src);
        if w.may_exit {
            return None;
        }
        prev = w.after;
    }
    let last = items.last()?;
    if prev != Prev::Fails {
        return None;
    }
    Some(if last.rest.is_empty() {
        let mut before = Prev::Unknown;
        for item in &items[..items.len() - 1] {
            before = walk(item, before, src).after;
        }
        pipeline_fails(&last.first, before, src)
            .unwrap_or_else(|| format!("`{}` always fails", text(src, last.span)))
    } else {
        format!("`{}` always fails", text(src, last.span))
    })
}

/// For tests: the status of `test ARGS` with literal arguments, through the
/// shared evaluator.
#[cfg(test)]
pub(crate) fn const_test(args: &[&str]) -> Option<i32> {
    let a: Vec<TestArg> = args.iter().map(|s| TestArg::Known(s)).collect();
    sem::eval_test(&a, false, &mut |_, _| None).status()
}
