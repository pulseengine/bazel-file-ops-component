//! A small GitHub-expression evaluator for `runs-on` (subset: `||`, `&&`, `!`,
//! `==`, `!=`, parentheses, string literals, `contains`, `startsWith`,
//! `endsWith`, `fromJSON`, property access).
//!
//! It answers one question: with `github.event_name == 'pull_request'` and a
//! head repository that differs from `github.repository` (a fork PR), and with
//! the `strategy.matrix` values enumerated, can the expression select a
//! runner that is not a GitHub-hosted label? Everything else is an unknown
//! that may be truthy or falsy. Evaluation is over *sets* of possible values
//! and is lazy like GitHub's `&&` and `||`: an unreachable branch is not
//! evaluated, so it cannot make a correct guard fail.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Bool(bool),
    Str(String),
    Arr(Vec<V>),
    Null,
    /// `github.event.pull_request.head.repo.full_name`.
    Head,
    /// `github.repository`.
    Repo,
    /// Anything we cannot know: may be truthy or falsy, may be any label.
    Unknown,
    /// An unknown that was found truthy (it can be any label).
    UnknownTruthy,
    /// An unknown that was found falsy (a value GitHub cannot schedule on).
    Falsy,
}

impl V {
    fn can_truthy(&self) -> bool {
        match self {
            V::Bool(b) => *b,
            V::Str(s) => !s.is_empty(),
            V::Arr(_) | V::UnknownTruthy => true,
            V::Null | V::Falsy => false,
            V::Head | V::Repo | V::Unknown => true,
        }
    }
    fn can_falsy(&self) -> bool {
        match self {
            V::Bool(b) => !*b,
            V::Str(s) => s.is_empty(),
            V::Arr(_) | V::UnknownTruthy => false,
            V::Null | V::Falsy => true,
            V::Head | V::Repo | V::Unknown => true,
        }
    }
    fn truthy_of(&self) -> V {
        match self {
            V::Head | V::Repo | V::Unknown => V::UnknownTruthy,
            o => o.clone(),
        }
    }
    fn falsy_of(&self) -> V {
        match self {
            V::Head | V::Repo | V::Unknown => V::Falsy,
            o => o.clone(),
        }
    }
    fn is_unknownish(&self) -> bool {
        matches!(
            self,
            V::Head | V::Repo | V::Unknown | V::UnknownTruthy | V::Falsy
        )
    }
}

/// A GitHub-hosted runner label: `ubuntu-*`, `macos-*`, `windows-*`.
pub fn hosted_label(s: &str) -> bool {
    let l = s.trim().to_ascii_lowercase();
    !l.contains("self-hosted")
        && !l.contains(char::is_whitespace)
        && ["ubuntu-", "macos-", "windows-"]
            .iter()
            .any(|p| l.starts_with(p))
}

/// Can this final value of `runs-on` land on a non-hosted runner?
pub fn selects_non_hosted(v: &V) -> bool {
    match v {
        V::Str(s) => !hosted_label(s),
        V::Arr(items) => !(items.len() == 1 && matches!(&items[0], V::Str(s) if hosted_label(s))),
        V::Head | V::Repo | V::Unknown | V::UnknownTruthy => true,
        V::Bool(_) | V::Null | V::Falsy => false,
    }
}

#[derive(Debug, Clone)]
pub enum E {
    Lit(V),
    Prop(String),
    Call(String, Vec<E>),
    Not(Box<E>),
    Eq(Box<E>, Box<E>, bool),
    And(Box<E>, Box<E>),
    Or(Box<E>, Box<E>),
}

#[derive(Debug, Clone, PartialEq)]
enum T {
    Lp,
    Rp,
    Comma,
    Not,
    And,
    Or,
    Eq,
    Ne,
    Str(String),
    Id(String),
}

fn lex(s: &str) -> Result<Vec<T>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        match c[i] {
            ch if ch.is_whitespace() => i += 1,
            '(' => {
                out.push(T::Lp);
                i += 1
            }
            ')' => {
                out.push(T::Rp);
                i += 1
            }
            ',' => {
                out.push(T::Comma);
                i += 1
            }
            '&' if c.get(i + 1) == Some(&'&') => {
                out.push(T::And);
                i += 2
            }
            '|' if c.get(i + 1) == Some(&'|') => {
                out.push(T::Or);
                i += 2
            }
            '=' if c.get(i + 1) == Some(&'=') => {
                out.push(T::Eq);
                i += 2
            }
            '!' if c.get(i + 1) == Some(&'=') => {
                out.push(T::Ne);
                i += 2
            }
            '!' => {
                out.push(T::Not);
                i += 1
            }
            '\'' => {
                let mut v = String::new();
                i += 1;
                loop {
                    match c.get(i) {
                        None => return Err("unterminated string literal".into()),
                        Some('\'') if c.get(i + 1) == Some(&'\'') => {
                            v.push('\'');
                            i += 2
                        }
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some(ch) => {
                            v.push(*ch);
                            i += 1
                        }
                    }
                }
                out.push(T::Str(v));
            }
            ch if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.') => {
                let st = i;
                while i < c.len() && (c[i].is_alphanumeric() || matches!(c[i], '_' | '-' | '.')) {
                    i += 1;
                }
                out.push(T::Id(c[st..i].iter().collect()));
            }
            ch => return Err(format!("unsupported character `{ch}` in expression")),
        }
    }
    Ok(out)
}

struct P {
    t: Vec<T>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }
    fn eat(&mut self, t: &T) -> bool {
        if self.peek() == Some(t) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn or(&mut self) -> Result<E, String> {
        let mut l = self.and()?;
        while self.eat(&T::Or) {
            l = E::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<E, String> {
        let mut l = self.eq()?;
        while self.eat(&T::And) {
            l = E::And(Box::new(l), Box::new(self.eq()?));
        }
        Ok(l)
    }
    fn eq(&mut self) -> Result<E, String> {
        let mut l = self.unary()?;
        loop {
            let neg = if self.eat(&T::Eq) {
                false
            } else if self.eat(&T::Ne) {
                true
            } else {
                return Ok(l);
            };
            l = E::Eq(Box::new(l), Box::new(self.unary()?), neg);
        }
    }
    fn unary(&mut self) -> Result<E, String> {
        if self.eat(&T::Not) {
            return Ok(E::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<E, String> {
        match self.t.get(self.i).cloned() {
            Some(T::Lp) => {
                self.i += 1;
                let e = self.or()?;
                if !self.eat(&T::Rp) {
                    return Err("missing `)`".into());
                }
                Ok(e)
            }
            Some(T::Str(s)) => {
                self.i += 1;
                Ok(E::Lit(V::Str(s)))
            }
            Some(T::Id(id)) => {
                self.i += 1;
                if self.eat(&T::Lp) {
                    let mut args = Vec::new();
                    if !self.eat(&T::Rp) {
                        loop {
                            args.push(self.or()?);
                            if self.eat(&T::Rp) {
                                break;
                            }
                            if !self.eat(&T::Comma) {
                                return Err("expected `,` or `)` in call".into());
                            }
                        }
                    }
                    return Ok(E::Call(id.to_ascii_lowercase(), args));
                }
                Ok(match id.to_ascii_lowercase().as_str() {
                    "true" => E::Lit(V::Bool(true)),
                    "false" => E::Lit(V::Bool(false)),
                    "null" => E::Lit(V::Null),
                    l if l.chars().next().is_some_and(|c| c.is_ascii_digit()) => E::Lit(V::Str(id)),
                    l => E::Prop(l.to_string()),
                })
            }
            other => Err(format!("unexpected {other:?} in expression")),
        }
    }
}

pub fn parse(src: &str) -> Result<E, String> {
    let mut p = P { t: lex(src)?, i: 0 };
    let e = p.or()?;
    if p.i != p.t.len() {
        return Err("trailing tokens in expression".into());
    }
    Ok(e)
}

/// `matrix.<key>` names referenced (lowercased).
pub fn matrix_keys(e: &E, out: &mut Vec<String>) {
    match e {
        E::Prop(p) => {
            if let Some(k) = p.strip_prefix("matrix.") {
                let k = k.split('.').next().unwrap_or(k).to_string();
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        E::Call(_, a) => a.iter().for_each(|x| matrix_keys(x, out)),
        E::Not(a) => matrix_keys(a, out),
        E::Eq(a, b, _) | E::And(a, b) | E::Or(a, b) => {
            matrix_keys(a, out);
            matrix_keys(b, out);
        }
        E::Lit(_) => {}
    }
}

fn push(v: &mut Vec<V>, x: V) {
    if !v.contains(&x) {
        v.push(x);
    }
}

fn both() -> Vec<V> {
    vec![V::Bool(true), V::Bool(false)]
}

fn json_to_v(j: &serde_json::Value) -> V {
    match j {
        serde_json::Value::Null => V::Null,
        serde_json::Value::Bool(b) => V::Bool(*b),
        serde_json::Value::String(s) => V::Str(s.clone()),
        serde_json::Value::Number(n) => V::Str(n.to_string()),
        serde_json::Value::Array(a) => V::Arr(a.iter().map(json_to_v).collect()),
        serde_json::Value::Object(_) => V::Unknown,
    }
}

fn eq_pair(a: &V, b: &V) -> Vec<V> {
    match (a, b) {
        (V::Head, V::Repo) | (V::Repo, V::Head) => vec![V::Bool(false)],
        (V::Head, V::Head) | (V::Repo, V::Repo) => vec![V::Bool(true)],
        (V::Str(x), V::Str(y)) => vec![V::Bool(x.eq_ignore_ascii_case(y))],
        (V::Bool(x), V::Bool(y)) => vec![V::Bool(x == y)],
        (V::Null, V::Null) => vec![V::Bool(true)],
        (V::Null, V::Str(s)) | (V::Str(s), V::Null) => vec![V::Bool(s.is_empty())],
        _ => both(),
    }
}

pub fn eval(e: &E, matrix: &HashMap<String, V>) -> Vec<V> {
    match e {
        E::Lit(v) => vec![v.clone()],
        E::Prop(p) => vec![match p.as_str() {
            "github.event_name" => V::Str("pull_request".into()),
            "github.repository" => V::Repo,
            "github.event.pull_request.head.repo.full_name" => V::Head,
            p => match p.strip_prefix("matrix.") {
                Some(k) => matrix.get(k).cloned().unwrap_or(V::Unknown),
                None => V::Unknown,
            },
        }],
        E::Not(a) => {
            let mut out = Vec::new();
            for x in eval(a, matrix) {
                if x.can_truthy() {
                    push(&mut out, V::Bool(false));
                }
                if x.can_falsy() {
                    push(&mut out, V::Bool(true));
                }
            }
            out
        }
        E::Eq(a, b, neg) => {
            let (av, bv) = (eval(a, matrix), eval(b, matrix));
            let mut out = Vec::new();
            for x in &av {
                for y in &bv {
                    for r in eq_pair(x, y) {
                        match r {
                            V::Bool(r) => push(&mut out, V::Bool(r != *neg)),
                            o => push(&mut out, o),
                        }
                    }
                }
            }
            out
        }
        E::And(a, b) => {
            let mut out = Vec::new();
            let mut need_b = false;
            for x in eval(a, matrix) {
                if x.can_falsy() {
                    push(&mut out, x.falsy_of());
                }
                need_b |= x.can_truthy();
            }
            if need_b {
                for y in eval(b, matrix) {
                    push(&mut out, y);
                }
            }
            out
        }
        E::Or(a, b) => {
            let mut out = Vec::new();
            let mut need_b = false;
            for x in eval(a, matrix) {
                if x.can_truthy() {
                    push(&mut out, x.truthy_of());
                }
                need_b |= x.can_falsy();
            }
            if need_b {
                for y in eval(b, matrix) {
                    push(&mut out, y);
                }
            }
            out
        }
        E::Call(name, args) => call(name, args, matrix),
    }
}

fn call(name: &str, args: &[E], matrix: &HashMap<String, V>) -> Vec<V> {
    let ev = |i: usize| args.get(i).map(|a| eval(a, matrix)).unwrap_or_default();
    match (name, args.len()) {
        ("fromjson", 1) => {
            let mut out = Vec::new();
            for x in ev(0) {
                push(
                    &mut out,
                    match &x {
                        V::Str(s) => serde_json::from_str::<serde_json::Value>(s)
                            .map(|j| json_to_v(&j))
                            .unwrap_or(V::Unknown),
                        V::Arr(_) => x.clone(),
                        _ => V::Unknown,
                    },
                );
            }
            out
        }
        ("contains" | "startswith" | "endswith", 2) => {
            let (av, bv) = (ev(0), ev(1));
            let mut out = Vec::new();
            for x in &av {
                for y in &bv {
                    match (name, x, y) {
                        ("contains", V::Str(s), V::Str(t)) => push(
                            &mut out,
                            V::Bool(s.to_ascii_lowercase().contains(&t.to_ascii_lowercase())),
                        ),
                        ("contains", V::Arr(items), V::Str(t))
                            if !items.iter().any(V::is_unknownish) =>
                        {
                            push(
                                &mut out,
                                V::Bool(
                                    items.iter().any(
                                        |i| matches!(i, V::Str(s) if s.eq_ignore_ascii_case(t)),
                                    ),
                                ),
                            )
                        }
                        ("startswith", V::Str(s), V::Str(t)) => push(
                            &mut out,
                            V::Bool(s.to_ascii_lowercase().starts_with(&t.to_ascii_lowercase())),
                        ),
                        ("endswith", V::Str(s), V::Str(t)) => push(
                            &mut out,
                            V::Bool(s.to_ascii_lowercase().ends_with(&t.to_ascii_lowercase())),
                        ),
                        _ => both().into_iter().for_each(|b| push(&mut out, b)),
                    }
                }
            }
            out
        }
        _ => vec![V::Unknown],
    }
}

/// The `${{ ... }}` body when the whole string is one expression; the plain
/// string otherwise. A string that mixes text and expressions is refused.
pub enum Template {
    Plain(String),
    Expr(String),
}

pub fn template(s: &str) -> Result<Template, String> {
    let t = s.trim();
    if !t.contains("${{") {
        return Ok(Template::Plain(t.to_string()));
    }
    if !t.starts_with("${{") {
        return Err("text around a ${{ }} expression".into());
    }
    let c: Vec<char> = t.chars().collect();
    let mut i = 3;
    let mut in_str = false;
    while i < c.len() {
        if c[i] == '\'' {
            in_str = !in_str;
        } else if !in_str && c[i] == '}' && c.get(i + 1) == Some(&'}') {
            return if i + 2 == c.len() {
                Ok(Template::Expr(c[3..i].iter().collect()))
            } else {
                Err("text after the ${{ }} expression".into())
            };
        }
        i += 1;
    }
    Err("unterminated ${{ }}".into())
}
