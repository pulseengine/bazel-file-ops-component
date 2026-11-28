//! Structured run errors (REQ-KS-060).
//!
//! A built-in that fails prints its GNU-style line on stderr, as before, and
//! the host also records a [`RunError`]: the `site` of the command that ran
//! (the id the component passed to the WIT `site` import), the built-in, a
//! stable [`ErrorClass`], the message and a help line. The CLI maps the site
//! to the command's span and renders a diagnostic. Errors are kept only when
//! the call's status is non-zero, so a line a built-in prints on success is
//! not reported as an error.

/// What kind of failure a run error is. [`ErrorClass::as_str`] is stable:
/// the CLI shows it as the diagnostic id `run:<class>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ErrorClass {
    /// A path outside every grant.
    Grants,
    /// A refusal to write through (or copy) a symlink, or a symlink loop.
    Symlink,
    /// A file or directory that does not exist.
    NotFound,
    /// The operating system refused access.
    Permission,
    /// An unknown flag, a missing or extra operand, a bad option value.
    Usage,
    /// A regular expression that does not compile.
    Regex,
    /// `edit` found no match (or not the count `-c` asked for).
    EditNoMatch,
    /// Input that is not valid JSON, YAML, TOML or UTF-8, or a VALUE that
    /// does not parse.
    Parse,
    /// Any other I/O error (is a directory, not a directory, ...).
    Io,
    /// Any other failure a built-in reports.
    Failed,
}

impl ErrorClass {
    /// Every class, in order.
    pub const ALL: &'static [ErrorClass] = &[
        ErrorClass::Grants,
        ErrorClass::Symlink,
        ErrorClass::NotFound,
        ErrorClass::Permission,
        ErrorClass::Usage,
        ErrorClass::Regex,
        ErrorClass::EditNoMatch,
        ErrorClass::Parse,
        ErrorClass::Io,
        ErrorClass::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorClass::Grants => "grants",
            ErrorClass::Symlink => "symlink",
            ErrorClass::NotFound => "not-found",
            ErrorClass::Permission => "permission",
            ErrorClass::Usage => "usage",
            ErrorClass::Regex => "regex",
            ErrorClass::EditNoMatch => "edit-no-match",
            ErrorClass::Parse => "parse",
            ErrorClass::Io => "io",
            ErrorClass::Failed => "failed",
        }
    }

    /// The class of an I/O error by its kind.
    pub fn of_io(e: &std::io::Error) -> ErrorClass {
        match e.kind() {
            std::io::ErrorKind::NotFound => ErrorClass::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorClass::Permission,
            _ => ErrorClass::Io,
        }
    }
}

/// One failure of a built-in or a grant check during a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunError {
    /// The `site` id of the command that failed: its index in
    /// `kapsel_plan::Plan::commands()`. `None` when no `site` was named
    /// (a component compiled before the import existed, or `call_direct`).
    pub site: Option<u32>,
    /// argv[0] of the call.
    pub builtin: String,
    pub class: ErrorClass,
    /// The line printed on stderr, without the trailing newline.
    pub message: String,
    /// The operand concerned (a path, a flag, a pattern), when there is one.
    pub operand: Option<String>,
    /// What to do, one line.
    pub help: String,
}

/// The help line for a class, given the built-in, the operand and, for a
/// usage error, the built-in's supported flags.
pub fn help_for(
    class: ErrorClass,
    builtin: &str,
    operand: Option<&str>,
    supported: Option<&str>,
) -> String {
    let op = |d: &str| operand.map_or_else(|| d.to_string(), |o| format!("`{o}`"));
    match class {
        ErrorClass::Grants => format!(
            "{} is outside the session's grants: run it inside a granted directory, \
             or add `--rw DIR` / `--ro DIR` to kapsel's command line",
            op("the path")
        ),
        ErrorClass::Symlink => format!(
            "kapsel does not write through or copy symlinks: name the file {} points to",
            op("the symlink")
        ),
        ErrorClass::NotFound => format!(
            "{} does not exist: check the path, or create it before this command",
            op("the path")
        ),
        ErrorClass::Permission => format!(
            "the file system refuses access to {}: check its permissions",
            op("the path")
        ),
        ErrorClass::Usage => match supported {
            Some(s) => format!("kapsel's {builtin} supports: {s}"),
            None => format!("check the arguments of `{builtin}` against its usage"),
        },
        ErrorClass::Regex => format!(
            "fix the pattern {}; escape a literal metacharacter with a backslash",
            op("")
        ),
        ErrorClass::EditNoMatch => "the search text must occur in the file exactly as written \
             (whitespace and case count); add `--allow-none` only if no match is intended"
            .to_string(),
        ErrorClass::Parse => format!(
            "fix the input at the line and column the message names, or pass a string \
             VALUE with `--str`; {builtin} never writes a file it cannot parse"
        ),
        ErrorClass::Io => format!(
            "the operating system refused the operation on {}",
            op("the operand")
        ),
        ErrorClass::Failed => {
            format!("the message says what `{builtin}` rejected; fix that operand or input")
        }
    }
}

/// For a regex error message from the `regex` crate (the pattern line with a
/// `^` marker under the error), `column N (`c`): reason`.
pub fn regex_position(err: &str) -> Option<String> {
    let lines: Vec<&str> = err.lines().collect();
    let caret = lines.iter().position(|l| {
        let t = l.trim();
        !t.is_empty() && t.chars().all(|c| c == '^' || c == '-')
    })?;
    let pat_line = *lines.get(caret.checked_sub(1)?)?;
    let indent = pat_line.len() - pat_line.trim_start().len();
    let col_bytes = lines[caret].find('^')?.checked_sub(indent)?;
    let pattern = &pat_line[indent..];
    let col = pattern.get(..col_bytes)?.chars().count() + 1;
    let ch = pattern.get(col_bytes..)?.chars().next();
    let reason = lines
        .iter()
        .find_map(|l| l.trim().strip_prefix("error: "))
        .unwrap_or("");
    let at = match ch {
        Some(c) => format!("column {col} (`{c}`)"),
        None => format!("column {col} (end of pattern)"),
    };
    Some(if reason.is_empty() {
        at
    } else {
        format!("{at}: {reason}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_has_a_help_line() {
        for c in ErrorClass::ALL {
            let h = help_for(*c, "cat", Some("x"), Some("-n"));
            assert!(!h.trim().is_empty(), "{c:?}");
        }
    }

    #[test]
    fn regex_position_reads_the_marker() {
        // Built at run time so clippy does not reject the deliberately bad pattern.
        let pat = format!("ab{}c", '(');
        let e = regex::Regex::new(&pat).unwrap_err().to_string();
        let p = regex_position(&e).unwrap();
        assert!(p.starts_with("column 3 (`(`)"), "{p} from {e}");
        assert!(p.contains("unclosed group"), "{p}");
    }
}
