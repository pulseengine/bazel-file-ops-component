//! The parts of Go's `encoding/json` that decide what TinyGo's file_ops
//! component accepts and what it says when it refuses.
//!
//! `Unmarshal` first runs a byte scanner over the whole input (syntax errors
//! win over type errors), then decodes into the target struct. Decoding
//! matches keys exactly, then case-insensitively; ignores unknown keys;
//! leaves a field unchanged on `null`; keeps the first type error and
//! carries on; and decodes a repeated array key into the elements already
//! there. This module reproduces each of those, and the error texts.

/// A syntactically valid JSON value. Numbers keep their source text.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The word Go's `UnmarshalTypeError` uses for this kind of value.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }
}

// ---------------------------------------------------------------------------
// Scanner: a port of encoding/json/scanner.go, for the error messages.
// ---------------------------------------------------------------------------

const MAX_NESTING_DEPTH: usize = 10000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Parse {
    ObjectKey,
    ObjectValue,
    ArrayValue,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum St {
    BeginValueOrEmpty,
    BeginValue,
    BeginStringOrEmpty,
    BeginString,
    EndValue,
    EndTop,
    InString,
    InStringEsc,
    InStringEscU,
    InStringEscU1,
    InStringEscU12,
    InStringEscU123,
    Neg,
    One,
    Zero,
    Dot,
    Dot0,
    E,
    ESign,
    E0,
    T,
    Tr,
    Tru,
    F,
    Fa,
    Fal,
    Fals,
    N,
    Nu,
    Nul,
    Error,
}

struct Scanner {
    step: St,
    parse: Vec<Parse>,
    end_top: bool,
    err: Option<String>,
}

fn is_space(c: u8) -> bool {
    c <= b' ' && (c == b' ' || c == b'\t' || c == b'\r' || c == b'\n')
}

/// Go's `quoteChar`: the offending byte, quoted the way `strconv.Quote`
/// quotes a one-byte string.
fn quote_char(c: u8) -> String {
    if c == b'\'' {
        return "'\\''".to_string();
    }
    if c == b'"' {
        return "'\"'".to_string();
    }
    let inner = match c {
        0x07 => "\\a".to_string(),
        0x08 => "\\b".to_string(),
        0x0c => "\\f".to_string(),
        b'\n' => "\\n".to_string(),
        b'\r' => "\\r".to_string(),
        b'\t' => "\\t".to_string(),
        0x0b => "\\v".to_string(),
        b'\\' => "\\\\".to_string(),
        0x20..=0x7e => (c as char).to_string(),
        _ => format!("\\x{c:02x}"),
    };
    format!("'{inner}'")
}

impl Scanner {
    fn new() -> Self {
        Scanner {
            step: St::BeginValue,
            parse: Vec::new(),
            end_top: false,
            err: None,
        }
    }

    fn error(&mut self, c: u8, context: &str) {
        self.step = St::Error;
        self.err = Some(format!("invalid character {} {}", quote_char(c), context));
    }

    fn push(&mut self, c: u8, p: Parse) {
        self.parse.push(p);
        if self.parse.len() > MAX_NESTING_DEPTH {
            self.error(c, "exceeded max depth");
        }
    }

    fn pop(&mut self) {
        self.parse.pop();
        if self.parse.is_empty() {
            self.step = St::EndTop;
            self.end_top = true;
        } else {
            self.step = St::EndValue;
        }
    }

    /// Feed one byte; returns false on error.
    fn feed(&mut self, c: u8) -> bool {
        self.run(self.step, c);
        self.step != St::Error
    }

    fn run(&mut self, st: St, c: u8) {
        match st {
            St::Error => {}
            St::BeginValueOrEmpty => {
                if is_space(c) {
                    return;
                }
                if c == b']' {
                    self.run(St::EndValue, c);
                } else {
                    self.run(St::BeginValue, c);
                }
            }
            St::BeginValue => {
                if is_space(c) {
                    return;
                }
                match c {
                    b'{' => {
                        self.step = St::BeginStringOrEmpty;
                        self.push(c, Parse::ObjectKey);
                    }
                    b'[' => {
                        self.step = St::BeginValueOrEmpty;
                        self.push(c, Parse::ArrayValue);
                    }
                    b'"' => self.step = St::InString,
                    b'-' => self.step = St::Neg,
                    b'0' => self.step = St::Zero,
                    b't' => self.step = St::T,
                    b'f' => self.step = St::F,
                    b'n' => self.step = St::N,
                    b'1'..=b'9' => self.step = St::One,
                    _ => self.error(c, "looking for beginning of value"),
                }
            }
            St::BeginStringOrEmpty => {
                if is_space(c) {
                    return;
                }
                if c == b'}' {
                    let n = self.parse.len();
                    self.parse[n - 1] = Parse::ObjectValue;
                    self.run(St::EndValue, c);
                } else {
                    self.run(St::BeginString, c);
                }
            }
            St::BeginString => {
                if is_space(c) {
                    return;
                }
                if c == b'"' {
                    self.step = St::InString;
                } else {
                    self.error(c, "looking for beginning of object key string");
                }
            }
            St::EndValue => {
                let n = self.parse.len();
                if n == 0 {
                    self.step = St::EndTop;
                    self.end_top = true;
                    self.run(St::EndTop, c);
                    return;
                }
                if is_space(c) {
                    self.step = St::EndValue;
                    return;
                }
                match self.parse[n - 1] {
                    Parse::ObjectKey => {
                        if c == b':' {
                            self.parse[n - 1] = Parse::ObjectValue;
                            self.step = St::BeginValue;
                        } else {
                            self.error(c, "after object key");
                        }
                    }
                    Parse::ObjectValue => {
                        if c == b',' {
                            self.parse[n - 1] = Parse::ObjectKey;
                            self.step = St::BeginString;
                        } else if c == b'}' {
                            self.pop();
                        } else {
                            self.error(c, "after object key:value pair");
                        }
                    }
                    Parse::ArrayValue => {
                        if c == b',' {
                            self.step = St::BeginValue;
                        } else if c == b']' {
                            self.pop();
                        } else {
                            self.error(c, "after array element");
                        }
                    }
                }
            }
            St::EndTop => {
                if !is_space(c) {
                    self.error(c, "after top-level value");
                }
            }
            St::InString => {
                if c == b'"' {
                    self.step = St::EndValue;
                } else if c == b'\\' {
                    self.step = St::InStringEsc;
                } else if c < 0x20 {
                    self.error(c, "in string literal");
                }
            }
            St::InStringEsc => match c {
                b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => self.step = St::InString,
                b'u' => self.step = St::InStringEscU,
                _ => self.error(c, "in string escape code"),
            },
            St::InStringEscU | St::InStringEscU1 | St::InStringEscU12 | St::InStringEscU123 => {
                if c.is_ascii_hexdigit() {
                    self.step = match st {
                        St::InStringEscU => St::InStringEscU1,
                        St::InStringEscU1 => St::InStringEscU12,
                        St::InStringEscU12 => St::InStringEscU123,
                        _ => St::InString,
                    };
                } else {
                    self.error(c, "in \\u hexadecimal character escape");
                }
            }
            St::Neg => match c {
                b'0' => self.step = St::Zero,
                b'1'..=b'9' => self.step = St::One,
                _ => self.error(c, "in numeric literal"),
            },
            St::One => {
                if c.is_ascii_digit() {
                    self.step = St::One;
                } else {
                    self.run(St::Zero, c);
                }
            }
            St::Zero => match c {
                b'.' => self.step = St::Dot,
                b'e' | b'E' => self.step = St::E,
                _ => self.run(St::EndValue, c),
            },
            St::Dot => {
                if c.is_ascii_digit() {
                    self.step = St::Dot0;
                } else {
                    self.error(c, "after decimal point in numeric literal");
                }
            }
            St::Dot0 => match c {
                b'0'..=b'9' => {}
                b'e' | b'E' => self.step = St::E,
                _ => self.run(St::EndValue, c),
            },
            St::E => {
                if c == b'+' || c == b'-' {
                    self.step = St::ESign;
                } else {
                    self.run(St::ESign, c);
                }
            }
            St::ESign => {
                if c.is_ascii_digit() {
                    self.step = St::E0;
                } else {
                    self.error(c, "in exponent of numeric literal");
                }
            }
            St::E0 => {
                if !c.is_ascii_digit() {
                    self.run(St::EndValue, c);
                }
            }
            St::T => self.lit(c, b'r', St::Tr, "in literal true (expecting 'r')"),
            St::Tr => self.lit(c, b'u', St::Tru, "in literal true (expecting 'u')"),
            St::Tru => self.lit(c, b'e', St::EndValue, "in literal true (expecting 'e')"),
            St::F => self.lit(c, b'a', St::Fa, "in literal false (expecting 'a')"),
            St::Fa => self.lit(c, b'l', St::Fal, "in literal false (expecting 'l')"),
            St::Fal => self.lit(c, b's', St::Fals, "in literal false (expecting 's')"),
            St::Fals => self.lit(c, b'e', St::EndValue, "in literal false (expecting 'e')"),
            St::N => self.lit(c, b'u', St::Nu, "in literal null (expecting 'u')"),
            St::Nu => self.lit(c, b'l', St::Nul, "in literal null (expecting 'l')"),
            St::Nul => self.lit(c, b'l', St::EndValue, "in literal null (expecting 'l')"),
        }
    }

    fn lit(&mut self, c: u8, want: u8, next: St, context: &str) {
        if c == want {
            self.step = next;
        } else {
            self.error(c, context);
        }
    }

    fn eof(&mut self) -> Result<(), String> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.end_top {
            return Ok(());
        }
        self.run(self.step, b' ');
        if self.end_top {
            return Ok(());
        }
        if self.err.is_none() {
            self.err = Some("unexpected end of JSON input".to_string());
        }
        Err(self.err.clone().unwrap_or_default())
    }
}

/// Go's `checkValid`: the syntax error `json.Unmarshal` reports, if any.
pub fn check_valid(data: &[u8]) -> Result<(), String> {
    let mut s = Scanner::new();
    for &c in data {
        if !s.feed(c) {
            return Err(s.err.unwrap_or_default());
        }
    }
    s.eof()
}

// ---------------------------------------------------------------------------
// Value parser (input already checked).
// ---------------------------------------------------------------------------

/// Parse input that [`check_valid`] accepted.
pub fn parse(data: &[u8]) -> Result<Value, String> {
    check_valid(data)?;
    let mut p = Parser { d: data, i: 0 };
    Ok(p.value())
}

struct Parser<'a> {
    d: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.d.len() && is_space(self.d[self.i]) {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Value {
        self.ws();
        let c = self.d[self.i];
        match c {
            b'{' => {
                self.i += 1;
                let mut pairs = Vec::new();
                self.ws();
                if self.d[self.i] == b'}' {
                    self.i += 1;
                    return Value::Object(pairs);
                }
                loop {
                    self.ws();
                    let k = self.string();
                    self.ws();
                    self.i += 1; // ':'
                    let v = self.value();
                    pairs.push((k, v));
                    self.ws();
                    let c = self.d[self.i];
                    self.i += 1;
                    if c == b'}' {
                        return Value::Object(pairs);
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.d[self.i] == b']' {
                    self.i += 1;
                    return Value::Array(items);
                }
                loop {
                    items.push(self.value());
                    self.ws();
                    let c = self.d[self.i];
                    self.i += 1;
                    if c == b']' {
                        return Value::Array(items);
                    }
                }
            }
            b'"' => Value::String(self.string()),
            b't' => {
                self.i += 4;
                Value::Bool(true)
            }
            b'f' => {
                self.i += 5;
                Value::Bool(false)
            }
            b'n' => {
                self.i += 4;
                Value::Null
            }
            _ => {
                let start = self.i;
                while self.i < self.d.len()
                    && matches!(
                        self.d[self.i],
                        b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
                    )
                {
                    self.i += 1;
                }
                Value::Number(String::from_utf8_lossy(&self.d[start..self.i]).into_owned())
            }
        }
    }

    /// Go's `unquote`: escapes decoded, each invalid UTF-8 byte and each
    /// unpaired surrogate becomes U+FFFD.
    fn string(&mut self) -> String {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            let c = self.d[self.i];
            match c {
                b'"' => {
                    self.i += 1;
                    return out;
                }
                b'\\' => {
                    let e = self.d[self.i + 1];
                    self.i += 2;
                    match e {
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let rr = self.hex4(self.i);
                            self.i += 4;
                            if (0xD800..0xE000).contains(&rr) {
                                let rr1 = if self.i + 6 <= self.d.len()
                                    && self.d[self.i] == b'\\'
                                    && self.d[self.i + 1] == b'u'
                                    && self.d[self.i + 2..self.i + 6]
                                        .iter()
                                        .all(u8::is_ascii_hexdigit)
                                {
                                    Some(self.hex4(self.i + 2))
                                } else {
                                    None
                                };
                                match rr1 {
                                    Some(lo)
                                        if (0xD800..0xDC00).contains(&rr)
                                            && (0xDC00..0xE000).contains(&lo) =>
                                    {
                                        let cp = 0x10000 + ((rr - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                        self.i += 6;
                                    }
                                    _ => out.push('\u{FFFD}'),
                                }
                            } else {
                                out.push(char::from_u32(rr).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => out.push(other as char), // '\\', '/', '"'
                    }
                }
                0x00..=0x7f => {
                    out.push(c as char);
                    self.i += 1;
                }
                _ => {
                    let end = (self.i + 4).min(self.d.len());
                    match std::str::from_utf8(&self.d[self.i..end]) {
                        Ok(s) => {
                            let ch = s.chars().next().unwrap_or('\u{FFFD}');
                            out.push(ch);
                            self.i += ch.len_utf8();
                        }
                        Err(e) if e.valid_up_to() > 0 => {
                            let s = std::str::from_utf8(&self.d[self.i..self.i + e.valid_up_to()])
                                .unwrap_or("");
                            let ch = s.chars().next().unwrap_or('\u{FFFD}');
                            out.push(ch);
                            self.i += ch.len_utf8();
                        }
                        Err(_) => {
                            out.push('\u{FFFD}');
                            self.i += 1;
                        }
                    }
                }
            }
        }
    }

    fn hex4(&self, at: usize) -> u32 {
        let s = std::str::from_utf8(&self.d[at..at + 4]).unwrap_or("0000");
        u32::from_str_radix(s, 16).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Decoder: struct-field matching, type errors with Go's error context.
// ---------------------------------------------------------------------------

const CASE_MASK: u8 = !0x20;

/// The case-insensitive comparison the component's `encoding/json` uses
/// (Go before 1.21, `foldFunc`): ASCII letters match either case, and in a
/// field name with an `s` or `k` the key may spell them as U+017F (long s)
/// or U+212A (Kelvin sign). No other non-ASCII rune matches; the
/// equivalence corpus pins this (`keys-fold-*`).
fn equal_fold(name: &str, key: &str) -> bool {
    let (s, t) = (name.as_bytes(), key.as_bytes());
    if name.bytes().any(|b| matches!(b, b's' | b'S' | b'k' | b'K')) {
        // equalFoldRight
        // `t` stays on a char boundary: it advances by one ASCII byte or by
        // one whole char.
        let mut t = key;
        for &sb in s {
            let Some(tr) = t.chars().next() else {
                return false;
            };
            if tr.is_ascii() {
                let tb = tr as u8;
                if sb != tb {
                    let up = sb & CASE_MASK;
                    if !up.is_ascii_uppercase() || up != tb & CASE_MASK {
                        return false;
                    }
                }
                t = &t[1..];
                continue;
            }
            let ok = match sb {
                b's' | b'S' => tr == '\u{017F}',
                b'k' | b'K' => tr == '\u{212A}',
                _ => false,
            };
            if !ok {
                return false;
            }
            t = &t[tr.len_utf8()..];
        }
        return t.is_empty();
    }
    if s.len() != t.len() {
        return false;
    }
    if name.bytes().any(|b| !b.is_ascii_alphabetic()) {
        // asciiEqualFold
        s.iter().zip(t).all(|(&sb, &tb)| {
            sb == tb || (sb.is_ascii_alphabetic() && sb & CASE_MASK == tb & CASE_MASK)
        })
    } else {
        // simpleLetterEqualFold
        s.iter()
            .zip(t)
            .all(|(&sb, &tb)| sb & CASE_MASK == tb & CASE_MASK)
    }
}

/// Find the field a key names: exact match first, then the first field, in
/// declaration order, that folds equal.
pub fn match_field<'a>(key: &str, fields: &[&'a str]) -> Option<&'a str> {
    if let Some(f) = fields.iter().find(|f| **f == key) {
        return Some(f);
    }
    fields.iter().find(|f| equal_fold(f, key)).copied()
}

/// Decoding state: the first type error and Go's error context.
#[derive(Default)]
pub struct Decoder {
    first_error: Option<String>,
    struct_name: Option<&'static str>,
    field_stack: Vec<&'static str>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an `UnmarshalTypeError`; only the first one is kept.
    pub fn type_error(&mut self, value: &str, type_name: &str) {
        if self.first_error.is_some() {
            return;
        }
        let msg = match self.struct_name {
            Some(s) => format!(
                "json: cannot unmarshal {value} into Go struct field {s}.{} of type {type_name}",
                self.field_stack.join(".")
            ),
            None if !self.field_stack.is_empty() => format!(
                "json: cannot unmarshal {value} into Go struct field .{} of type {type_name}",
                self.field_stack.join(".")
            ),
            None => format!("json: cannot unmarshal {value} into Go value of type {type_name}"),
        };
        self.first_error = Some(msg);
    }

    pub fn finish(self) -> Result<(), String> {
        match self.first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Decode an object into a struct named `struct_name` (Go type
    /// `type_name`). `field` is called with the canonical field name.
    pub fn object(
        &mut self,
        v: &Value,
        struct_name: &'static str,
        type_name: &str,
        fields: &[&'static str],
        mut field: impl FnMut(&mut Decoder, &'static str, &Value),
    ) {
        match v {
            Value::Null => {}
            Value::Object(pairs) => {
                let saved_struct = self.struct_name;
                let depth = self.field_stack.len();
                for (k, val) in pairs {
                    if let Some(name) = match_field(k, fields) {
                        self.field_stack.push(name);
                        self.struct_name = Some(struct_name);
                        field(self, name, val);
                        self.field_stack.truncate(depth);
                        self.struct_name = saved_struct;
                    }
                }
            }
            other => self.type_error(other.kind(), type_name),
        }
    }

    pub fn string(&mut self, v: &Value, out: &mut String) {
        match v {
            Value::Null => {}
            Value::String(s) => *out = s.clone(),
            other => self.type_error(other.kind(), "string"),
        }
    }

    pub fn opt_string(&mut self, v: &Value, out: &mut Option<String>) {
        match v {
            Value::Null => *out = None,
            Value::String(s) => *out = Some(s.clone()),
            other => self.type_error(other.kind(), "string"),
        }
    }

    pub fn boolean(&mut self, v: &Value, out: &mut bool) {
        match v {
            Value::Null => {}
            Value::Bool(b) => *out = *b,
            other => self.type_error(other.kind(), "bool"),
        }
    }

    /// A Go `int`-kind value: 32 bits in TinyGo's wasm build (4294967296
    /// does not fit; the corpus pins it).
    pub fn int(&mut self, v: &Value, out: &mut i64, type_name: &str) {
        match v {
            Value::Null => {}
            Value::Number(n) => match parse_go_int(n).filter(|i| i32::try_from(*i).is_ok()) {
                Some(i) => *out = i,
                None => self.type_error(&format!("number {n}"), type_name),
            },
            other => self.type_error(other.kind(), type_name),
        }
    }

    /// Decode an array into a Go slice, reusing elements as Go does.
    pub fn slice<T: Default + Clone>(
        &mut self,
        v: &Value,
        out: &mut GoSlice<T>,
        type_name: &str,
        mut elem: impl FnMut(&mut Decoder, &Value, &mut T),
    ) {
        match v {
            Value::Null => *out = GoSlice::default(),
            Value::Array(items) => {
                if items.is_empty() {
                    *out = GoSlice::default();
                    return;
                }
                for (i, item) in items.iter().enumerate() {
                    if i >= out.backing.len() {
                        out.backing.push(T::default());
                    }
                    elem(self, item, &mut out.backing[i]);
                }
                out.len = items.len();
            }
            other => self.type_error(other.kind(), type_name),
        }
    }
}

/// `strconv.ParseInt(s, 10, 64)` for text the scanner accepted as a number.
fn parse_go_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}

/// A Go slice: the elements past `len` survive a shrink, and a later decode
/// of a longer array into the same field reuses them, as Go's does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GoSlice<T> {
    backing: Vec<T>,
    len: usize,
}

impl<T: Clone> GoSlice<T> {
    pub fn to_vec(&self) -> Vec<T> {
        self.backing[..self.len].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_errors_match_go() {
        let cases: &[(&str, &str)] = &[
            ("", "unexpected end of JSON input"),
            ("{\"a\":1", "unexpected end of JSON input"),
            ("{\"a\":1} x", "invalid character 'x' after top-level value"),
            ("{\"a\" 1}", "invalid character '1' after object key"),
            (
                "{\"a\":1 \"b\"}",
                "invalid character '\"' after object key:value pair",
            ),
            (
                "{\"a\":1,}",
                "invalid character '}' looking for beginning of object key string",
            ),
            (
                "tru",
                "invalid character ' ' in literal true (expecting 'e')",
            ),
            (
                "[1,]",
                "invalid character ']' looking for beginning of value",
            ),
            (
                "\u{1}",
                "invalid character '\\x01' looking for beginning of value",
            ),
            ("01", "invalid character '1' after top-level value"),
            (
                "1.",
                "invalid character ' ' after decimal point in numeric literal",
            ),
            ("\"\\q\"", "invalid character 'q' in string escape code"),
        ];
        for (input, want) in cases {
            assert_eq!(
                check_valid(input.as_bytes()).unwrap_err(),
                *want,
                "input {input:?}"
            );
        }
        assert_eq!(
            check_valid(b"\xff").unwrap_err(),
            "invalid character '\\xff' looking for beginning of value"
        );
        assert!(check_valid(b" {\"a\":[1,2.5e-3,true,false,null,\"x\"]} ").is_ok());
    }

    #[test]
    fn strings_decode_like_go() {
        let v = parse(b"\"a\\u00e9\\ud83d\\ude00\\ud800x\xffz\"").unwrap();
        assert_eq!(
            v,
            Value::String("a\u{e9}\u{1F600}\u{FFFD}x\u{FFFD}z".into())
        );
    }

    #[test]
    fn folding_matches_go() {
        let f = ["workspace_dir", "operations"];
        assert_eq!(match_field("WORKSPACE_DIR", &f), Some("workspace_dir"));
        assert_eq!(
            match_field("work\u{17F}pace_dir", &f),
            Some("workspace_dir")
        );
        assert_eq!(
            match_field("wor\u{212A}space_dir", &f),
            Some("workspace_dir")
        );
        assert_eq!(match_field("workspace_d\u{131}r", &f), None);
        assert_eq!(match_field("workspacedir", &f), None);
        assert_eq!(match_field("tYpE", &["type", "path"]), Some("type"));
        assert_eq!(
            match_field("OUTPUT_FILE", &["output_file"]),
            Some("output_file")
        );
        assert_eq!(match_field("output-file", &["output_file"]), None);
    }
}
