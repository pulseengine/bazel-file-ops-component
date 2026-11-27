//! Shared checks for the golden and property tests.
#![allow(dead_code)]

use kapsel_syntax::ast::*;

/// Every AST span is inside the source, on char boundaries, non-empty
/// (except the documented cases) and inside its parent's span. Heredoc
/// bodies sit after their command line, so they are checked against the
/// enclosing script instead of the redirect.
pub fn check_spans(src: &str, script: &Script) -> Result<(), String> {
    let mut c = Checker {
        src,
        errors: Vec::new(),
    };
    c.script(script, None);
    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors.join("\n"))
    }
}

struct Checker<'a> {
    src: &'a str,
    errors: Vec<String>,
}

impl Checker<'_> {
    fn span(&mut self, what: &str, s: Span, parent: Option<Span>, allow_empty: bool) {
        let (a, b) = (s.start as usize, s.end as usize);
        if a > b
            || b > self.src.len()
            || !self.src.is_char_boundary(a)
            || !self.src.is_char_boundary(b)
        {
            self.errors.push(format!("{what}: bad span {a}..{b}"));
            return;
        }
        if a == b && !allow_empty {
            self.errors.push(format!("{what}: empty span at {a}"));
        }
        if let Some(p) = parent
            && (s.start < p.start || s.end > p.end)
        {
            self.errors.push(format!(
                "{what}: span {a}..{b} {:?} outside parent {}..{}",
                &self.src[a..b],
                p.start,
                p.end
            ));
        }
    }

    fn script(&mut self, s: &Script, parent: Option<Span>) {
        self.span("script", s.span, parent, s.items.is_empty());
        for i in &s.items {
            self.and_or(i, s.span, s.span);
        }
    }

    fn and_or(&mut self, a: &AndOrList, parent: Span, script: Span) {
        self.span("and-or", a.span, Some(parent), false);
        self.pipeline(&a.first, a.span, script);
        for (_, p) in &a.rest {
            self.pipeline(p, a.span, script);
        }
    }

    fn pipeline(&mut self, p: &Pipeline, parent: Span, script: Span) {
        self.span("pipeline", p.span, Some(parent), false);
        for c in &p.commands {
            match c {
                Command::Simple(s) => {
                    self.span("simple", s.span, Some(p.span), false);
                    for a in &s.assignments {
                        self.span("assignment", a.span, Some(s.span), false);
                        self.word(&a.value, a.span, script, true);
                    }
                    for w in &s.words {
                        self.word(w, s.span, script, false);
                    }
                    for r in &s.redirects {
                        self.span("redirect", r.span, Some(s.span), false);
                        match &r.target {
                            RedirectTarget::Word { word } => self.word(word, r.span, script, false),
                            RedirectTarget::Fd { .. } => {}
                            RedirectTarget::Heredoc { body, .. } => {
                                self.word(body, script, script, true)
                            }
                        }
                    }
                }
                Command::For(f) => {
                    self.span("for", f.span, Some(p.span), false);
                    for w in &f.items {
                        self.word(w, f.span, script, false);
                    }
                    for i in &f.body {
                        self.and_or(i, f.span, script);
                    }
                }
                Command::While(w) => {
                    self.span("while", w.span, Some(p.span), false);
                    for i in w.condition.iter().chain(&w.body) {
                        self.and_or(i, w.span, script);
                    }
                }
            }
        }
    }

    fn word(&mut self, w: &Word, parent: Span, script: Span, allow_empty: bool) {
        self.span("word", w.span, Some(parent), allow_empty);
        self.parts(&w.parts, w.span, script);
    }

    fn parts(&mut self, parts: &[WordPart], parent: Span, script: Span) {
        let _ = script;
        for p in parts {
            match p {
                WordPart::DoubleQuoted { parts } => self.parts(parts, parent, script),
                WordPart::CommandSubst { script: s } => self.script(s, Some(parent)),
                _ => {}
            }
        }
    }
}

/// Error spans are inside the source and on char boundaries; messages name
/// the construct.
pub fn check_errors(src: &str, errors: &[ParseError]) -> Result<(), String> {
    for e in errors {
        let (a, b) = (e.span.start as usize, e.span.end as usize);
        if a > b || b > src.len() || !src.is_char_boundary(a) || !src.is_char_boundary(b) {
            return Err(format!("bad error span {a}..{b} for {e:?}"));
        }
        if !e.message.contains(&e.construct) {
            return Err(format!("message does not name construct: {e:?}"));
        }
    }
    Ok(())
}

/// Human-reviewable rendering of an error list for `.expected` goldens.
pub fn render_errors(src: &str, errors: &[ParseError]) -> String {
    let mut out = String::new();
    for e in errors {
        let text = src
            .get(e.span.start as usize..e.span.end as usize)
            .unwrap_or("<bad span>");
        out.push_str(&format!(
            "error[{}] {}..{} {:?}\n",
            e.construct, e.span.start, e.span.end, text
        ));
        out.push_str(&format!("  message: {}\n", e.message));
        if !e.expected.is_empty() {
            out.push_str(&format!("  expected: {}\n", e.expected.join(", ")));
        }
        if let Some(h) = &e.help {
            out.push_str(&format!("  help: {h}\n"));
        }
    }
    out
}
