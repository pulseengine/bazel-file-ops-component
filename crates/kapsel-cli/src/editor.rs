//! The line editor of the interactive shell (REQ-KS-019), used only when
//! stdin and stdout are both terminals. Everything else (pipes, scripts,
//! `-c`, tests) keeps the plain `read_line` path in `main.rs`.
//!
//! The pieces are small and pure so they can be tested without a terminal:
//! a validator (`is_incomplete`), a highlighter and a hinter (both run
//! `parse` and `check` on the line; nothing executes), a completer, and the
//! history file.

use std::borrow::Cow;
use std::io::IsTerminal;
use std::path::PathBuf;

use nu_ansi_term::{Color, Style};
use reedline::{
    ColumnarMenu, Completer, CompletionResult, Emacs, FileBackedHistory, Highlighter, Hinter,
    History, KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, StyledText, Suggestion, ValidationResult,
    Validator, default_emacs_keybindings,
};

/// History entries kept on disk and in memory.
pub const HISTORY_CAP: usize = 5_000;

/// Lines longer than this are highlighted from the parse alone, without the
/// check stage, so a keystroke stays cheap on a pasted script.
pub const MAX_CHECK_BYTES: usize = 4096;

/// What one read produced.
pub enum Read {
    /// A complete command (possibly several lines), with a trailing newline.
    Line(String),
    /// Ctrl-C: the line is discarded, the session goes on.
    Interrupted,
    /// Ctrl-D on an empty line.
    Eof,
    /// The terminal failed; the caller falls back to the plain reader.
    Unavailable,
}

pub struct Editor {
    rl: Reedline,
    prompt: KapselPrompt,
    cwd: std::sync::Arc<std::sync::Mutex<PathBuf>>,
}

impl Editor {
    /// The editor, or `None` when stdin or stdout is not a terminal, the
    /// terminal is `dumb`, or the editor cannot be set up. `None` means: use
    /// the plain reader.
    pub fn new() -> Option<Editor> {
        if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
            return None;
        }
        if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
            return None;
        }
        let history: Box<dyn History> = match history_path() {
            Some(p) => Box::new(open_history(&p).ok()?),
            None => Box::new(FileBackedHistory::new(HISTORY_CAP).ok()?),
        };
        let mut kb = default_emacs_keybindings();
        kb.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu("completion_menu".into()),
                ReedlineEvent::MenuNext,
            ]),
        );
        let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
        let cwd = std::sync::Arc::new(std::sync::Mutex::new(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ));
        let rl = Reedline::create()
            .with_history(history)
            .with_validator(Box::new(KapselValidator))
            .with_highlighter(Box::new(KapselHighlighter))
            .with_hinter(Box::new(RuleHinter))
            .with_completer(Box::new(KapselCompleter { cwd: cwd.clone() }))
            .with_menu(ReedlineMenu::EngineCompleter(Box::new(
                ColumnarMenu::default().with_name("completion_menu"),
            )))
            .with_edit_mode(Box::new(Emacs::new(kb)))
            .with_ansi_colors(color);
        Some(Editor {
            rl,
            prompt: KapselPrompt,
            cwd,
        })
    }

    /// Tells the completer where the session is, before each prompt.
    pub fn set_cwd(&mut self, dir: PathBuf) {
        if let Ok(mut d) = self.cwd.lock() {
            *d = dir;
        }
    }

    pub fn read(&mut self) -> Read {
        match self.rl.read_line(&self.prompt) {
            Ok(Signal::Success(mut s)) => {
                s.push('\n');
                Read::Line(s)
            }
            Ok(Signal::CtrlC(_)) => Read::Interrupted,
            Ok(Signal::CtrlD) => Read::Eof,
            Ok(_) => Read::Interrupted,
            Err(_) => Read::Unavailable,
        }
    }
}

// ---- history ----------------------------------------------------------------

/// `$KAPSEL_HOME/history`; without it `~/.config/kapsel/history` (beside the
/// keys). `None` when neither is set: the history then lives in memory.
pub fn history_path() -> Option<PathBuf> {
    let set = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    if let Some(h) = set("KAPSEL_HOME") {
        return Some(PathBuf::from(h).join("history"));
    }
    set("HOME").map(|h| PathBuf::from(h).join(".config/kapsel/history"))
}

/// Creates the file with mode 0600 (and narrows an existing one), then opens
/// it as a capped history. A refused line is saved like any other, so it can
/// be recalled and fixed.
pub fn open_history(path: &std::path::Path) -> std::io::Result<FileBackedHistory> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        o.mode(0o600);
        o.open(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    o.open(path)?;
    FileBackedHistory::with_file(HISTORY_CAP, path.to_path_buf())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

// ---- prompt -----------------------------------------------------------------

struct KapselPrompt;

impl Prompt for KapselPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("kapsel")
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("$ ")
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }
    fn render_prompt_history_search_indicator(&self, h: PromptHistorySearch) -> Cow<'_, str> {
        Cow::Owned(format!("(search: {}) ", h.term))
    }
    fn get_prompt_color(&self) -> Color {
        Color::Default
    }
    fn get_indicator_color(&self) -> Color {
        Color::Default
    }
    fn get_prompt_multiline_color(&self) -> Color {
        Color::Default
    }
}

// ---- validator --------------------------------------------------------------

/// Enter on an unfinished construct continues the line.
pub struct KapselValidator;

impl Validator for KapselValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if kapsel_syntax::is_incomplete(line) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

// ---- analysis (highlighter and hinter) ---------------------------------------

/// How bad a stretch of the line is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Clean,
    Warn,
    Refuse,
}

/// The findings of the check stage as `(start, end, level, rule)`, byte
/// offsets into `line`. An unfinished line has none (it is still being
/// typed); an over-long line is parsed but not checked, so only its parse
/// errors show.
pub fn findings(line: &str) -> Vec<(usize, usize, Level, String)> {
    if line.trim().is_empty() || kapsel_syntax::is_incomplete(line) {
        return Vec::new();
    }
    let outcome = match kapsel_syntax::parse(line) {
        Err(errs) => kapsel_check::refuse_parse_errors(line, &errs),
        Ok(_) if line.len() > MAX_CHECK_BYTES => return Vec::new(),
        Ok(script) => kapsel_check::check(line, &script),
    };
    let fs = match outcome {
        kapsel_plan::CheckOutcome::Refused(r) => r.findings,
        kapsel_plan::CheckOutcome::Plan(p) => p.warnings,
    };
    fs.into_iter()
        .map(|f| {
            let lvl = match f.severity {
                kapsel_plan::Severity::Refuse => Level::Refuse,
                kapsel_plan::Severity::Warn => Level::Warn,
            };
            (f.span.start as usize, f.span.end as usize, lvl, f.rule)
        })
        .collect()
}

/// The line cut into runs of equal level: `(start, end, level)`, covering
/// the whole line. Refuse wins over Warn where spans overlap.
pub fn runs(line: &str) -> Vec<(usize, usize, Level)> {
    let mut per = vec![Level::Clean; line.len()];
    for (s, e, lvl, _) in findings(line) {
        let e = e.min(line.len());
        for slot in per.iter_mut().take(e).skip(s.min(e)) {
            *slot = (*slot).max(lvl);
        }
    }
    let mut out: Vec<(usize, usize, Level)> = Vec::new();
    let mut i = 0;
    while i < line.len() {
        // Never split inside a character.
        let mut j = i + 1;
        while j < line.len() && (per[j] == per[i] || !line.is_char_boundary(j)) {
            j += 1;
        }
        out.push((i, j, per[i]));
        i = j;
    }
    out
}

/// Refused spans red and underlined, warned spans yellow, the rest default.
pub struct KapselHighlighter;

impl Highlighter for KapselHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut st = StyledText::new();
        for (s, e, lvl) in runs(line) {
            let style = match lvl {
                Level::Clean => Style::new(),
                Level::Warn => Style::new().fg(Color::Yellow),
                Level::Refuse => Style::new().fg(Color::Red).underline(),
            };
            st.push((style, line[s..e].to_string()));
        }
        st
    }
}

/// A muted rule id after a line that will be refused.
#[derive(Default)]
pub struct RuleHinter;

/// The text of the hint for `line`, if it will be refused.
pub fn hint_text(line: &str) -> Option<String> {
    findings(line)
        .into_iter()
        .find(|f| f.2 == Level::Refuse)
        .map(|f| format!("  {}", f.3))
}

impl Hinter for RuleHinter {
    fn handle(
        &mut self,
        line: &str,
        _pos: usize,
        _history: &dyn History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        match hint_text(line) {
            Some(t) if use_ansi_coloring => Style::new().fg(Color::DarkGray).paint(t).to_string(),
            Some(t) => t,
            None => String::new(),
        }
    }
    // Nothing to accept: the hint is information, not text.
    fn complete_hint(&self) -> String {
        String::new()
    }
    fn next_hint_token(&self) -> String {
        String::new()
    }
}

// ---- completer --------------------------------------------------------------

/// Words that leave the next word in command position.
const OPENERS: &[&str] = &[
    "if", "then", "elif", "else", "while", "until", "do", "!", "{", "time",
];

/// What is being completed at the end of `prefix`.
#[derive(Debug, PartialEq, Eq)]
pub enum Slot<'a> {
    /// A command name, with the typed prefix.
    Command(&'a str),
    /// A flag of built-in `tool`, with the typed prefix (starts with `-`).
    Flag(&'a str, &'a str),
    /// A file name, with the typed prefix (may hold a directory part).
    File(&'a str),
}

pub fn slot(prefix: &str) -> Slot<'_> {
    let is_sep = |c: char| c.is_whitespace() || ";|&(){}".contains(c);
    let cur_at = prefix.rfind(is_sep).map_or(0, |i| {
        i + prefix[i..].chars().next().map_or(1, char::len_utf8)
    });
    let (before, cur) = prefix.split_at(cur_at);
    // The command being typed starts after the last separator that ends a command.
    let seg_at = before
        .rfind([';', '|', '&', '(', '\n'])
        .map_or(0, |i| i + 1);
    let mut words = before[seg_at..].split_whitespace().peekable();
    while words.peek().is_some_and(|w| OPENERS.contains(w)) {
        words.next();
    }
    match words.next() {
        None => Slot::Command(cur),
        Some(tool) if cur.starts_with('-') => Slot::Flag(tool, cur),
        Some(_) => Slot::File(cur),
    }
}

/// Completes against the session's working directory, which kapsel's `cd`
/// changes without changing the process's.
pub struct KapselCompleter {
    cwd: std::sync::Arc<std::sync::Mutex<PathBuf>>,
}

/// Suggestions for the word ending at byte `pos` of `line`; `dir` is where
/// file names come from.
pub fn complete_in(line: &str, pos: usize, dir: &std::path::Path) -> Vec<Suggestion> {
    let pos = pos.min(line.len());
    if !line.is_char_boundary(pos) {
        return Vec::new();
    }
    let prefix = &line[..pos];
    let mk = |value: String, start: usize, space: bool| Suggestion {
        value,
        span: Span::new(start, pos),
        append_whitespace: space,
        ..Suggestion::default()
    };
    match slot(prefix) {
        Slot::Command(cur) => {
            let start = pos - cur.len();
            kapsel_plan::BUILTINS
                .iter()
                .filter(|b| b.starts_with(cur) && **b != "[")
                .map(|b| mk((*b).to_string(), start, true))
                .collect()
        }
        Slot::Flag(tool, cur) => {
            let start = pos - cur.len();
            kapsel_check::builtin_flag_names(tool)
                .into_iter()
                .filter(|f| f.starts_with(cur))
                .map(|f| mk(f, start, true))
                .collect()
        }
        Slot::File(cur) => {
            let start = pos - cur.len();
            let (dpart, name) = cur.rsplit_once('/').map_or(("", cur), |(d, n)| (d, n));
            // An empty directory part before a slash is the root.
            let base = if !cur.contains('/') {
                dir.to_path_buf()
            } else if dpart.is_empty() {
                PathBuf::from("/")
            } else {
                dir.join(dpart)
            };
            let Ok(rd) = std::fs::read_dir(&base) else {
                return Vec::new();
            };
            let mut out: Vec<Suggestion> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().into_string().ok()?;
                    // A name with control characters would be printed raw in the
                    // completion menu, where an escape sequence acts on the terminal.
                    if n.chars().any(char::is_control) {
                        return None;
                    }
                    if !n.starts_with(name) || (n.starts_with('.') && !name.starts_with('.')) {
                        return None;
                    }
                    let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                    let mut v = String::new();
                    if cur.contains('/') {
                        v.push_str(dpart);
                        v.push('/');
                    }
                    v.push_str(&escape(&n));
                    if is_dir {
                        v.push('/');
                    }
                    Some(mk(v, start, !is_dir))
                })
                .collect();
            out.sort_by(|a, b| a.value.cmp(&b.value));
            out.truncate(200);
            out
        }
    }
}

/// Backslash-escapes the characters the shell would otherwise act on.
fn escape(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_whitespace() || "'\"\\$`&;|<>()*?[]{}#~!".contains(c) {
            s.push('\\');
        }
        s.push(c);
    }
    s
}

impl Completer for KapselCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        let dir = self
            .cwd
            .lock()
            .map(|d| d.clone())
            .unwrap_or_else(|_| PathBuf::from("."));
        CompletionResult::fresh(complete_in(line, pos, &dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(line: &str, dir: &std::path::Path) -> Vec<String> {
        complete_in(line, line.len(), dir)
            .into_iter()
            .map(|s| s.value)
            .collect()
    }

    #[test]
    fn highlighter_marks_refused_warned_and_clean_lines() {
        // Refused: `|| true` is KS-R001.
        let r = runs("echo x || true");
        assert!(r.iter().any(|x| x.2 == Level::Refuse), "{r:?}");
        let (s, e, _) = *r.iter().find(|x| x.2 == Level::Refuse).unwrap();
        assert_eq!(&"echo x || true"[s..e], "|| true");
        assert_eq!(hint_text("echo x || true").as_deref(), Some("  KS-R001"));

        // Clean: one run, default style.
        assert_eq!(runs("echo hi"), vec![(0, 7, Level::Clean)]);
        assert_eq!(hint_text("echo hi"), None);

        // The styled text: refused red+underline, clean default.
        let st = KapselHighlighter.highlight("echo x || true", 0);
        assert!(
            st.buffer
                .iter()
                .any(|(s, _)| s.foreground == Some(Color::Red) && s.is_underline)
        );
        let st = KapselHighlighter.highlight("echo hi", 0);
        assert_eq!(st.buffer.len(), 1);
        assert_eq!(st.buffer[0].0, Style::new());
        assert_eq!(st.raw_string(), "echo hi");
    }

    #[test]
    fn highlighter_marks_warned_lines() {
        // Find a line that the check stage warns about without refusing.
        let warned = [
            "cd /tmp",
            "echo a; echo a",
            "ls x; ls x",
            "export A=1",
            "sleep 1",
            "cat f | cat",
            "cd /tmp && pwd",
        ];
        let hit = warned
            .iter()
            .find(|l| runs(l).iter().any(|x| x.2 == Level::Warn))
            .unwrap_or_else(|| panic!("no warn sample warns; adjust the list"));
        let st = KapselHighlighter.highlight(hit, 0);
        assert!(
            st.buffer
                .iter()
                .any(|(s, _)| s.foreground == Some(Color::Yellow))
        );
        assert_eq!(hint_text(hit), None, "a warning is not a refusal");
        assert_eq!(st.raw_string(), **hit);
    }

    #[test]
    fn unfinished_and_long_lines_are_not_coloured_wrongly() {
        assert!(findings("for x in a b; do").is_empty());
        let long = format!("echo {}", "a".repeat(MAX_CHECK_BYTES + 10));
        assert_eq!(runs(&long).len(), 1);
    }

    #[test]
    fn validator_agrees_with_is_incomplete() {
        for l in [
            "echo hi",
            "for x in a b; do",
            "for x in a b; do echo $x; done",
            "if true; then",
            "echo 'open",
            "",
            "a |",
            "a && ",
        ] {
            let incomplete = matches!(KapselValidator.validate(l), ValidationResult::Incomplete);
            assert_eq!(incomplete, kapsel_syntax::is_incomplete(l), "{l:?}");
        }
        assert!(matches!(
            KapselValidator.validate("for x in a; do"),
            ValidationResult::Incomplete
        ));
    }

    #[test]
    fn completes_builtin_names_in_command_position() {
        let d = tempfile::tempdir().unwrap();
        let v = values("gre", d.path());
        assert_eq!(v, vec!["grep"]);
        let v = values("ec", d.path());
        assert!(v.contains(&"echo".to_string()));
        // After a pipe and after `do`.
        assert!(values("cat f | gre", d.path()).contains(&"grep".to_string()));
        assert!(values("for x in a; do ech", d.path()).contains(&"echo".to_string()));
        // Every built-in is offered for the empty prefix, except `[`.
        let all = values("", d.path());
        assert!(all.contains(&"cat".to_string()) && !all.contains(&"[".to_string()));
    }

    #[test]
    fn completes_flags_after_a_builtin() {
        let d = tempfile::tempdir().unwrap();
        let v = values("grep -", d.path());
        assert!(v.contains(&"-i".to_string()), "{v:?}");
        assert!(v.contains(&"--count".to_string()), "{v:?}");
        let v = values("grep --inv", d.path());
        assert_eq!(v, vec!["--invert-match"]);
        // The REQ-KS-063 tables are reused too.
        assert!(values("edit --al", d.path()).contains(&"--allow-none".to_string()));
        // No flags for an unknown tool.
        assert!(values("nosuch -", d.path()).is_empty());
    }

    #[test]
    fn completes_file_names_from_the_directory() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("alpha.txt"), "").unwrap();
        std::fs::write(d.path().join("my file"), "").unwrap();
        std::fs::write(d.path().join(".hidden"), "").unwrap();
        std::fs::create_dir(d.path().join("adir")).unwrap();
        std::fs::write(d.path().join("adir/inner"), "").unwrap();
        assert_eq!(values("cat al", d.path()), vec!["alpha.txt"]);
        assert_eq!(values("cat a", d.path()), vec!["adir/", "alpha.txt"]);
        assert_eq!(values("cat adir/i", d.path()), vec!["adir/inner"]);
        assert_eq!(values("cat my", d.path()), vec!["my\\ file"]);
        assert!(!values("cat ", d.path()).contains(&".hidden".to_string()));
        assert_eq!(values("cat .h", d.path()), vec![".hidden"]);
    }

    #[test]
    fn file_names_with_control_characters_are_never_offered() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("bad\u{1b}]0;owned\u{7}name"), "").unwrap();
        std::fs::write(d.path().join("bell\u{7}"), "").unwrap();
        std::fs::write(d.path().join("good"), "").unwrap();
        assert_eq!(values("cat ", d.path()), vec!["good"]);
        assert!(values("cat b", d.path()).is_empty());
    }

    #[test]
    fn suggestion_spans_replace_the_typed_word() {
        let d = tempfile::tempdir().unwrap();
        let s = complete_in("cat f | gre", 11, d.path());
        assert_eq!((s[0].span.start, s[0].span.end), (8, 11));
    }

    #[cfg(unix)]
    #[test]
    fn history_file_is_created_0600_under_the_given_path() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("home").join("history");
        let mut h = open_history(&p).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // A refused line is recorded like any other.
        h.save(reedline::HistoryItem::from_command_line("echo x || true"))
            .unwrap();
        h.sync().unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("echo x || true"), "{text:?}");
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // A wider existing file is narrowed.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        open_history(&p).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn history_path_follows_kapsel_home() {
        // Read-only on the environment: compare against what it says now.
        let want = std::env::var_os("KAPSEL_HOME")
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join("history"));
        if let Some(w) = want {
            assert_eq!(history_path(), Some(w));
        }
    }
}
