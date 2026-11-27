//! A small parser for the simple-command part of the subset, for tests only.
//!
//! The real parser is `kapsel_syntax::parse` (ws/parser). Until it lands, the
//! analysis and diagnostics tests need `kapsel_ast::Script` values with real
//! spans, and writing them by hand is error-prone. This parser covers what the
//! KS-S tests use: `;` and newlines, `&&`, `||`, `|`, `!`, assignments,
//! quotes, `$NAME`, `${NAME}`, `$(...)`, globs, redirects, heredocs and
//! `for NAME in ...; do ...; done`. It panics on anything else: it is a test
//! fixture, not a parser.
//!
//! `real_parser_gives_the_same_findings` checks that shapes and findings agree
//! with `kapsel_syntax::parse` on the planted cases. TODO: move the remaining
//! tests to the real parser and delete this module.

use kapsel_ast::*;

/// Parses `src` into a script. Panics on input outside the fixture subset.
pub fn parse(src: &str) -> Script {
    let mut p = P { s: src.as_bytes(), src, i: 0 };
    let items = p.list(&[]);
    p.skip_ws_nl();
    assert!(p.i == p.s.len(), "testkit: unexpected input at byte {}: {:?}", p.i, &src[p.i..]);
    Script { items, span: Span::new(0, src.len() as u32) }
}

struct P<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn starts(&self, t: &str) -> bool {
        self.src[self.i..].starts_with(t)
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' {
                self.i += 1;
            } else if c == b'#' {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }
    fn newline(&mut self) {
        self.i += 1;
    }

    /// Reads one heredoc body starting at the current position, up to and
    /// including the delimiter line.
    fn heredoc_body(&mut self, delim: &str, strip: bool, quoted: bool) -> RedirectTarget {
        let start = self.i;
        let mut body = String::new();
        loop {
            let line_end = self.src[self.i..].find('\n').map(|n| self.i + n).unwrap_or(self.s.len());
            let line = &self.src[self.i..line_end];
            let cmp = if strip { line.trim_start_matches('\t') } else { line };
            if cmp == delim {
                let body_end = self.i;
                self.i = (line_end + 1).min(self.s.len());
                let part = if quoted {
                    WordPart::SingleQuoted { text: body }
                } else {
                    WordPart::Literal { text: body }
                };
                let w = Word { parts: vec![part], span: Span::new(start as u32, body_end as u32) };
                return RedirectTarget::Heredoc { body: w, quoted, strip_tabs: strip };
            }
            body.push_str(cmp);
            body.push('\n');
            assert!(line_end < self.s.len(), "testkit: unterminated heredoc");
            self.i = line_end + 1;
        }
    }
    fn skip_ws_nl(&mut self) {
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'\n') => self.newline(),
                Some(b';') if !self.starts(";;") => self.i += 1,
                _ => break,
            }
        }
    }

    fn at_keyword(&self, kws: &[&str]) -> bool {
        kws.iter().any(|k| {
            self.starts(k)
                && self.s.get(self.i + k.len()).is_none_or(|c| matches!(c, b' ' | b'\t' | b'\n' | b';'))
        })
    }

    fn list(&mut self, stop: &[&str]) -> Vec<AndOrList> {
        let mut items = Vec::new();
        loop {
            self.skip_ws_nl();
            if self.peek().is_none() || self.peek() == Some(b')') || self.at_keyword(stop) {
                return items;
            }
            items.push(self.and_or());
        }
    }

    fn and_or(&mut self) -> AndOrList {
        let start = self.i;
        let first = self.pipeline();
        let mut rest = Vec::new();
        loop {
            self.skip_ws();
            let op = if self.starts("&&") {
                AndOrOp::And
            } else if self.starts("||") {
                AndOrOp::Or
            } else {
                break;
            };
            self.i += 2;
            loop {
                self.skip_ws();
                if self.peek() == Some(b'\n') {
                    self.newline();
                } else {
                    break;
                }
            }
            rest.push((op, self.pipeline()));
        }
        let end = rest.last().map(|(_, p): &(AndOrOp, Pipeline)| p.span.end).unwrap_or(first.span.end);
        AndOrList { first, rest, span: Span::new(start as u32, end) }
    }

    fn pipeline(&mut self) -> Pipeline {
        self.skip_ws();
        let start = self.i;
        let mut negated = false;
        if self.peek() == Some(b'!') {
            negated = true;
            self.i += 1;
        }
        let mut commands = vec![self.command()];
        loop {
            self.skip_ws();
            if self.peek() == Some(b'|') && !self.starts("||") {
                self.i += 1;
                commands.push(self.command());
            } else {
                break;
            }
        }
        let end = match commands.last().unwrap() {
            Command::Simple(c) => c.span.end,
            Command::For(f) => f.span.end,
            Command::While(w) => w.span.end,
        };
        Pipeline { negated, commands, span: Span::new(start as u32, end) }
    }

    fn command(&mut self) -> Command {
        self.skip_ws();
        if self.at_keyword(&["for"]) {
            return Command::For(self.for_loop());
        }
        Command::Simple(self.simple())
    }

    fn expect_kw(&mut self, kw: &str) {
        self.skip_ws_nl();
        assert!(self.at_keyword(&[kw]), "testkit: expected `{kw}` at byte {}", self.i);
        self.i += kw.len();
    }

    fn for_loop(&mut self) -> ForLoop {
        let start = self.i;
        self.i += 3;
        self.skip_ws();
        let ns = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            self.i += 1;
        }
        let var = self.src[ns..self.i].to_string();
        self.expect_kw("in");
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(b';') | Some(b'\n')) {
                break;
            }
            items.push(self.word());
        }
        self.expect_kw("do");
        let body = self.list(&["done"]);
        self.expect_kw("done");
        ForLoop { var, items, body, span: Span::new(start as u32, self.i as u32) }
    }

    fn simple(&mut self) -> SimpleCommand {
        self.skip_ws();
        let start = self.i;
        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects: Vec<Redirect> = Vec::new();
        let mut heredoc_slots = Vec::new();
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else { break };
            if matches!(c, b';' | b'\n' | b'|' | b'&' | b')') {
                break;
            }
            if words.is_empty() && self.at_keyword(&["done", "do"]) {
                break;
            }
            // redirect?
            let save = self.i;
            let mut fd = None;
            let ds = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i > ds && matches!(self.peek(), Some(b'<') | Some(b'>')) {
                fd = Some(self.src[ds..self.i].parse().unwrap());
            } else {
                self.i = save;
            }
            if matches!(self.peek(), Some(b'<') | Some(b'>')) {
                let op = if self.starts("<<") {
                    self.i += 2;
                    RedirectOp::Heredoc
                } else if self.starts(">>") {
                    self.i += 2;
                    RedirectOp::Append
                } else if self.starts(">&") {
                    self.i += 2;
                    RedirectOp::DupOut
                } else if self.starts("<&") {
                    self.i += 2;
                    RedirectOp::DupIn
                } else if self.starts(">") {
                    self.i += 1;
                    RedirectOp::Write
                } else {
                    self.i += 1;
                    RedirectOp::Read
                };
                let target = match op {
                    RedirectOp::DupOut | RedirectOp::DupIn => {
                        let ds = self.i;
                        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                            self.i += 1;
                        }
                        RedirectTarget::Fd { fd: self.src[ds..self.i].parse().unwrap() }
                    }
                    RedirectOp::Heredoc => {
                        let strip = if self.peek() == Some(b'-') {
                            self.i += 1;
                            true
                        } else {
                            false
                        };
                        self.skip_ws();
                        let w = self.word();
                        let quoted = w.parts.iter().any(|p| !matches!(p, WordPart::Literal { .. }));
                        let delim = flat(&w);
                        heredoc_slots.push((redirects.len(), delim, strip, quoted));
                        RedirectTarget::Fd { fd: u32::MAX }
                    }
                    _ => {
                        self.skip_ws();
                        RedirectTarget::Word { word: self.word() }
                    }
                };
                redirects.push(Redirect { fd, op, target, span: Span::new(save as u32, self.i as u32) });
                continue;
            }
            // assignment?
            if words.is_empty() {
                let ns = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                    self.i += 1;
                }
                if self.i > ns && self.peek() == Some(b'=') && !self.s[ns].is_ascii_digit() {
                    let name = self.src[ns..self.i].to_string();
                    self.i += 1;
                    let value = self.word();
                    assignments.push(Assignment { name, value, span: Span::new(ns as u32, self.i as u32) });
                    continue;
                }
                self.i = ns;
            }
            words.push(self.word());
        }
        let end = self.i;
        // Trim trailing whitespace from the span.
        let mut e = end;
        while e > start && matches!(self.s[e - 1], b' ' | b'\t') {
            e -= 1;
        }
        let mut cmd = SimpleCommand { assignments, words, redirects, span: Span::new(start as u32, e as u32) };
        // Fixture restriction: a command with a heredoc must end its line, so
        // the bodies follow directly.
        if !heredoc_slots.is_empty() {
            self.skip_ws();
            assert!(matches!(self.peek(), Some(b'\n')), "testkit: a heredoc must end its line (byte {})", self.i);
            self.newline();
            for (idx, delim, strip, quoted) in heredoc_slots {
                cmd.redirects[idx].target = self.heredoc_body(&delim, strip, quoted);
            }
        }
        cmd
    }

    fn word(&mut self) -> Word {
        let start = self.i;
        let mut parts: Vec<WordPart> = Vec::new();
        let mut lit = String::new();
        let flush = |lit: &mut String, parts: &mut Vec<WordPart>| {
            if !lit.is_empty() {
                parts.push(WordPart::Literal { text: std::mem::take(lit) });
            }
        };
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\n' | b';' | b'|' | b'&' | b'<' | b'>' | b')' => break,
                b'\\' => {
                    let ch = self.src[self.i + 1..].chars().next().unwrap();
                    lit.push(ch);
                    self.i += 1 + ch.len_utf8();
                }
                b'\'' => {
                    flush(&mut lit, &mut parts);
                    let end = self.src[self.i + 1..].find('\'').expect("testkit: unterminated '") + self.i + 1;
                    parts.push(WordPart::SingleQuoted { text: self.src[self.i + 1..end].to_string() });
                    self.i = end + 1;
                }
                b'"' => {
                    flush(&mut lit, &mut parts);
                    self.i += 1;
                    let mut inner = Vec::new();
                    let mut ilit = String::new();
                    loop {
                        match self.peek().expect("testkit: unterminated \"") {
                            b'"' => {
                                self.i += 1;
                                break;
                            }
                            b'$' => {
                                flush(&mut ilit, &mut inner);
                                inner.push(self.dollar());
                            }
                            b'\\' => {
                                let ch = self.src[self.i + 1..].chars().next().unwrap();
                                ilit.push(ch);
                                self.i += 1 + ch.len_utf8();
                            }
                            _ => {
                                let ch = self.src[self.i..].chars().next().unwrap();
                                ilit.push(ch);
                                self.i += ch.len_utf8();
                            }
                        }
                    }
                    flush(&mut ilit, &mut inner);
                    parts.push(WordPart::DoubleQuoted { parts: inner });
                }
                b'$' => {
                    flush(&mut lit, &mut parts);
                    parts.push(self.dollar());
                }
                b'*' | b'?' => {
                    flush(&mut lit, &mut parts);
                    self.i += 1;
                    parts.push(WordPart::Glob { glob: if c == b'*' { GlobPart::Star } else { GlobPart::Question } });
                }
                b'[' => {
                    flush(&mut lit, &mut parts);
                    let end = self.src[self.i..].find(']').expect("testkit: unterminated [") + self.i;
                    parts.push(WordPart::Glob { glob: GlobPart::Class { set: self.src[self.i + 1..end].to_string() } });
                    self.i = end + 1;
                }
                _ => {
                    let ch = self.src[self.i..].chars().next().unwrap();
                    lit.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
        flush(&mut lit, &mut parts);
        assert!(self.i > start, "testkit: expected a word at byte {start}");
        Word { parts, span: Span::new(start as u32, self.i as u32) }
    }

    fn dollar(&mut self) -> WordPart {
        self.i += 1;
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let start = self.i;
                let items = self.list(&[]);
                self.skip_ws_nl();
                assert_eq!(self.peek(), Some(b')'), "testkit: unterminated $(");
                let span = Span::new(start as u32, self.i as u32);
                self.i += 1;
                WordPart::CommandSubst { script: Box::new(Script { items, span }) }
            }
            Some(b'{') => {
                let end = self.src[self.i..].find('}').expect("testkit: unterminated ${") + self.i;
                let name = self.src[self.i + 1..end].to_string();
                self.i = end + 1;
                WordPart::Param { name }
            }
            Some(c) if matches!(c, b'?' | b'#' | b'@' | b'*') || c.is_ascii_digit() => {
                self.i += 1;
                WordPart::Param { name: (c as char).to_string() }
            }
            _ => {
                let ns = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                    self.i += 1;
                }
                if self.i == ns {
                    WordPart::Literal { text: "$".into() }
                } else {
                    WordPart::Param { name: self.src[ns..self.i].to_string() }
                }
            }
        }
    }
}

fn flat(w: &Word) -> String {
    let mut s = String::new();
    for p in &w.parts {
        match p {
            WordPart::Literal { text } | WordPart::SingleQuoted { text } => s.push_str(text),
            WordPart::DoubleQuoted { parts } => {
                for p in parts {
                    if let WordPart::Literal { text } = p {
                        s.push_str(text);
                    }
                }
            }
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pipeline_and_redirects() {
        let src = "cat a.txt | sort -n > out.txt && echo \"done $X\"";
        let s = parse(src);
        assert_eq!(s.items.len(), 1);
        let Command::Simple(c) = &s.items[0].first.commands[1] else { panic!() };
        assert_eq!(&src[c.span.start as usize..c.span.end as usize], "sort -n > out.txt");
        assert_eq!(c.redirects.len(), 1);
    }

    #[test]
    fn parses_heredoc_body_span() {
        let src = "cat <<EOF\nhello $X\nEOF\necho after\n";
        let s = parse(src);
        assert_eq!(s.items.len(), 2);
        let Command::Simple(c) = &s.items[0].first.commands[0] else { panic!() };
        let RedirectTarget::Heredoc { body, .. } = &c.redirects[0].target else { panic!("{:?}", c.redirects) };
        assert_eq!(&src[body.span.start as usize..body.span.end as usize], "hello $X\n");
    }

    #[test]
    fn parses_for_and_capture() {
        let s = parse("for f in a b; do cat $f; done\necho $(ls out)");
        assert!(matches!(s.items[0].first.commands[0], Command::For(_)));
        assert_eq!(s.items.len(), 2);
    }
}
