//! A small test-only parser for the shell subset, used until `kapsel-syntax`
//! lands. It builds `kapsel_ast` trees with exact spans so tests can write
//! scripts as text. It is not the product parser: no heredocs, no escapes in
//! double quotes, no error recovery.

use kapsel_ast::*;

pub fn parse(src: &str) -> Result<Script, Vec<ParseError>> {
    let mut p = P { b: src.as_bytes(), i: 0, errors: Vec::new() };
    let items = p.list(&[]);
    p.skip_seps();
    if p.i < p.b.len() {
        p.err("syntax", "unexpected input", p.i, p.b.len());
    }
    if p.errors.is_empty() {
        Ok(Script { items, span: Span::new(0, src.len() as u32) })
    } else {
        Err(p.errors)
    }
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
    errors: Vec<ParseError>,
}

fn sp(a: usize, b: usize) -> Span {
    Span::new(a as u32, b as u32)
}

impl P<'_> {
    fn err(&mut self, construct: &str, msg: &str, a: usize, b: usize) {
        self.errors.push(ParseError { construct: construct.into(), message: msg.into(), span: sp(a, b), expected: vec![], help: None });
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }
    fn skip_seps(&mut self) {
        while let Some(c) = self.peek() {
            if c == b'#' {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.i += 1;
                }
            } else if matches!(c, b' ' | b'\t' | b'\n' | b';') {
                self.i += 1;
            } else {
                break;
            }
        }
    }
    fn starts(&self, s: &str) -> bool {
        self.b[self.i..].starts_with(s.as_bytes())
    }
    fn at_keyword(&self, kw: &str) -> bool {
        self.starts(kw)
            && !self.b.get(self.i + kw.len()).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    }
    fn eat_keyword(&mut self, kw: &str) {
        self.skip_seps();
        if self.at_keyword(kw) {
            self.i += kw.len();
        } else {
            self.err("syntax", &format!("expected `{kw}`"), self.i, self.i);
        }
    }

    fn list(&mut self, stop: &[&str]) -> Vec<AndOrList> {
        let mut out = Vec::new();
        loop {
            self.skip_seps();
            if self.i >= self.b.len()
                || self.peek() == Some(b')')
                || stop.iter().any(|k| self.at_keyword(k))
            {
                break;
            }
            let before = self.i;
            out.push(self.and_or());
            if self.i == before {
                self.err("syntax", "stuck", self.i, self.i + 1);
                self.i += 1;
            }
        }
        out
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
            self.skip_seps();
            rest.push((op, self.pipeline()));
        }
        self.skip_ws();
        if self.peek() == Some(b'&') {
            self.err("background", "trailing `&` starts a background job", self.i, self.i + 1);
            self.i += 1;
        }
        AndOrList { first, rest, span: sp(start, self.i) }
    }

    fn pipeline(&mut self) -> Pipeline {
        self.skip_ws();
        let start = self.i;
        let mut negated = false;
        if self.peek() == Some(b'!') && matches!(self.b.get(self.i + 1), Some(b' ')) {
            negated = true;
            self.i += 1;
            self.skip_ws();
        }
        let mut commands = vec![self.command()];
        loop {
            self.skip_ws();
            if self.peek() == Some(b'|') && self.b.get(self.i + 1) != Some(&b'|') {
                self.i += 1;
                self.skip_seps();
                commands.push(self.command());
            } else {
                break;
            }
        }
        let end = match commands.last() {
            Some(Command::Simple(s)) => s.span.end,
            Some(Command::For(f)) => f.span.end,
            Some(Command::While(w)) => w.span.end,
            None => self.i as u32,
        };
        Pipeline { negated, commands, span: Span::new(start as u32, end) }
    }

    fn command(&mut self) -> Command {
        self.skip_ws();
        if self.at_keyword("for") {
            return Command::For(self.for_loop());
        }
        if self.at_keyword("while") || self.at_keyword("until") {
            return Command::While(self.while_loop());
        }
        for kw in ["if", "case"] {
            if self.at_keyword(kw) {
                let s = self.i;
                self.i += kw.len();
                self.err(kw, "not in the subset", s, self.i);
            }
        }
        Command::Simple(self.simple())
    }

    fn for_loop(&mut self) -> ForLoop {
        let start = self.i;
        self.i += 3;
        self.skip_ws();
        let var = self.name();
        self.eat_keyword("in");
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), None | Some(b'\n' | b';')) {
                break;
            }
            items.push(self.word());
        }
        self.eat_keyword("do");
        let body = self.list(&["done"]);
        self.eat_keyword("done");
        ForLoop { var, items, body, span: sp(start, self.i) }
    }

    fn while_loop(&mut self) -> WhileLoop {
        let start = self.i;
        let until = self.at_keyword("until");
        self.i += if until { 5 } else { 5 };
        let condition = self.list(&["do"]);
        self.eat_keyword("do");
        let body = self.list(&["done"]);
        self.eat_keyword("done");
        WhileLoop { until, condition, body, span: sp(start, self.i) }
    }

    fn name(&mut self) -> String {
        let s = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.b[s..self.i]).into_owned()
    }

    fn at_assignment(&self) -> bool {
        let mut j = self.i;
        if !self.b.get(j).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
            return false;
        }
        while self.b.get(j).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            j += 1;
        }
        self.b.get(j) == Some(&b'=')
    }

    fn end_of_command(&self) -> bool {
        matches!(self.peek(), None | Some(b'\n' | b';' | b'|' | b'&' | b')'))
    }

    fn simple(&mut self) -> SimpleCommand {
        self.skip_ws();
        let start = self.i;
        let mut s = SimpleCommand { assignments: vec![], words: vec![], redirects: vec![], span: Span::default() };
        let mut end = self.i;
        loop {
            self.skip_ws();
            if self.end_of_command() {
                break;
            }
            if let Some(r) = self.try_redirect() {
                end = r.span.end as usize;
                s.redirects.push(r);
            } else if s.words.is_empty() && self.at_assignment() {
                let a = self.i;
                let name = self.name();
                self.i += 1;
                let value = self.word();
                end = self.i;
                s.assignments.push(Assignment { name, value, span: sp(a, self.i) });
            } else {
                let w = self.word();
                end = w.span.end as usize;
                s.words.push(w);
            }
        }
        s.span = sp(start, end);
        s
    }

    fn try_redirect(&mut self) -> Option<Redirect> {
        let start = self.i;
        let mut j = self.i;
        while self.b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if !matches!(self.b.get(j), Some(b'<' | b'>')) {
            return None;
        }
        let fd = if j > self.i {
            std::str::from_utf8(&self.b[self.i..j]).ok()?.parse().ok()
        } else {
            None
        };
        self.i = j;
        let op = if self.starts(">>") {
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
        self.skip_ws();
        let target = if matches!(op, RedirectOp::DupOut | RedirectOp::DupIn)
            && self.peek().is_some_and(|c| c.is_ascii_digit())
        {
            let n = self.name();
            RedirectTarget::Fd { fd: n.parse().unwrap_or(0) }
        } else {
            RedirectTarget::Word { word: self.word() }
        };
        Some(Redirect { fd, op, target, span: sp(start, self.i) })
    }

    fn word_end(&self) -> bool {
        matches!(self.peek(), None | Some(b' ' | b'\t' | b'\n' | b';' | b'|' | b'&' | b'<' | b'>' | b')'))
    }

    fn word(&mut self) -> Word {
        let start = self.i;
        let mut parts: Vec<WordPart> = Vec::new();
        while !self.word_end() {
            let c = self.b[self.i];
            match c {
                b'\'' => {
                    let s = self.i + 1;
                    let e = self.b[s..].iter().position(|c| *c == b'\'').map_or(self.b.len(), |p| s + p);
                    parts.push(WordPart::SingleQuoted { text: String::from_utf8_lossy(&self.b[s..e]).into_owned() });
                    self.i = (e + 1).min(self.b.len());
                }
                b'"' => {
                    self.i += 1;
                    let mut inner = Vec::new();
                    while self.peek().is_some_and(|c| c != b'"') {
                        if self.peek() == Some(b'$') {
                            inner.push(self.dollar());
                        } else {
                            let s = self.i;
                            while self.peek().is_some_and(|c| c != b'"' && c != b'$') {
                                self.i += 1;
                            }
                            inner.push(WordPart::Literal { text: String::from_utf8_lossy(&self.b[s..self.i]).into_owned() });
                        }
                    }
                    self.i = (self.i + 1).min(self.b.len());
                    parts.push(WordPart::DoubleQuoted { parts: inner });
                }
                b'$' => parts.push(self.dollar()),
                b'*' => {
                    self.i += 1;
                    parts.push(WordPart::Glob { glob: GlobPart::Star });
                }
                b'?' => {
                    self.i += 1;
                    parts.push(WordPart::Glob { glob: GlobPart::Question });
                }
                b'[' => {
                    let s = self.i + 1;
                    let closes = self.b[s..].iter().take_while(|c| !matches!(c, b' ' | b'\t' | b'\n' | b';' | b'|' | b'&')).any(|c| *c == b']');
                    if !closes {
                        self.i += 1;
                        match parts.last_mut() {
                            Some(WordPart::Literal { text }) => text.push('['),
                            _ => parts.push(WordPart::Literal { text: "[".into() }),
                        }
                        continue;
                    }
                    let e = self.b[s..].iter().position(|c| *c == b']').map_or(self.b.len(), |p| s + p);
                    parts.push(WordPart::Glob { glob: GlobPart::Class { set: String::from_utf8_lossy(&self.b[s..e]).into_owned() } });
                    self.i = (e + 1).min(self.b.len());
                }
                _ => {
                    let s = self.i;
                    while !self.word_end() && !b"'\"$*?[".contains(&self.b[self.i]) {
                        self.i += 1;
                    }
                    if self.i == s {
                        self.i += 1;
                    }
                    let text = String::from_utf8_lossy(&self.b[s..self.i]).into_owned();
                    if let Some(WordPart::Literal { text: prev }) = parts.last_mut() {
                        prev.push_str(&text);
                    } else {
                        parts.push(WordPart::Literal { text });
                    }
                }
            }
        }
        Word { parts, span: sp(start, self.i) }
    }

    fn dollar(&mut self) -> WordPart {
        let s = self.i;
        self.i += 1;
        if self.peek() == Some(b'(') {
            self.i += 1;
            let inner_start = self.i;
            let items = self.list(&[]);
            let inner_end = self.i;
            if self.peek() == Some(b')') {
                self.i += 1;
            } else {
                self.err("syntax", "unclosed `$(`", s, self.i);
            }
            return WordPart::CommandSubst { script: Box::new(Script { items, span: sp(inner_start, inner_end) }) };
        }
        if self.peek().is_some_and(|c| b"?#@*".contains(&c)) {
            self.i += 1;
        } else {
            while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                self.i += 1;
            }
        }
        WordPart::Param { name: String::from_utf8_lossy(&self.b[s + 1..self.i]).into_owned() }
    }
}
