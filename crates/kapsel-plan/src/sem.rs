//! The meaning of `test` / `[`, of `exit`'s operand and of a valid variable
//! name, written once. The host (`kapsel-host`) runs these built-ins with this
//! code; the check stage (`kapsel-check`) folds them with the same code when
//! every argument is known before the run (KS-R001, KS-R018). Two
//! implementations drifted apart once (the second v0.3.0 review: `exit -256`,
//! `test x -o y`), so there is one.
//!
//! Pure: no I/O. The file tests (`-e`, `-f`, ...) go through a callback the
//! caller supplies; the check stage answers "unknown".

/// The status `exit OPERAND` exits with, or None when the operand is not a
/// number (the host then reports "numeric argument required" and exits 2).
/// Surrounding white space is ignored, a sign is allowed, and the value is
/// taken modulo 256: `exit -256`, `exit +0` and `exit " 0"` all exit 0.
pub fn exit_status(operand: &str) -> Option<u8> {
    operand
        .trim()
        .parse::<i64>()
        .ok()
        .map(|n| n.rem_euclid(256) as u8)
}

/// A name `export` and assignments accept: a letter or `_`, then letters,
/// digits and `_`.
pub fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic() || x == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// One argument of `test` as the evaluator sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestArg<'a> {
    /// Its value.
    Known(&'a str),
    /// A single field whose value is only known at run time but is never
    /// empty (it has a literal character). `may_be_operator` is false when
    /// the value cannot be an operator of `test` either (it starts with a
    /// literal character that no operator starts with, see
    /// [`operator_free_start`]); an argument that may be an operator makes the
    /// parse itself depend on the run, and the result is
    /// [`TestOutcome::Unknown`].
    NonEmpty { may_be_operator: bool },
    /// A single field whose value is only known at run time, starts with
    /// `-` and is not an operator of `test`: two characters long (`short`,
    /// an unsupported unary operator) or longer.
    Dash { short: bool },
}

/// True when a value starting with `c` cannot be an operator of `test`
/// (`!`, `(`, `)`, `-x`, `=`, `==`, `!=`, `<`, `>`).
pub fn operator_free_start(c: char) -> bool {
    !matches!(c, '-' | '!' | '(' | ')' | '=' | '<' | '>')
}

/// The result of `test`: its status, or that it depends on the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// Status 0.
    True,
    /// Status 1.
    False,
    /// Status 2, with the message the host prints.
    Error(String),
    /// Only the run can tell (an argument or a file test is not known).
    Unknown,
}

impl TestOutcome {
    /// The exit status, if known.
    pub fn status(&self) -> Option<i32> {
        match self {
            TestOutcome::True => Some(0),
            TestOutcome::False => Some(1),
            TestOutcome::Error(_) => Some(2),
            TestOutcome::Unknown => None,
        }
    }
}

/// The unary operators `test` supports.
pub const TEST_UNARY: &[&str] = &[
    "-e", "-f", "-d", "-r", "-w", "-x", "-s", "-z", "-n", "-L", "-h",
];
/// The binary operators `test` supports.
pub const TEST_BINARY: &[&str] = &[
    "=", "==", "!=", "<", ">", "-eq", "-ne", "-lt", "-le", "-gt", "-ge",
];

/// The other words `test` treats as operators.
pub const TEST_OTHER: &[&str] = &["!", "-a", "-o", "(", ")"];

fn supported_ops() -> String {
    format!(
        "{} {} {}",
        TEST_UNARY.join(" "),
        TEST_BINARY.join(" "),
        TEST_OTHER.join(" ")
    )
}

/// Evaluates `test ARGS` (`bracket` false) or `[ ARGS ]` (`bracket` true,
/// the last argument must be `]`). `file_test(op, path)` answers a file
/// operator (`-e`, `-f`, `-d`, `-r`, `-w`, `-x`, `-s`, `-L`, `-h`) for a
/// known path; None means unknown.
///
/// The grammar: `-o` binds loosest, then `-a`, then `!`, then `( ... )`,
/// binary and unary primaries, and a lone word (true when non-empty). Both
/// sides of `-o` / `-a` are evaluated.
pub fn eval_test(
    args: &[TestArg<'_>],
    bracket: bool,
    file_test: &mut dyn FnMut(&str, &str) -> Option<bool>,
) -> TestOutcome {
    let mut t = args;
    if bracket {
        match t.split_last() {
            Some((TestArg::Known("]"), rest)) => t = rest,
            // `"]$X"` may be `]`.
            Some((TestArg::NonEmpty { .. }, _)) => return TestOutcome::Unknown,
            _ => return TestOutcome::Error("missing ']'".into()),
        }
    }
    if t.is_empty() {
        return TestOutcome::False;
    }
    let mut p = Parser {
        t,
        pos: 0,
        file_test,
    };
    match p.or() {
        Ok(v) if p.pos >= p.t.len() => match v {
            Some(true) => TestOutcome::True,
            Some(false) => TestOutcome::False,
            None => TestOutcome::Unknown,
        },
        Ok(_) => match p.t[p.pos] {
            TestArg::Known(left) => TestOutcome::Error(format!("{left}: unexpected argument")),
            // A word left over is an error whatever its value.
            _ => TestOutcome::Error("unexpected argument".into()),
        },
        Err(Stop::Error(e)) => TestOutcome::Error(e),
        Err(Stop::Unknown) => TestOutcome::Unknown,
    }
}

enum Stop {
    Error(String),
    /// The parse or an error depends on a value only the run knows.
    Unknown,
}

struct Parser<'a, 'b, 'c> {
    t: &'a [TestArg<'b>],
    pos: usize,
    file_test: &'c mut dyn FnMut(&str, &str) -> Option<bool>,
}

type R = Result<Option<bool>, Stop>;

impl Parser<'_, '_, '_> {
    /// Whether the token `k` ahead is exactly `s`.
    fn is(&self, k: usize, s: &str) -> Result<bool, Stop> {
        match self.t.get(self.pos + k) {
            None => Ok(false),
            Some(TestArg::Known(v)) => Ok(*v == s),
            Some(TestArg::NonEmpty {
                may_be_operator: false,
            })
            | Some(TestArg::Dash { .. }) => Ok(false),
            Some(TestArg::NonEmpty {
                may_be_operator: true,
            }) => Err(Stop::Unknown),
        }
    }

    /// The token `k` ahead as an operator candidate: its text, or None for a
    /// value that cannot be an operator.
    fn op(&self, k: usize) -> Result<Option<&str>, Stop> {
        match self.t.get(self.pos + k) {
            Some(TestArg::Known(v)) => Ok(Some(v)),
            // Stand-ins with the same shape: unsupported operators.
            Some(TestArg::Dash { short: true }) => Ok(Some("-?")),
            Some(TestArg::Dash { short: false }) => Ok(Some("-??")),
            Some(TestArg::NonEmpty {
                may_be_operator: true,
            }) => Err(Stop::Unknown),
            _ => Ok(None),
        }
    }

    fn rem(&self) -> usize {
        self.t.len() - self.pos
    }

    fn or(&mut self) -> R {
        let mut v = self.and()?;
        while self.is(0, "-o")? {
            self.pos += 1;
            let r = self.and()?;
            v = match (v, r) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            };
        }
        Ok(v)
    }

    fn and(&mut self) -> R {
        let mut v = self.not()?;
        while self.is(0, "-a")? {
            self.pos += 1;
            let r = self.not()?;
            v = match (v, r) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            };
        }
        Ok(v)
    }

    fn not(&mut self) -> R {
        if self.rem() > 1 && self.is(0, "!")? {
            self.pos += 1;
            return Ok(self.not()?.map(|b| !b));
        }
        self.primary()
    }

    fn primary(&mut self) -> R {
        let Some(&tok) = self.t.get(self.pos) else {
            return Err(Stop::Error("missing argument".into()));
        };
        if self.rem() > 1 && self.is(0, "(")? {
            self.pos += 1;
            let v = self.or()?;
            if !self.is(0, ")")? {
                return Err(Stop::Error("expected ')'".into()));
            }
            self.pos += 1;
            return Ok(v);
        }
        let rem = self.rem();
        if rem >= 3
            && let Some(op) = self.op(1)?
            && (op.starts_with('-') || TEST_BINARY.contains(&op))
        {
            let op = op.to_string();
            if TEST_BINARY.contains(&op.as_str()) {
                let rhs = self.t[self.pos + 2];
                self.pos += 3;
                return self.binary(tok, &op, rhs);
            }
            if !matches!(op.as_str(), "-a" | "-o") {
                return Err(Stop::Error(format!(
                    "unsupported operator '{op}'; supported: {}",
                    supported_ops()
                )));
            }
        }
        if rem >= 2
            && let Some(op) = self.op(0)?
            && op.starts_with('-')
            && op.len() == 2
            && !matches!(op, "-a" | "-o")
        {
            let op = op.to_string();
            if TEST_UNARY.contains(&op.as_str()) {
                let arg = self.t[self.pos + 1];
                self.pos += 2;
                return Ok(self.unary(&op, arg));
            }
            return Err(Stop::Error(format!(
                "unsupported operator '{op}'; supported: {}",
                supported_ops()
            )));
        }
        self.pos += 1;
        Ok(Some(match tok {
            TestArg::Known(v) => !v.is_empty(),
            TestArg::NonEmpty { .. } | TestArg::Dash { .. } => true,
        }))
    }

    fn unary(&mut self, op: &str, arg: TestArg<'_>) -> Option<bool> {
        match (op, arg) {
            ("-z", TestArg::Known(v)) => Some(v.is_empty()),
            ("-n", TestArg::Known(v)) => Some(!v.is_empty()),
            (_, TestArg::Known(path)) => (self.file_test)(op, path),
            // The other kinds are never empty.
            ("-z", _) => Some(false),
            ("-n", _) => Some(true),
            _ => None,
        }
    }

    fn binary(&mut self, l: TestArg<'_>, op: &str, r: TestArg<'_>) -> R {
        let (TestArg::Known(l), TestArg::Known(r)) = (l, r) else {
            // An integer comparison may fail to parse at run time.
            if op.starts_with('-') {
                return Err(Stop::Unknown);
            }
            // The empty string against a non-empty value: unequal, and less.
            let empty_left = match (l, r) {
                (TestArg::Known(""), _) => true,
                (_, TestArg::Known("")) => false,
                _ => return Ok(None),
            };
            return Ok(Some(match op {
                "=" | "==" => false,
                "!=" => true,
                "<" => empty_left,
                _ => !empty_left,
            }));
        };
        Ok(Some(match op {
            "=" | "==" => l == r,
            "!=" => l != r,
            "<" => l < r,
            ">" => l > r,
            _ => {
                let a: i64 = l
                    .trim()
                    .parse()
                    .map_err(|_| Stop::Error(format!("{l}: integer expression expected")))?;
                let b: i64 = r
                    .trim()
                    .parse()
                    .map_err(|_| Stop::Error(format!("{r}: integer expression expected")))?;
                match op {
                    "-eq" => a == b,
                    "-ne" => a != b,
                    "-lt" => a < b,
                    "-le" => a <= b,
                    "-gt" => a > b,
                    _ => a >= b,
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(a: &[&str]) -> TestOutcome {
        let v: Vec<TestArg> = a.iter().map(|s| TestArg::Known(s)).collect();
        eval_test(&v, false, &mut |_, _| None)
    }

    #[test]
    fn exit_status_matches_the_host_parse() {
        for (s, v) in [
            ("0", Some(0)),
            ("+0", Some(0)),
            ("-0", Some(0)),
            (" 0", Some(0)),
            ("-256", Some(0)),
            ("256", Some(0)),
            ("+65", Some(65)),
            ("-191", Some(65)),
            (" 78", Some(78)),
            ("x", None),
            ("", None),
        ] {
            assert_eq!(exit_status(s), v, "{s:?}");
        }
    }

    #[test]
    fn test_grammar_by_example() {
        use TestOutcome::*;
        assert_eq!(known(&[]), False);
        assert_eq!(known(&["x", "-o", "y"]), True);
        assert_eq!(known(&["x", "-a", ""]), False);
        assert_eq!(known(&["(", "x", ")"]), True);
        assert_eq!(known(&["a", "<", "b"]), True);
        assert_eq!(known(&["x", "-gt", "1"]).status(), Some(2));
        assert_eq!(known(&["-e", "a"]), Unknown);
        assert_eq!(known(&["-e", "a", "-o", "x"]), True);
        let ne = TestArg::NonEmpty {
            may_be_operator: false,
        };
        let any = TestArg::NonEmpty {
            may_be_operator: true,
        };
        assert_eq!(eval_test(&[ne], false, &mut |_, _| None), True);
        assert_eq!(eval_test(&[any], false, &mut |_, _| None), True);
        assert_eq!(
            eval_test(&[TestArg::Known("-n"), ne], false, &mut |_, _| None),
            True
        );
        assert_eq!(
            eval_test(&[any, TestArg::Known("x")], false, &mut |_, _| None),
            Unknown
        );
        assert_eq!(
            eval_test(
                &[ne, TestArg::Known("="), TestArg::Known("x")],
                false,
                &mut |_, _| None
            ),
            Unknown
        );
    }
}
