//! Hand-written scanner and recursive-descent parser building the lossless
//! green tree. Scanning and parsing are one pass because shell lexing depends
//! on the grammar position (reserved words, assignments, heredocs).
//!
//! Invariants:
//! - every byte consumed is emitted as exactly one token, so the tree text is
//!   the input;
//! - every loop either consumes input or exits, and nesting is capped at
//!   [`MAX_DEPTH`], so no input hangs or overflows the stack;
//! - refused constructs are parsed structurally where cheap, so one input
//!   reports every independent error instead of cascading.

use crate::kinds::SyntaxKind::{self, *};
use kapsel_ast::{ParseError, Span};
use rowan::{GreenNode, GreenNodeBuilder};

/// Combined nesting cap for lists and commands.
const MAX_DEPTH: u32 = 100;

/// Reserved words that close an enclosing construct.
const CLOSERS: &[&str] = &["do", "done", "then", "elif", "else", "fi", "esac", "}"];

const RESERVED: &[&str] = &[
    "!", "{", "}", "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while",
    "until", "do", "done", "in", "function", "coproc", "[[",
];

pub(crate) struct Output {
    pub green: GreenNode,
    pub errors: Vec<ParseError>,
    pub incomplete: bool,
}

pub(crate) fn parse(src: &str) -> Output {
    let mut p = Parser {
        src,
        pos: 0,
        b: GreenNodeBuilder::new(),
        errors: Vec::new(),
        incomplete: false,
        aborted: false,
        pending: Vec::new(),
        depth: 0,
    };
    p.b.start_node(SCRIPT.into());
    p.list_body(true);
    for h in std::mem::take(&mut p.pending) {
        p.eof_syntax(
            format!(
                "the heredoc body for `{}` is missing; it starts on the line after the command",
                h.delim
            ),
            h.start,
            h.end,
            &[h.delim.as_str()],
            Some("put the body on the following lines, then a line containing only the delimiter"),
        );
    }
    if p.pos < src.len() {
        let n = src.len() - p.pos;
        p.token(ERROR_TEXT, n);
    }
    p.b.finish_node();
    let mut errors = p.errors;
    errors.sort_by_key(|e| (e.span.start, std::cmp::Reverse(e.span.end)));
    Output {
        green: p.b.finish(),
        errors,
        incomplete: p.incomplete && !p.aborted,
    }
}

struct Pending {
    delim: String,
    quoted: bool,
    strip_tabs: bool,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Word,
    DQuote,
    Heredoc,
}

fn is_meta(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
    )
}

/// A character that can appear in a reserved word or plain name run.
fn is_plain(c: char) -> bool {
    !is_meta(c) && !matches!(c, '\'' | '"' | '\\' | '$' | '`')
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Message and help for each refused construct.
fn refusal(construct: &str) -> (&'static str, &'static str) {
    match construct {
        "if" => (
            "`if` statements are not in kapsel's shell subset yet",
            "use `cmd && then-part || else-part` only when the else part cannot mask a failure",
        ),
        "case" => (
            "`case` statements are not in kapsel's shell subset",
            "compare with test: `[ \"$x\" = value ] && cmd`",
        ),
        "function-definition" => (
            "function definitions are not in kapsel's shell subset",
            "write the commands out where they are used, or put them in a script file",
        ),
        "subshell" => (
            "subshells `( ... )` are not in kapsel's shell subset",
            "write the commands without the parentheses; `cd dir && cmd` keeps a cd scoped to one line",
        ),
        "brace-group" => (
            "brace groups `{ ...; }` are not in kapsel's shell subset",
            "write the commands without the braces and redirect each one",
        ),
        "backquote" => (
            "backquote command substitution is not in kapsel's shell subset",
            "use $(...)",
        ),
        "background" => (
            "background jobs `&` are not in kapsel's shell subset",
            "background jobs escape the session; run it in the foreground",
        ),
        "param-operator" => (
            "parameter operators inside `${...}` are not in kapsel's shell subset",
            "assign the value first, then use plain ${name}",
        ),
        "arithmetic" => (
            "arithmetic `$(( ))` and `(( ))` is not in kapsel's shell subset",
            "compute the value outside the command and pass it as a literal",
        ),
        "process-substitution" => (
            "process substitution `<( )` and `>( )` is not in kapsel's shell subset",
            "write the output to a file first: `cmd > tmp.txt`, then pass tmp.txt",
        ),
        "double-bracket" => (
            "`[[ ... ]]` tests are not in kapsel's shell subset",
            "use test or [",
        ),
        "pipe-stderr" => ("`|&` is not in kapsel's shell subset", "use 2>&1 |"),
        "here-string" => (
            "here-strings `<<<` are not in kapsel's shell subset",
            "use a pipe: `printf '%s\\n' \"$x\" | cmd`",
        ),
        "clobber" => ("`>|` is not in kapsel's shell subset", "use >"),
        "for-without-in" => (
            "`for` without `in` (looping over the positional parameters) is not in kapsel's shell subset",
            "name the items: `for x in \"$@\"; do ...; done`",
        ),
        "select" => (
            "`select` menus are not in kapsel's shell subset",
            "select is interactive; use `for x in ...` instead",
        ),
        "coproc" => (
            "coprocesses are not in kapsel's shell subset",
            "use a pipeline",
        ),
        "tilde" => (
            "tilde expansion `~` is not in kapsel's shell subset",
            "write \"$HOME\" instead of ~, or quote it ('~') to mean a literal tilde",
        ),
        "brace-expansion" => (
            "brace expansion `{a,b}` and `{1..3}` is not in kapsel's shell subset",
            "write the words out: `a b`, or quote the braces to mean them literally",
        ),
        "dollar-single-quote" => (
            "`$'...'` strings are not in kapsel's shell subset",
            "use plain quotes, or printf for escapes: printf 'a\\tb\\n'",
        ),
        "dollar-double-quote" => (
            "`$\"...\"` locale strings are not in kapsel's shell subset",
            "drop the `$`: \"...\"",
        ),
        "redirect-both" => ("`&>` is not in kapsel's shell subset", "use `> file 2>&1`"),
        "read-write-redirect" => (
            "`<>` read-write redirection is not in kapsel's shell subset",
            "use < to read or > to write",
        ),
        "fd-close" => (
            "closing a file descriptor with `>&-` is not in kapsel's shell subset",
            "redirect to /dev/null instead: 2>/dev/null",
        ),
        "compound-redirect" => (
            "redirections on a whole loop are not in kapsel's shell subset yet",
            "pipe into the loop (`cat file | while read l; do ...; done`) or redirect the commands inside it",
        ),
        "array-assignment" => (
            "array assignment `name=(...)` is not in kapsel's shell subset",
            "use a space-separated string and a `for` loop",
        ),
        "special-param" => (
            "this special parameter is not in kapsel's shell subset",
            "only $?, $#, $0-$9, $@ and $* are available",
        ),
        _ => (
            "this construct is not in kapsel's shell subset",
            "say again without it",
        ),
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    b: GreenNodeBuilder<'static>,
    errors: Vec<ParseError>,
    /// An error was caused by reaching the end of input inside a construct.
    incomplete: bool,
    /// Nesting cap hit; the rest of the input was swallowed.
    aborted: bool,
    pending: Vec<Pending>,
    depth: u32,
}

impl<'a> Parser<'a> {
    // ------------------------------------------------------------ scanning

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn at(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn char_at(&self, p: usize) -> Option<char> {
        self.src.get(p..).and_then(|s| s.chars().next())
    }

    /// Emits `len` bytes from the current position as one token.
    fn token(&mut self, kind: SyntaxKind, len: usize) {
        let mut end = (self.pos + len).min(self.src.len());
        while !self.src.is_char_boundary(end) {
            end += 1;
        }
        if end > self.pos {
            self.b.token(kind.into(), &self.src[self.pos..end]);
            self.pos = end;
        }
    }

    fn kw(&mut self, k: &str) {
        self.token(KEYWORD, k.len());
    }

    /// Emits a token wrapped in an ERROR node.
    fn error_token(&mut self, kind: SyntaxKind, len: usize) {
        self.b.start_node(ERROR.into());
        self.token(kind, len);
        self.b.finish_node();
    }

    /// Spaces, tabs, line continuations and comments.
    fn skip_blanks(&mut self) {
        loop {
            let r = self.rest();
            let n = r.bytes().take_while(|b| *b == b' ' || *b == b'\t').count();
            if n > 0 {
                self.token(WHITESPACE, n);
            } else if r.starts_with("\\\n") {
                self.token(LINE_CONT, 2);
            } else if r.starts_with('#') {
                let n = r.find('\n').unwrap_or(r.len());
                self.token(COMMENT, n);
            } else {
                break;
            }
        }
    }

    /// Position after spaces, tabs and line continuations, without emitting.
    fn past_blanks(&self) -> usize {
        let b = self.src.as_bytes();
        let mut p = self.pos;
        loop {
            match b.get(p) {
                Some(b' ' | b'\t') => p += 1,
                Some(b'\\') if b.get(p + 1) == Some(&b'\n') => p += 2,
                _ => return p,
            }
        }
    }

    fn newline(&mut self) {
        self.token(NEWLINE, 1);
        if !self.pending.is_empty() {
            self.heredoc_bodies();
        }
    }

    /// Blanks, comments and newlines.
    fn linebreak(&mut self) {
        loop {
            self.skip_blanks();
            if self.at("\n") {
                self.newline();
            } else {
                break;
            }
        }
    }

    /// The run of plain characters at `p`.
    fn word_at(&self, p: usize) -> &'a str {
        let r = &self.src[p..];
        let n = r
            .char_indices()
            .find(|(_, c)| !is_plain(*c))
            .map_or(r.len(), |(i, _)| i);
        &r[..n]
    }

    /// True when the plain run at `p` is exactly `w` and ends at a delimiter.
    fn plain_word_is(&self, p: usize, w: &str) -> bool {
        let run = self.word_at(p);
        run == w && self.char_at(p + run.len()).is_none_or(is_meta)
    }

    fn reserved_at(&self, p: usize) -> Option<&'static str> {
        let run = self.word_at(p);
        if run.is_empty() || !self.char_at(p + run.len()).is_none_or(is_meta) {
            return None;
        }
        RESERVED.iter().find(|r| **r == run).copied()
    }

    fn peek_reserved(&self) -> Option<&'static str> {
        self.reserved_at(self.pos)
    }

    fn at_word_start(&self) -> bool {
        self.peek().is_some_and(|c| !is_meta(c) && c != '#')
    }

    fn can_start_command(&self) -> bool {
        match self.peek() {
            None => false,
            Some(';' | '|' | ')' | '\n') => false,
            Some('&') => self.at("&>"),
            Some(_) => true,
        }
    }

    fn at_closer(&self) -> bool {
        self.at(")") || self.at(";;") || self.peek_reserved().is_some_and(|w| CLOSERS.contains(&w))
    }

    /// Length of the token at the current position, for messages and recovery.
    fn op_len(&self) -> usize {
        for op in [
            ";;", "&&", "||", "|&", "&>", "<<", ">>", ";", "&", "|", ")", "(", "<", ">", "\n",
        ] {
            if self.at(op) {
                return op.len();
            }
        }
        let w = self.word_at(self.pos);
        if !w.is_empty() {
            return w.len();
        }
        self.peek().map_or(0, char::len_utf8)
    }

    /// Describes the token at the current position.
    fn found(&self) -> String {
        match self.peek() {
            None => "end of input".into(),
            Some('\n') => "a newline".into(),
            Some(_) => format!("`{}`", &self.src[self.pos..self.pos + self.op_len()]),
        }
    }

    // ------------------------------------------------------------ errors

    fn push_err(
        &mut self,
        construct: &str,
        message: String,
        start: usize,
        end: usize,
        expected: &[&str],
        help: Option<&str>,
    ) {
        let mut end = end.max(start).min(self.src.len());
        let start = start.min(end);
        // A construct's span ends at its last token, not at trailing blanks.
        while end > start && matches!(self.src.as_bytes()[end - 1], b' ' | b'\t' | b'\n') {
            end -= 1;
        }
        self.errors.push(ParseError {
            construct: construct.into(),
            message: format!("{construct}: {message}"),
            span: Span::new(start as u32, end as u32),
            expected: expected.iter().map(|s| (*s).to_string()).collect(),
            help: help.map(str::to_string),
        });
    }

    fn refuse(&mut self, construct: &str, start: usize, end: usize) {
        let (msg, help) = refusal(construct);
        self.push_err(construct, msg.into(), start, end, &[], Some(help));
    }

    fn refuse_help(&mut self, construct: &str, start: usize, end: usize, help: &str) {
        let (msg, _) = refusal(construct);
        self.push_err(construct, msg.into(), start, end, &[], Some(help));
    }

    fn syntax(
        &mut self,
        message: impl Into<String>,
        start: usize,
        end: usize,
        expected: &[&str],
        help: Option<&str>,
    ) {
        self.push_err("syntax", message.into(), start, end, expected, help);
    }

    fn eof_syntax(
        &mut self,
        message: impl Into<String>,
        start: usize,
        end: usize,
        expected: &[&str],
        help: Option<&str>,
    ) {
        self.incomplete = true;
        self.syntax(message, start, end, expected, help);
    }

    /// "expected X, found Y" at the current token, without consuming it.
    fn expected_here(&mut self, expected: &[&str], opener: (usize, usize), opener_text: &str) {
        let list = expected
            .iter()
            .map(|e| format!("`{e}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        if self.eof() {
            self.eof_syntax(
                format!("`{opener_text}` is not closed: expected {list}, found end of input"),
                opener.0,
                opener.1,
                expected,
                None,
            );
        } else {
            let found = self.found();
            let s = self.pos;
            let e = s + self.op_len();
            self.syntax(
                format!("expected {list} to continue `{opener_text}`, found {found}"),
                s,
                e,
                expected,
                None,
            );
        }
    }

    /// Consumes one unexpected token into an ERROR node.
    fn unexpected(&mut self, expected: &[&str]) {
        let start = self.pos;
        let len = self.op_len().max(1);
        let text = self.found();
        self.error_token(ERROR_TEXT, len);
        let help = match &self.src[start..self.pos] {
            ")" => Some("remove it, or quote it to pass it as text: ')'"),
            ";;" => Some("`;;` only ends a `case` branch; use a single `;`"),
            ";" | "|" | "&&" | "||" => Some("put a command before it"),
            _ => None,
        };
        self.syntax(
            format!("unexpected {text}"),
            start,
            self.pos,
            expected,
            help,
        );
    }

    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            if !self.aborted {
                self.aborted = true;
                let s = self.pos;
                self.syntax(
                    format!("nesting deeper than {MAX_DEPTH} levels"),
                    s,
                    self.src.len(),
                    &[],
                    Some("split the command into smaller steps"),
                );
            }
            let n = self.src.len() - self.pos;
            self.token(ERROR_TEXT, n);
            return false;
        }
        true
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    // ------------------------------------------------------------ lists

    /// Parses and-or lists until end of input or, when nested, a closer.
    /// Returns how many and-or lists were parsed.
    fn list_body(&mut self, top: bool) -> usize {
        if !self.enter() {
            return 0;
        }
        let mut count = 0;
        loop {
            self.linebreak();
            if self.eof() {
                break;
            }
            if self.at_closer() {
                if top {
                    self.unexpected(&[]);
                    continue;
                }
                break;
            }
            if !self.can_start_command() {
                self.unexpected(&["command"]);
                continue;
            }
            let before = self.pos;
            self.and_or();
            count += 1;
            if self.pos == before {
                self.unexpected(&["command"]);
                continue;
            }
            let p = self.past_blanks();
            let r = &self.src[p..];
            if r.starts_with(";;") || r.starts_with(')') {
                if top {
                    self.skip_blanks();
                    self.unexpected(&[]);
                } else {
                    break;
                }
            } else if r.starts_with(';') {
                self.skip_blanks();
                self.token(SEMI, 1);
            } else if r.starts_with('&') && !r.starts_with("&&") {
                self.skip_blanks();
                let s = self.pos;
                self.error_token(AMP, 1);
                self.refuse("background", s, s + 1);
            } else if r.is_empty() || r.starts_with('\n') || r.starts_with('#') {
            } else {
                self.skip_blanks();
                if !self.at_closer() || top {
                    let s = self.pos;
                    let e = s + self.op_len();
                    let found = self.found();
                    self.syntax(
                        format!("expected `;` or a newline before {found}"),
                        s,
                        e,
                        &[";", "newline"],
                        None,
                    );
                }
            }
        }
        self.leave();
        count
    }

    fn sub_list(&mut self) -> usize {
        self.b.start_node(LIST.into());
        let n = self.list_body(false);
        self.b.finish_node();
        n
    }

    fn and_or(&mut self) {
        self.b.start_node(AND_OR.into());
        self.pipeline();
        loop {
            let p = self.past_blanks();
            let r = &self.src[p..];
            let (kind, text) = if r.starts_with("&&") {
                (AND_AND, "&&")
            } else if r.starts_with("||") {
                (OR_OR, "||")
            } else {
                break;
            };
            self.skip_blanks();
            let s = self.pos;
            self.token(kind, 2);
            if !self.command_after(text, s, s + 2) {
                break;
            }
            self.pipeline();
        }
        self.b.finish_node();
    }

    /// After an operator that needs a following command: skips linebreaks
    /// and reports a missing command. True when a command can follow.
    fn command_after(&mut self, op: &str, s: usize, e: usize) -> bool {
        self.linebreak();
        if self.eof() {
            self.eof_syntax(
                format!("expected a command after `{op}`, found end of input"),
                s,
                e,
                &["command"],
                None,
            );
            return false;
        }
        if !self.can_start_command() || self.at_closer() {
            let found = self.found();
            self.syntax(
                format!("expected a command after `{op}`, found {found}"),
                s,
                e,
                &["command"],
                None,
            );
            return false;
        }
        true
    }

    fn pipeline(&mut self) {
        self.b.start_node(PIPELINE.into());
        if self.peek_reserved() == Some("!") {
            let s = self.pos;
            self.token(BANG, 1);
            self.skip_blanks();
            if !self.command_after("!", s, s + 1) {
                self.b.finish_node();
                return;
            }
        }
        self.command();
        loop {
            let p = self.past_blanks();
            let r = &self.src[p..];
            if r.starts_with("||") || !r.starts_with('|') {
                break;
            }
            self.skip_blanks();
            let s = self.pos;
            let text = if r.starts_with("|&") {
                self.error_token(PIPE_AMP, 2);
                self.refuse("pipe-stderr", s, s + 2);
                "|&"
            } else {
                self.token(PIPE, 1);
                "|"
            };
            if !self.command_after(text, s, self.pos) {
                break;
            }
            self.command();
        }
        self.b.finish_node();
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self) {
        if !self.enter() {
            return;
        }
        self.command_inner();
        self.leave();
    }

    fn command_inner(&mut self) {
        let cp = self.b.checkpoint();
        let in_subset = match self.peek_reserved() {
            Some("for") => {
                self.for_loop(false);
                true
            }
            Some("select") => {
                self.for_loop(true);
                false
            }
            Some(w @ ("while" | "until")) => {
                self.while_loop(w);
                true
            }
            Some("if") => {
                self.if_cmd();
                false
            }
            Some("case") => {
                self.case_cmd();
                false
            }
            Some("{") => {
                self.brace_group(true);
                false
            }
            Some("function") => {
                self.function_kw();
                return;
            }
            Some("coproc") => {
                self.coproc();
                return;
            }
            Some("[[") => {
                self.double_bracket();
                false
            }
            Some("!") => {
                let s = self.pos;
                self.error_token(ERROR_TEXT, 1);
                self.syntax(
                    "`!` is only allowed at the start of a pipeline",
                    s,
                    s + 1,
                    &[],
                    Some("write `! a | b` to negate the whole pipeline"),
                );
                self.skip_blanks();
                if self.can_start_command() && !self.at_closer() {
                    self.command();
                }
                return;
            }
            Some(w) if CLOSERS.contains(&w) => {
                self.unexpected(&["command"]);
                return;
            }
            _ => {
                if self.at("((") {
                    let s = self.pos;
                    self.arith(s, 2);
                    false
                } else if self.at("(") {
                    self.subshell();
                    false
                } else if self.is_function_def() {
                    self.function_def();
                    return;
                } else {
                    self.simple_command();
                    return;
                }
            }
        };
        if self.redirect_follows() {
            self.b.start_node_at(cp, ERROR.into());
            let rs = self.past_blanks();
            while self.redirect_follows() {
                self.skip_blanks();
                self.redirect();
            }
            self.b.finish_node();
            if in_subset {
                self.refuse("compound-redirect", rs, self.pos);
            }
        }
    }

    fn simple_command(&mut self) {
        self.b.start_node(SIMPLE_CMD.into());
        let mut seen = false;
        let mut first = true;
        loop {
            if !first {
                let p = self.past_blanks();
                match self.char_at(p) {
                    None | Some('\n' | ';' | '|' | ')' | '#') => break,
                    Some('&') if !self.src[p..].starts_with("&>") => break,
                    Some('(') => {
                        self.skip_blanks();
                        self.stray_paren();
                        continue;
                    }
                    _ => self.skip_blanks(),
                }
            }
            first = false;
            if !self.element(&mut seen) {
                break;
            }
        }
        self.b.finish_node();
    }

    /// `(` among a command's arguments: reported, its contents still parsed.
    fn stray_paren(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.token(LPAREN, 1);
        self.sub_list();
        if self.at(")") {
            self.token(RPAREN, 1);
        }
        self.b.finish_node();
        self.syntax(
            "unexpected `(` among a command's arguments",
            s,
            s + 1,
            &[],
            Some("quote it to pass it as text: '('"),
        );
    }

    fn element(&mut self, seen: &mut bool) -> bool {
        if self.at("<(") || self.at(">(") {
            self.proc_subst();
            *seen = true;
            return true;
        }
        if self.io_number_at(self.pos).is_some() || self.redirect_op_at(self.pos).is_some() {
            self.redirect();
            return true;
        }
        if !*seen && let Some(n) = self.assignment_len() {
            self.assignment(n);
            return true;
        }
        if self.at_word_start() {
            self.word();
            *seen = true;
            return true;
        }
        false
    }

    fn assignment_len(&self) -> Option<usize> {
        let r = self.rest();
        let mut it = r.char_indices();
        match it.next() {
            Some((_, c)) if is_name_start(c) => {}
            _ => return None,
        }
        for (i, c) in it {
            if c == '=' {
                return Some(i);
            }
            if !is_name_char(c) {
                return None;
            }
        }
        None
    }

    fn assignment(&mut self, name_len: usize) {
        let s = self.pos;
        self.b.start_node(ASSIGNMENT.into());
        self.token(NAME, name_len);
        self.token(EQ, 1);
        if self.at("(") {
            let ps = self.pos;
            let end = self.matching_paren(ps);
            self.error_token(ERROR_TEXT, end.unwrap_or(self.src.len()) - ps);
            self.refuse("array-assignment", s, self.pos);
            if end.is_none() {
                self.incomplete = true;
            }
        } else {
            self.b.start_node(WORD.into());
            if self.at("~") {
                self.tilde();
            }
            self.word_parts();
            self.b.finish_node();
        }
        self.b.finish_node();
    }

    /// Byte index after the `)` matching the `(` at `p`, skipping quotes.
    fn matching_paren(&self, p: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut it = self.src[p..].char_indices();
        while let Some((i, c)) = it.next() {
            match c {
                '\\' => {
                    it.next();
                }
                '\'' => {
                    for (_, d) in it.by_ref() {
                        if d == '\'' {
                            break;
                        }
                    }
                }
                '"' => {
                    while let Some((_, d)) = it.next() {
                        if d == '\\' {
                            it.next();
                        } else if d == '"' {
                            break;
                        }
                    }
                }
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(p + i + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn redirect_op_at(&self, p: usize) -> Option<(SyntaxKind, usize)> {
        let r = &self.src[p..];
        if r.starts_with("<(") || r.starts_with(">(") {
            return None;
        }
        const OPS: &[(&str, SyntaxKind)] = &[
            ("<<<", TLT),
            ("<<-", DLT_DASH),
            ("<<", DLT),
            ("<>", LT_GT),
            ("<&", LT_AMP),
            ("<", LT),
            (">>", DGT),
            (">&", GT_AMP),
            (">|", CLOBBER),
            (">", GT),
            ("&>>", AMP_GT),
            ("&>", AMP_GT),
        ];
        OPS.iter()
            .find(|(s, _)| r.starts_with(s))
            .map(|(s, k)| (*k, s.len()))
    }

    /// Digits directly followed by `<` or `>`.
    fn io_number_at(&self, p: usize) -> Option<usize> {
        let n = self.src[p..].bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        match self.redirect_op_at(p + n) {
            Some((AMP_GT, _)) | None => None,
            Some(_) => Some(n),
        }
    }

    fn redirect_follows(&self) -> bool {
        let p = self.past_blanks();
        self.io_number_at(p).is_some() || self.redirect_op_at(p).is_some()
    }

    fn redirect(&mut self) {
        let start = self.pos;
        self.b.start_node(REDIRECT.into());
        if let Some(d) = self.io_number_at(self.pos) {
            self.token(IO_NUMBER, d);
            if d > 1 {
                self.syntax(
                    "file descriptors above 9 are not in kapsel's shell subset",
                    start,
                    self.pos,
                    &[],
                    Some("use a single-digit file descriptor, e.g. 2>"),
                );
            }
        }
        let Some((kind, len)) = self.redirect_op_at(self.pos) else {
            self.b.finish_node();
            return;
        };
        let op_start = self.pos;
        self.token(kind, len);
        let op_end = self.pos;
        let op_text = &self.src[op_start..op_end];
        match kind {
            TLT => self.refuse("here-string", op_start, op_end),
            LT_GT => self.refuse("read-write-redirect", op_start, op_end),
            CLOBBER => self.refuse("clobber", op_start, op_end),
            AMP_GT => self.refuse("redirect-both", op_start, op_end),
            _ => {}
        }
        let heredoc = matches!(kind, DLT | DLT_DASH);
        let dup = matches!(kind, GT_AMP | LT_AMP);
        self.skip_blanks();
        if !self.at_word_start() {
            let found = self.found();
            let (what, exp) = if heredoc {
                ("a heredoc delimiter", "delimiter")
            } else if dup {
                ("a file descriptor number", "fd number")
            } else {
                ("a file name", "word")
            };
            self.syntax(
                format!("expected {what} after `{op_text}`, found {found}"),
                op_start,
                op_end,
                &[exp],
                None,
            );
        } else if heredoc {
            let ws = self.pos;
            self.word();
            let raw = &self.src[ws..self.pos];
            let (delim, quoted, expansion) = unquote_delim(raw);
            if expansion {
                self.syntax(
                    "a heredoc delimiter must be a plain word, not an expansion",
                    ws,
                    self.pos,
                    &["delimiter"],
                    Some("use a fixed word such as EOF"),
                );
            }
            self.pending.push(Pending {
                delim,
                quoted,
                strip_tabs: kind == DLT_DASH,
                start,
                end: self.pos,
            });
        } else if dup {
            let ws = self.pos;
            self.word();
            let raw = &self.src[ws..self.pos];
            if raw == "-" {
                self.refuse("fd-close", start, self.pos);
            } else if !(raw.len() == 1 && raw.as_bytes()[0].is_ascii_digit()) {
                self.syntax(
                    format!("`{op_text}` needs a single-digit file descriptor, found `{raw}`"),
                    ws,
                    self.pos,
                    &["fd number"],
                    Some("to send both streams to a file use `> file 2>&1`"),
                );
            }
        } else {
            self.word();
        }
        self.b.finish_node();
    }

    fn is_function_def(&self) -> bool {
        let w = self.word_at(self.pos);
        if w.is_empty() || w.contains('=') {
            return false;
        }
        let b = self.src.as_bytes();
        let mut p = self.pos + w.len();
        while matches!(b.get(p), Some(b' ' | b'\t')) {
            p += 1;
        }
        if b.get(p) != Some(&b'(') {
            return false;
        }
        p += 1;
        while matches!(b.get(p), Some(b' ' | b'\t')) {
            p += 1;
        }
        b.get(p) == Some(&b')')
    }

    fn function_def(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.word();
        self.skip_blanks();
        self.token(LPAREN, 1);
        self.skip_blanks();
        self.token(RPAREN, 1);
        self.function_body(s);
        self.b.finish_node();
        self.refuse("function-definition", s, self.pos);
    }

    fn function_kw(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.kw("function");
        self.skip_blanks();
        if self.at_word_start() {
            self.word();
        }
        self.skip_blanks();
        if self.at("(") {
            self.token(LPAREN, 1);
            self.skip_blanks();
            if self.at(")") {
                self.token(RPAREN, 1);
            }
        }
        self.function_body(s);
        self.b.finish_node();
        self.refuse("function-definition", s, self.pos);
    }

    fn function_body(&mut self, s: usize) {
        self.linebreak();
        if self.eof() {
            self.eof_syntax(
                "expected a function body, found end of input",
                s,
                self.pos,
                &["{"],
                None,
            );
        } else if self.peek_reserved() == Some("{") {
            // The function is refused already; don't report its braces again.
            self.brace_group(false);
        } else if self.can_start_command() && !self.at_closer() {
            self.command();
        } else {
            let found = self.found();
            let p = self.pos;
            self.syntax(
                format!("expected a function body, found {found}"),
                p,
                p + self.op_len(),
                &["{"],
                None,
            );
        }
    }

    fn coproc(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.kw("coproc");
        self.skip_blanks();
        if self.can_start_command() && !self.at_closer() {
            self.command();
        }
        self.b.finish_node();
        self.refuse("coproc", s, self.pos);
    }

    fn proc_subst(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.token(ERROR_TEXT, 2);
        self.sub_list();
        self.close_paren(s, 2);
        self.b.finish_node();
        self.refuse("process-substitution", s, self.pos);
    }

    /// Expects `)` closing the opener at `s` of `len` bytes.
    fn close_paren(&mut self, s: usize, len: usize) {
        if self.at(")") {
            self.token(RPAREN, 1);
        } else {
            let opener = self.src[s..s + len].to_string();
            self.expected_here(&[")"], (s, s + len), &opener);
        }
    }

    fn subshell(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.token(LPAREN, 1);
        self.sub_list();
        self.close_paren(s, 1);
        self.b.finish_node();
        self.refuse("subshell", s, self.pos);
    }

    fn brace_group(&mut self, record: bool) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.kw("{");
        self.sub_list();
        self.expect_kw("}", (s, s + 1), "{");
        self.b.finish_node();
        if record {
            self.refuse("brace-group", s, self.pos);
        }
    }

    /// Consumes reserved word `k` or reports it missing.
    fn expect_kw(&mut self, k: &str, opener: (usize, usize), opener_text: &str) -> bool {
        if self.peek_reserved() == Some(k) {
            self.kw(k);
            true
        } else {
            self.expected_here(&[k], opener, opener_text);
            false
        }
    }

    fn if_cmd(&mut self) {
        let s = self.pos;
        let op = (s, s + 2);
        self.b.start_node(ERROR.into());
        self.kw("if");
        self.sub_list();
        if self.expect_kw("then", op, "if") {
            self.sub_list();
            loop {
                match self.peek_reserved() {
                    Some("elif") => {
                        self.kw("elif");
                        self.sub_list();
                        if !self.expect_kw("then", op, "if") {
                            break;
                        }
                        self.sub_list();
                    }
                    Some("else") => {
                        self.kw("else");
                        self.sub_list();
                        self.expect_kw("fi", op, "if");
                        break;
                    }
                    Some("fi") => {
                        self.kw("fi");
                        break;
                    }
                    _ => {
                        self.expected_here(&["elif", "else", "fi"], op, "if");
                        break;
                    }
                }
            }
        }
        self.b.finish_node();
        self.refuse("if", s, self.pos);
    }

    fn case_cmd(&mut self) {
        let s = self.pos;
        let op = (s, s + 4);
        self.b.start_node(ERROR.into());
        self.kw("case");
        self.skip_blanks();
        if self.at_word_start() {
            self.word();
        } else {
            self.expected_here(&["word"], op, "case");
        }
        self.linebreak();
        if self.expect_kw("in", op, "case") {
            loop {
                self.linebreak();
                if self.eof() {
                    self.expected_here(&["esac"], op, "case");
                    break;
                }
                if self.peek_reserved() == Some("esac") {
                    self.kw("esac");
                    break;
                }
                if self.at("(") {
                    self.token(LPAREN, 1);
                }
                loop {
                    self.skip_blanks();
                    if !self.at_word_start() {
                        break;
                    }
                    self.word();
                    self.skip_blanks();
                    if self.at("|") && !self.at("||") {
                        self.token(PIPE, 1);
                    } else {
                        break;
                    }
                }
                self.skip_blanks();
                if self.at(")") {
                    self.token(RPAREN, 1);
                } else {
                    self.expected_here(&[")"], op, "case");
                    break;
                }
                self.sub_list();
                if self.at(";;") {
                    self.token(DSEMI, 2);
                } else if self.peek_reserved() != Some("esac") {
                    self.expected_here(&[";;", "esac"], op, "case");
                    break;
                }
            }
        }
        self.b.finish_node();
        self.refuse("case", s, self.pos);
    }

    fn double_bracket(&mut self) {
        let s = self.pos;
        self.b.start_node(ERROR.into());
        self.kw("[[");
        loop {
            self.skip_blanks();
            if self.eof() || self.at("\n") {
                self.expected_here(&["]]"], (s, s + 2), "[[");
                break;
            }
            if self.plain_word_is(self.pos, "]]") {
                self.kw("]]");
                break;
            }
            if self.at_word_start() {
                self.word();
            } else {
                let n = self.op_len().max(1);
                self.token(ERROR_TEXT, n);
            }
        }
        self.b.finish_node();
        self.refuse("double-bracket", s, self.pos);
    }

    fn for_loop(&mut self, select: bool) {
        let s = self.pos;
        let kwtext = if select { "select" } else { "for" };
        let op = (s, s + kwtext.len());
        self.b
            .start_node(if select { ERROR } else { FOR_LOOP }.into());
        self.kw(kwtext);
        self.skip_blanks();
        let name = self.word_at(self.pos);
        let name_ok = !name.is_empty()
            && name.starts_with(is_name_start)
            && name.chars().all(is_name_char)
            && self.char_at(self.pos + name.len()).is_none_or(is_meta);
        if name_ok {
            self.token(NAME, name.len());
        } else if self.eof() {
            self.expected_here(&["name"], op, kwtext);
        } else {
            let found = self.found();
            let p = self.pos;
            self.syntax(
                format!("expected a variable name after `{kwtext}`, found {found}"),
                p,
                p + self.op_len(),
                &["name"],
                Some("a loop variable is letters, digits and _, not starting with a digit"),
            );
            if self.at_word_start() {
                self.word();
            }
        }
        let after_name = self.pos;
        self.linebreak();
        if self.peek_reserved() == Some("in") {
            self.kw("in");
            loop {
                self.skip_blanks();
                if self.at_word_start() {
                    self.word();
                } else {
                    break;
                }
            }
            if self.at(";") {
                self.token(SEMI, 1);
            } else if self.at("\n") {
                self.newline();
            } else if !self.eof() {
                let found = self.found();
                let p = self.pos;
                self.syntax(
                    format!(
                        "expected `;` or a newline after the `{kwtext}` word list, found {found}"
                    ),
                    p,
                    p + self.op_len(),
                    &[";", "newline"],
                    None,
                );
            }
        } else {
            if !select {
                self.refuse("for-without-in", s, after_name);
            }
            if self.at(";") {
                self.token(SEMI, 1);
            }
        }
        self.linebreak();
        self.do_group(op, kwtext);
        self.b.finish_node();
        if select {
            self.refuse("select", s, self.pos);
        }
    }

    fn while_loop(&mut self, w: &'static str) {
        let s = self.pos;
        let op = (s, s + w.len());
        self.b.start_node(WHILE_LOOP.into());
        self.kw(w);
        let n = self.sub_list();
        if n == 0 && !self.eof() {
            let p = self.pos;
            self.syntax(
                format!("`{w}` needs a condition command before `do`"),
                s,
                p.max(op.1),
                &["command"],
                None,
            );
        }
        self.do_group(op, w);
        self.b.finish_node();
    }

    fn do_group(&mut self, op: (usize, usize), opener: &str) {
        if self.peek_reserved() == Some("do") {
            let ds = self.pos;
            self.kw("do");
            let n = self.sub_list();
            if n == 0 && !self.eof() {
                self.syntax(
                    "a loop body needs at least one command between `do` and `done`",
                    ds,
                    ds + 2,
                    &["command"],
                    None,
                );
            }
            if self.peek_reserved() == Some("done") {
                self.kw("done");
            } else {
                self.expected_here(&["done"], op, opener);
            }
        } else {
            self.expected_here(&["do"], op, opener);
            // Recover as if `do` were there, so `for x in a; echo; done`
            // reports one error, not three.
            if self.can_start_command() && !self.at_closer() {
                self.sub_list();
                if self.peek_reserved() == Some("done") {
                    self.kw("done");
                }
            }
        }
    }

    // ------------------------------------------------------------ words

    fn word(&mut self) {
        self.b.start_node(WORD.into());
        if self.at("~") {
            self.tilde();
        }
        self.word_parts();
        self.b.finish_node();
    }

    fn tilde(&mut self) {
        let s = self.pos;
        let run = self.word_at(s);
        let n = run.find('/').unwrap_or(run.len()).max(1);
        self.error_token(ERROR_TEXT, 1);
        self.refuse("tilde", s, s + n);
    }

    fn flush(&mut self, lit: &mut Option<usize>) {
        if let Some(s) = lit.take()
            && s < self.pos
        {
            self.b.token(LIT.into(), &self.src[s..self.pos]);
        }
    }

    fn lit_char(&mut self, lit: &mut Option<usize>, c: char) {
        lit.get_or_insert(self.pos);
        self.pos += c.len_utf8();
    }

    /// The parts of an unquoted word, up to a metacharacter.
    fn word_parts(&mut self) {
        let mut lit = None;
        while let Some(c) = self.peek() {
            if is_meta(c) {
                break;
            }
            match c {
                '\\' => {
                    self.flush(&mut lit);
                    match self.char_at(self.pos + 1) {
                        Some('\n') => self.token(LINE_CONT, 2),
                        Some(ch) => self.token(ESCAPE, 1 + ch.len_utf8()),
                        None => {
                            let s = self.pos;
                            self.error_token(ERROR_TEXT, 1);
                            self.eof_syntax(
                                "backslash at end of input",
                                s,
                                s + 1,
                                &[],
                                Some("remove it, or continue the command on the next line"),
                            );
                        }
                    }
                }
                '\'' => {
                    self.flush(&mut lit);
                    self.single_quote();
                }
                '"' => {
                    self.flush(&mut lit);
                    self.double_quote();
                }
                '$' => {
                    self.flush(&mut lit);
                    self.dollar(Ctx::Word);
                }
                '`' => {
                    self.flush(&mut lit);
                    self.backquote();
                }
                '*' => {
                    self.flush(&mut lit);
                    self.token(GLOB_STAR, 1);
                }
                '?' => {
                    self.flush(&mut lit);
                    self.token(GLOB_QUESTION, 1);
                }
                '[' => match self.bracket_len() {
                    Some(n) => {
                        self.flush(&mut lit);
                        self.token(GLOB_CLASS, n);
                    }
                    None => self.lit_char(&mut lit, c),
                },
                '{' => match self.brace_expansion_len() {
                    Some(n) => {
                        self.flush(&mut lit);
                        let s = self.pos;
                        self.error_token(ERROR_TEXT, n);
                        self.refuse("brace-expansion", s, s + n);
                    }
                    None => self.lit_char(&mut lit, c),
                },
                _ => self.lit_char(&mut lit, c),
            }
        }
        self.flush(&mut lit);
    }

    fn single_quote(&mut self) {
        let s = self.pos;
        match self.src[s + 1..].find('\'') {
            Some(i) => self.token(SQUOTE, i + 2),
            None => {
                self.token(SQUOTE, self.src.len() - s);
                self.eof_syntax(
                    "unterminated single quote",
                    s,
                    s + 1,
                    &["'"],
                    Some("add the closing ' ; a single-quoted string cannot contain '"),
                );
            }
        }
    }

    fn double_quote(&mut self) {
        let s = self.pos;
        self.b.start_node(DQUOTED.into());
        self.token(DQUOTE, 1);
        let mut lit = None;
        loop {
            match self.peek() {
                None => {
                    self.flush(&mut lit);
                    self.eof_syntax(
                        "unterminated double quote",
                        s,
                        s + 1,
                        &["\""],
                        Some("add the closing \" ; write \\\" for a quote inside"),
                    );
                    break;
                }
                Some('"') => {
                    self.flush(&mut lit);
                    self.token(DQUOTE, 1);
                    break;
                }
                Some('\\') => match self.char_at(self.pos + 1) {
                    Some('"' | '\\' | '$' | '`') => {
                        self.flush(&mut lit);
                        self.token(ESCAPE, 2);
                    }
                    Some('\n') => {
                        self.flush(&mut lit);
                        self.token(LINE_CONT, 2);
                    }
                    _ => self.lit_char(&mut lit, '\\'),
                },
                Some('$') => {
                    self.flush(&mut lit);
                    self.dollar(Ctx::DQuote);
                }
                Some('`') => {
                    self.flush(&mut lit);
                    self.backquote();
                }
                Some(c) => self.lit_char(&mut lit, c),
            }
        }
        self.b.finish_node();
    }

    fn dollar(&mut self, ctx: Ctx) {
        let s = self.pos;
        match self.char_at(s + 1) {
            Some('{') => self.braced_param(),
            Some('(') => {
                if self.char_at(s + 2) == Some('(') {
                    self.arith(s, 3);
                } else {
                    self.cmd_subst();
                }
            }
            Some('\'') if ctx == Ctx::Word => {
                let mut end = None;
                let mut it = self.src[s + 2..].char_indices();
                while let Some((i, c)) = it.next() {
                    if c == '\\' {
                        it.next();
                    } else if c == '\'' {
                        end = Some(s + 2 + i + 1);
                        break;
                    }
                }
                self.error_token(ERROR_TEXT, end.unwrap_or(self.src.len()) - s);
                self.refuse("dollar-single-quote", s, self.pos);
                if end.is_none() {
                    self.incomplete = true;
                }
            }
            Some('"') if ctx == Ctx::Word => {
                self.error_token(ERROR_TEXT, 1);
                self.refuse("dollar-double-quote", s, s + 1);
            }
            Some(c) if is_name_start(c) => {
                let n = self.src[s + 1..]
                    .chars()
                    .take_while(|c| is_name_char(*c))
                    .count();
                self.b.start_node(PARAM.into());
                self.token(PARAM_TOKEN, 1 + n);
                self.b.finish_node();
            }
            Some(c) if c.is_ascii_digit() || matches!(c, '?' | '#' | '@' | '*') => {
                self.b.start_node(PARAM.into());
                self.token(PARAM_TOKEN, 2);
                self.b.finish_node();
            }
            Some(c @ ('$' | '!' | '-')) => {
                self.error_token(ERROR_TEXT, 2);
                let help = match c {
                    '$' => "the shell's process id is not available; use a fixed name or mktemp",
                    '!' => "background jobs are not in the subset, so there is no $!",
                    _ => "shell option flags are not available",
                };
                self.refuse_help("special-param", s, s + 2, help);
            }
            _ => self.token(LIT, 1),
        }
    }

    fn braced_param(&mut self) {
        let s = self.pos;
        let p = s + 2;
        let r = &self.src[p..];
        let first = r.chars().next();
        let name_len = match first {
            Some(c) if is_name_start(c) => r.chars().take_while(|c| is_name_char(*c)).count(),
            Some(c) if c.is_ascii_digit() => r.bytes().take_while(u8::is_ascii_digit).count(),
            Some('?' | '#' | '@' | '*') if r[1..].starts_with('}') => 1,
            _ => 0,
        };
        if name_len > 0 && self.char_at(p + name_len) == Some('}') {
            self.b.start_node(PARAM.into());
            self.token(DOLLAR_LBRACE, 2);
            self.token(PARAM_NAME, name_len);
            self.token(PARAM_RBRACE, 1);
            self.b.finish_node();
            return;
        }
        let end = self.matching_brace(p);
        self.error_token(ERROR_TEXT, end.unwrap_or(self.src.len()) - s);
        let Some(e) = end else {
            self.eof_syntax(
                "unterminated `${`",
                s,
                s + 2,
                &["}"],
                Some("close it with }"),
            );
            return;
        };
        let inner = &self.src[p..e - 1];
        let op = &inner[name_len.min(inner.len())..];
        if inner.starts_with('#') && inner.len() > 1 {
            self.refuse_help(
                "param-operator",
                s,
                e,
                "for a length use `printf %s \"$x\" | wc -c`",
            );
        } else if inner.starts_with('!') {
            self.refuse_help(
                "param-operator",
                s,
                e,
                "indirect expansion is not available; name the variable directly",
            );
        } else if name_len == 0 {
            self.syntax(
                format!(
                    "bad substitution `{}`: expected a parameter name",
                    &self.src[s..e]
                ),
                s,
                e,
                &["name"],
                Some("write ${name} with a name of letters, digits and _"),
            );
        } else {
            let x = &inner[..name_len.min(inner.len())];
            let help = if op.starts_with(":-") || op.starts_with('-') {
                format!(
                    "assign a default first: `[ -n \"${x}\" ] || {x}=default`, then use ${{{x}}}"
                )
            } else if op.starts_with(":=") || op.starts_with('=') {
                format!("assign the default as its own command: `[ -n \"${x}\" ] || {x}=default`")
            } else if op.starts_with(":?") || op.starts_with('?') {
                format!("test it: `[ -n \"${x}\" ] || exit 1`")
            } else if op.starts_with(":+") || op.starts_with('+') {
                format!("test it: `[ -n \"${x}\" ] && ...`")
            } else if op.starts_with('%') || op.starts_with('#') {
                "use basename, dirname or sed to cut the value".to_string()
            } else if op.starts_with('/') {
                "use sed to replace text in the value".to_string()
            } else if op.starts_with(':') {
                "use cut to take part of the value".to_string()
            } else {
                format!("write plain ${{{x}}}")
            };
            self.refuse_help("param-operator", s, e, &help);
        }
    }

    /// Index after the `}` matching an already-open `{`, skipping quotes.
    fn matching_brace(&self, p: usize) -> Option<usize> {
        let mut depth = 1usize;
        let mut it = self.src[p..].char_indices();
        while let Some((i, c)) = it.next() {
            match c {
                '\\' => {
                    it.next();
                }
                '\'' => {
                    for (_, d) in it.by_ref() {
                        if d == '\'' {
                            break;
                        }
                    }
                }
                '"' => {
                    while let Some((_, d)) = it.next() {
                        if d == '\\' {
                            it.next();
                        } else if d == '"' {
                            break;
                        }
                    }
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(p + i + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn cmd_subst(&mut self) {
        let s = self.pos;
        self.b.start_node(CMD_SUBST.into());
        self.token(DOLLAR_LPAREN, 2);
        self.b.start_node(SCRIPT.into());
        self.list_body(false);
        self.b.finish_node();
        self.close_paren(s, 2);
        self.b.finish_node();
    }

    /// `$((...))` or `((...))` starting at `s`, with a `prefix`-byte opener.
    fn arith(&mut self, s: usize, prefix: usize) {
        let mut depth = 0usize;
        let mut end = None;
        let b = self.src.as_bytes();
        let mut i = s + prefix;
        while i < b.len() {
            match b[i] {
                b'(' => depth += 1,
                b')' if depth > 0 => depth -= 1,
                b')' => {
                    end = Some(if b.get(i + 1) == Some(&b')') {
                        i + 2
                    } else {
                        i + 1
                    });
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        self.error_token(ERROR_TEXT, end.unwrap_or(self.src.len()) - s);
        self.refuse("arithmetic", s, self.pos);
        if end.is_none() {
            self.incomplete = true;
        }
    }

    fn backquote(&mut self) {
        let s = self.pos;
        let mut end = None;
        let mut it = self.src[s + 1..].char_indices();
        while let Some((i, c)) = it.next() {
            if c == '\\' {
                it.next();
            } else if c == '`' {
                end = Some(s + 1 + i + 1);
                break;
            }
        }
        self.error_token(ERROR_TEXT, end.unwrap_or(self.src.len()) - s);
        self.refuse("backquote", s, self.pos);
        if end.is_none() {
            self.incomplete = true;
        }
    }

    /// Length of a bracket expression `[...]` at the current position.
    fn bracket_len(&self) -> Option<usize> {
        let s = self.pos;
        let b = self.src.as_bytes();
        let mut i = s + 1;
        if matches!(b.get(i), Some(b'!' | b'^')) {
            i += 1;
        }
        if b.get(i) == Some(&b']') {
            i += 1;
        }
        loop {
            let c = self.char_at(i)?;
            if is_meta(c) || matches!(c, '\'' | '"' | '\\' | '$' | '`') {
                return None;
            }
            match c {
                ']' => return Some(i + 1 - s),
                '[' if matches!(b.get(i + 1), Some(b':' | b'=' | b'.')) => {
                    let close = [b[i + 1], b']'];
                    let mut j = i + 2;
                    let mut found = None;
                    while let Some(d) = self.char_at(j) {
                        if is_meta(d) {
                            break;
                        }
                        if b[j..].starts_with(&close) {
                            found = Some(j + 2);
                            break;
                        }
                        j += d.len_utf8();
                    }
                    i = found.unwrap_or(i + 1);
                }
                _ => i += c.len_utf8(),
            }
        }
    }

    /// Length of `{a,b}` / `{1..3}` at the current position, if it is one.
    fn brace_expansion_len(&self) -> Option<usize> {
        let s = self.pos;
        let mut depth = 0usize;
        let mut list = false;
        let mut it = self.src[s..].char_indices().peekable();
        while let Some((i, c)) = it.next() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return list.then_some(i + 1);
                    }
                }
                ',' if depth == 1 => list = true,
                '.' if depth == 1 && it.peek().map(|x| x.1) == Some('.') => list = true,
                '\\' => {
                    it.next();
                }
                '\'' => {
                    for (_, d) in it.by_ref() {
                        if d == '\'' {
                            break;
                        }
                    }
                }
                '"' => {
                    while let Some((_, d)) = it.next() {
                        if d == '\\' {
                            it.next();
                        } else if d == '"' {
                            break;
                        }
                    }
                }
                c if is_meta(c) => return None,
                _ => {}
            }
        }
        None
    }

    // ------------------------------------------------------------ heredocs

    fn heredoc_bodies(&mut self) {
        for h in std::mem::take(&mut self.pending) {
            self.b.start_node(HEREDOC_BODY.into());
            loop {
                if self.eof() {
                    self.eof_syntax(
                        format!("the heredoc is missing its closing `{}` line", h.delim),
                        h.start,
                        h.end,
                        &[h.delim.as_str()],
                        Some("end the body with a line containing only the delimiter"),
                    );
                    break;
                }
                if h.strip_tabs {
                    let n = self.rest().bytes().take_while(|b| *b == b'\t').count();
                    self.token(HEREDOC_TABS, n);
                }
                let r = self.rest();
                let (line, nl) = match r.find('\n') {
                    Some(i) => (&r[..i], 1),
                    None => (r, 0),
                };
                if line == h.delim {
                    self.token(HEREDOC_END, line.len() + nl);
                    break;
                }
                if h.quoted {
                    self.token(LIT, line.len() + nl);
                } else {
                    self.heredoc_line();
                }
            }
            self.b.finish_node();
        }
    }

    /// One line of an unquoted heredoc body, with expansions.
    fn heredoc_line(&mut self) {
        let mut lit = None;
        while let Some(c) = self.peek() {
            match c {
                '\n' => {
                    self.lit_char(&mut lit, c);
                    break;
                }
                '\\' => match self.char_at(self.pos + 1) {
                    Some('$' | '`' | '\\') => {
                        self.flush(&mut lit);
                        self.token(ESCAPE, 2);
                    }
                    Some('\n') => {
                        self.flush(&mut lit);
                        self.token(LINE_CONT, 2);
                    }
                    _ => self.lit_char(&mut lit, c),
                },
                '$' => {
                    self.flush(&mut lit);
                    self.dollar(Ctx::Heredoc);
                }
                '`' => {
                    self.flush(&mut lit);
                    self.backquote();
                }
                _ => self.lit_char(&mut lit, c),
            }
        }
        self.flush(&mut lit);
    }
}

/// Quote removal on a heredoc delimiter: (text, quoted, contains expansion).
fn unquote_delim(raw: &str) -> (String, bool, bool) {
    let mut out = String::new();
    let mut quoted = false;
    let mut expansion = false;
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\'' => {
                quoted = true;
                for d in it.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    out.push(d);
                }
            }
            '"' => {
                quoted = true;
                while let Some(d) = it.next() {
                    match d {
                        '"' => break,
                        '\\' if matches!(it.peek(), Some('"' | '\\' | '$' | '`')) => {
                            out.extend(it.next());
                        }
                        '$' => {
                            expansion = true;
                            out.push(d);
                        }
                        _ => out.push(d),
                    }
                }
            }
            '\\' => {
                quoted = true;
                out.extend(it.next());
            }
            '$' => {
                if it
                    .peek()
                    .is_some_and(|n| is_name_start(*n) || matches!(n, '{' | '('))
                {
                    expansion = true;
                }
                out.push(c);
            }
            '`' => {
                expansion = true;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    (out, quoted, expansion)
}
