//! KS-R001's question about the right-hand side of `||`: can its status be
//! anything but 0? If it cannot, the `||` turns every failure of the command
//! before it into success, whatever the spelling (`|| true`, `|| true | cat`,
//! `|| ! false`, `|| exit 0`, `|| test 1`, a loop whose body ends in `:`).
//!
//! The analysis is syntactic and only says "always 0" when the constructs make
//! it so by their definition in POSIX sh; anything it cannot decide counts as
//! "can fail" here (the other rules and the run still apply).

use kapsel_ast as ast;

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

/// The value of a `test` / `[` whose operands are all literal, as POSIX
/// defines it for 0 to 4 arguments. None when it is not a constant or not a
/// well-formed expression (status 2).
pub(crate) fn const_test(args: &[String]) -> Option<bool> {
    let not = |v: Option<bool>| v.map(|b| !b);
    let int = |s: &str| s.trim().parse::<i64>().ok();
    match args {
        [] => Some(false),
        [a] => Some(!a.is_empty()),
        [op, a] => match op.as_str() {
            "!" => not(const_test(&args[1..])),
            "-n" => Some(!a.is_empty()),
            "-z" => Some(a.is_empty()),
            _ => None,
        },
        [a, op, b] => match op.as_str() {
            "=" => Some(a == b),
            "!=" => Some(a != b),
            "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
                let (x, y) = (int(a)?, int(b)?);
                Some(match op.as_str() {
                    "-eq" => x == y,
                    "-ne" => x != y,
                    "-lt" => x < y,
                    "-le" => x <= y,
                    "-gt" => x > y,
                    _ => x >= y,
                })
            }
            _ if a == "!" => not(const_test(&args[1..])),
            _ => None,
        },
        [a, ..] if args.len() == 4 && a == "!" => not(const_test(&args[1..])),
        _ => None,
    }
}

/// The constant value of a `test` / `[` command, if it has one.
fn test_value(s: &ast::SimpleCommand) -> Option<bool> {
    let words = literal_words(s)?;
    let (name, rest) = words.split_first()?;
    match name.as_str() {
        "test" => const_test(rest),
        "[" => match rest.split_last() {
            Some((close, args)) if close == "]" => const_test(args),
            _ => None,
        },
        _ => None,
    }
}

/// `exit N` with N a multiple of 256 (status 0), or a bare `exit` when the
/// status before it is known to be 0.
fn exit_zero(s: &ast::SimpleCommand, prev_zero: bool, src: &str) -> Option<String> {
    let words = literal_words(s)?;
    if words.first().map(String::as_str) != Some("exit") {
        return None;
    }
    let t = text(src, s.span);
    match words.get(1..) {
        Some([]) if prev_zero => Some(format!(
            "`{t}` with no operand exits with the status of the command before it, which is 0"
        )),
        Some([n]) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            let v: u128 = n.parse().ok()?;
            match v {
                0 => Some(format!("`{t}` always exits with status 0")),
                v if v % 256 == 0 => Some(format!(
                    "`{t}` exits with status 0 (an exit status is taken modulo 256)"
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Why `c` always has status 0, given whether the status before it is 0.
fn command_zero(c: &ast::Command, prev_zero: bool, src: &str) -> Option<String> {
    match c {
        ast::Command::Simple(s) => {
            if let Some(n) = is_true(s) {
                return Some(format!("`{n}` always has status 0"));
            }
            if let Some(why) = exit_zero(s, prev_zero, src) {
                return Some(why);
            }
            if test_value(s) == Some(true) {
                return Some(format!(
                    "`{}` tests constants and is always true",
                    text(src, s.span)
                ));
            }
            None
        }
        ast::Command::For(f) => list_zero(&f.body, src).map(|why| {
            format!(
                "the `for` loop's body always ends with status 0 ({why}), and a loop with no \
                 items has status 0 too"
            )
        }),
        ast::Command::While(w) => {
            let kw = if w.until { "until" } else { "while" };
            // `while <always fails>` / `until <always 0>`: the body never runs.
            let never_runs = if w.until {
                list_zero(&w.condition, src)
            } else {
                list_fails(&w.condition, src)
            };
            if let Some(why) = never_runs {
                return Some(format!(
                    "the `{kw}` loop's body never runs ({why}), so the loop's status is 0"
                ));
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

/// Why the pipeline `p` always has status 0.
pub(crate) fn pipeline_zero(p: &ast::Pipeline, prev_zero: bool, src: &str) -> Option<String> {
    if p.negated {
        return stages_fail(&p.commands, src).map(|why| {
            format!(
                "`{}` negates a command that always fails ({why})",
                text(src, p.span)
            )
        });
    }
    if p.commands.len() > 1 {
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
    let single = p.commands.len() == 1;
    command_zero(p.commands.last()?, prev_zero && single, src)
}

/// Why the stages of a (non-negated) pipeline always fail: the last one does.
fn stages_fail(commands: &[ast::Command], src: &str) -> Option<String> {
    let s = simple(commands.last()?)?;
    if name_of(s).as_deref() == Some("false") {
        return Some("`false` always has status 1".to_string());
    }
    if test_value(s) == Some(false) {
        return Some(format!(
            "`{}` tests constants and is always false",
            text(src, s.span)
        ));
    }
    None
}

fn pipeline_fails(p: &ast::Pipeline, src: &str) -> Option<String> {
    if p.negated {
        return pipeline_zero(
            &ast::Pipeline {
                negated: false,
                commands: p.commands.clone(),
                span: p.span,
            },
            false,
            src,
        );
    }
    stages_fail(&p.commands, src)
}

/// Why an and-or list always has status 0.
fn and_or_zero(item: &ast::AndOrList, prev_zero: bool, src: &str) -> Option<String> {
    let mut why = pipeline_zero(&item.first, prev_zero, src);
    for (op, p) in &item.rest {
        why = match op {
            // `a && b` runs b only after a succeeded: 0 only if both are.
            ast::AndOrOp::And => why.and_then(|_| pipeline_zero(p, true, src)),
            // `a || b`: 0 if a always is (b never runs), else b decides.
            ast::AndOrOp::Or => why.or_else(|| pipeline_zero(p, false, src)),
        };
    }
    why
}

/// Why a command list (a loop body) always ends with status 0: its last item
/// always has status 0, or an unconditional `exit` that exits 0 ends it.
fn list_zero(items: &[ast::AndOrList], src: &str) -> Option<String> {
    let mut prev_zero = false;
    let mut why = None;
    for item in items {
        let lone = (item.rest.is_empty() && !item.first.negated && item.first.commands.len() == 1)
            .then(|| simple(&item.first.commands[0]))
            .flatten();
        if let Some(s) = lone
            && name_of(s).as_deref() == Some("exit")
        {
            // Nothing after an unconditional exit runs.
            return exit_zero(s, prev_zero, src);
        }
        why = and_or_zero(item, prev_zero, src);
        prev_zero = why.is_some() || lone.is_some_and(is_echo_like);
    }
    why
}

/// Why a command list (a loop condition) always ends with a non-zero status.
fn list_fails(items: &[ast::AndOrList], src: &str) -> Option<String> {
    let last = items.last()?;
    if !last.rest.is_empty() {
        return None;
    }
    pipeline_fails(&last.first, src)
}
