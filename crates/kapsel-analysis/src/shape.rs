//! Normalized command shapes.
//!
//! A [`CommandShape`] is what the KS-S rules compare: the tool name, the flag
//! set in canonical form (and how each flag was spelled), the operands with
//! their class (literal, parameter, glob, capture), and the literal paths the
//! command reads, writes or removes. Quoting and whitespace are gone, so
//! `grep -n 'foo' a` and `grep  -n foo a` have the same shape.
//!
//! Everything here is a pure function of the AST: same input, same shape.

use std::collections::BTreeSet;

use kapsel_ast::{self as ast, Span};

/// How an operand or redirect target is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArgClass {
    /// Only literal text, quoted or not.
    Literal,
    /// Contains a parameter (`$X`) and no glob or capture.
    Var,
    /// Contains an unquoted glob.
    Glob,
    /// Contains a command substitution.
    Capture,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Arg {
    pub class: ArgClass,
    /// The text with quoting removed; parameters as `$NAME`, captures as
    /// `$(...)`, globs in fnmatch syntax.
    pub text: String,
    pub span: Span,
}

/// One flag in canonical form, and how it was written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Flag {
    /// Canonical name and value: `-n 5`, `-r`, `--color=auto`.
    pub canonical: String,
    /// The canonical name alone: `-n`, `-r`.
    pub name: String,
    /// How the name and value were spelled: `-NUM` (`head -5`), `-n VALUE`,
    /// `-nVALUE`, `--lines=VALUE`, or the literal flag letter (`-R`).
    pub form: String,
    /// Source text of the words that make up the flag.
    pub raw: String,
    pub span: Span,
}

/// A literal path and where it appears.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathUse {
    /// Normalized: no leading `./`, no trailing `/`, no `//`.
    pub path: String,
    pub span: Span,
}

/// The token that joins a command to the one before it in its and-or list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Connector {
    Start,
    Pipe,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandShape {
    /// The command name, or `None` for an assignment-only command or a
    /// dynamic name.
    pub tool: Option<String>,
    pub flags: Vec<Flag>,
    pub operands: Vec<Arg>,
    /// Redirects as `op target`, e.g. `> out.txt`, `2>&1`.
    pub redirects: Vec<String>,
    pub reads: Vec<PathUse>,
    pub writes: Vec<PathUse>,
    /// Paths removed (`rm` operands, `mv` sources).
    pub removes: Vec<PathUse>,
    /// Paths named without being read or written (`ls`, `test`, `cd`).
    pub mentions: Vec<PathUse>,
    /// Some path operand or target is not a literal.
    pub dynamic_paths: bool,
    pub vars_used: BTreeSet<String>,
    /// Variables this command sets for the rest of the script: assignment-only
    /// commands, `export X=...`, and the variable of a `for` loop.
    pub vars_set: BTreeSet<String>,
    pub connector: Connector,
    /// Nesting depth in `for`/`while` loops.
    pub loop_depth: u32,
    /// Index of the top-level and-or item this command belongs to.
    pub item: usize,
    /// Upstream stage key when this command reads a pipe.
    pub upstream: Option<String>,
    pub span: Span,
    /// Span of the command name (the whole command when it has none).
    pub tool_span: Span,
    /// Canonical identity: equal keys mean the same command up to quoting,
    /// whitespace and flag order.
    pub key: String,
}

impl CommandShape {
    pub fn tool(&self) -> Option<&str> {
        self.tool.as_deref()
    }

    /// Every literal path the command touches, in any role.
    pub fn all_paths(&self) -> impl Iterator<Item = &PathUse> {
        self.reads.iter().chain(&self.writes).chain(&self.removes).chain(&self.mentions)
    }

    /// True when running this command can change files, the working
    /// directory or variables seen by later commands.
    pub fn mutates(&self) -> bool {
        if !self.writes.is_empty() || !self.removes.is_empty() || !self.vars_set.is_empty() || self.dynamic_write() {
            return true;
        }
        match self.tool() {
            None => false,
            Some("cd" | "mkdir" | "cp" | "mv" | "rm" | "tee" | "export") => true,
            Some("sed") => self.flags.iter().any(|f| f.name == "-i"),
            // Not a built-in: its effects are unknown.
            Some(t) => !kapsel_plan::BUILTINS.contains(&t),
        }
    }

    /// True when the command writes somewhere the analysis cannot name.
    pub fn dynamic_write(&self) -> bool {
        self.dynamic_paths && matches!(self.tool(), Some("mkdir" | "cp" | "mv" | "rm" | "tee"))
            || self.redirects.iter().any(|r| (r.contains('>')) && r.contains('$'))
    }
}

/// Flags that take a value, per tool. A value-taking letter in a cluster
/// consumes the rest of the cluster, or the next word.
fn value_flags(tool: &str) -> &'static str {
    match tool {
        "head" | "tail" => "nc",
        "cut" => "dfcb",
        "sort" => "kto",
        "grep" => "efmABC",
        "sed" => "ef",
        "uniq" => "fs",
        "seq" => "sf",
        "date" => "d",
        _ => "",
    }
}

/// Letters that are aliases of another letter for this tool.
fn alias(tool: &str, letter: char) -> char {
    match (tool, letter) {
        ("grep", 'R') => 'r',
        _ => letter,
    }
}

/// Tools whose operands are text, not paths.
fn text_operands(tool: &str) -> bool {
    matches!(tool, "echo" | "printf" | "seq" | "date" | "sleep" | "exit" | "true" | "false" | ":" | "export" | "pwd" | "tr")
}

/// Normalizes a literal path for comparison.
pub fn norm_path(p: &str) -> String {
    let mut s = p;
    while let Some(r) = s.strip_prefix("./") {
        s = r;
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(c);
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

fn classify(word: &ast::Word) -> (ArgClass, String, BTreeSet<String>) {
    let mut class = ArgClass::Literal;
    let mut text = String::new();
    let mut vars = BTreeSet::new();
    fn walk(parts: &[ast::WordPart], class: &mut ArgClass, text: &mut String, vars: &mut BTreeSet<String>) {
        for p in parts {
            match p {
                ast::WordPart::Literal { text: t } | ast::WordPart::SingleQuoted { text: t } => text.push_str(t),
                ast::WordPart::DoubleQuoted { parts } => walk(parts, class, text, vars),
                ast::WordPart::Param { name } => {
                    vars.insert(name.clone());
                    text.push('$');
                    text.push_str(name);
                    if *class == ArgClass::Literal {
                        *class = ArgClass::Var;
                    }
                }
                ast::WordPart::CommandSubst { .. } => {
                    text.push_str("$(...)");
                    *class = ArgClass::Capture;
                }
                ast::WordPart::Glob { glob } => {
                    match glob {
                        ast::GlobPart::Star => text.push('*'),
                        ast::GlobPart::Question => text.push('?'),
                        ast::GlobPart::Class { set } => {
                            text.push('[');
                            text.push_str(set);
                            text.push(']');
                        }
                    }
                    if *class != ArgClass::Capture {
                        *class = ArgClass::Glob;
                    }
                }
            }
        }
    }
    walk(&word.parts, &mut class, &mut text, &mut vars);
    (class, text, vars)
}

/// Collects variables used inside command substitutions of a word, which
/// `classify` does not descend into.
fn capture_scripts(word: &ast::Word) -> Vec<&ast::Script> {
    fn walk<'a>(parts: &'a [ast::WordPart], out: &mut Vec<&'a ast::Script>) {
        for p in parts {
            match p {
                ast::WordPart::CommandSubst { script } => out.push(script),
                ast::WordPart::DoubleQuoted { parts } => walk(parts, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&word.parts, &mut out);
    out
}

fn excerpt(source: &str, span: Span) -> &str {
    source.get(span.start as usize..span.end as usize).unwrap_or("")
}

/// Computes the shapes of every simple command in `script`, in source order.
/// Commands inside `$(...)` come before the command that uses them, as they
/// run first. A `for` loop contributes a shape with no tool that sets its
/// variable.
pub fn shapes(source: &str, script: &ast::Script) -> Vec<CommandShape> {
    let mut out = Vec::new();
    for (i, item) in script.items.iter().enumerate() {
        walk_and_or(source, item, i, 0, &mut out);
    }
    out
}

fn walk_and_or(source: &str, list: &ast::AndOrList, item: usize, depth: u32, out: &mut Vec<CommandShape>) {
    walk_pipeline(source, &list.first, Connector::Start, item, depth, out);
    for (op, p) in &list.rest {
        let c = match op {
            ast::AndOrOp::And => Connector::And,
            ast::AndOrOp::Or => Connector::Or,
        };
        walk_pipeline(source, p, c, item, depth, out);
    }
}

fn walk_pipeline(
    source: &str,
    p: &ast::Pipeline,
    first: Connector,
    item: usize,
    depth: u32,
    out: &mut Vec<CommandShape>,
) {
    let mut upstream: Option<String> = None;
    for (i, cmd) in p.commands.iter().enumerate() {
        let conn = if i == 0 { first } else { Connector::Pipe };
        match cmd {
            ast::Command::Simple(sc) => {
                let shape = simple_shape(source, sc, conn, item, depth, upstream.take(), out);
                upstream = Some(shape.key.clone());
                out.push(shape);
            }
            ast::Command::For(f) => {
                let mut vars_used = BTreeSet::new();
                for w in &f.items {
                    vars_used.extend(classify(w).2);
                }
                let key = format!("for {} in {}", f.var, f.items.iter().map(|w| classify(w).1).collect::<Vec<_>>().join(" "));
                out.push(CommandShape {
                    tool: None,
                    flags: Vec::new(),
                    operands: Vec::new(),
                    redirects: Vec::new(),
                    reads: Vec::new(),
                    writes: Vec::new(),
                    removes: Vec::new(),
                    mentions: Vec::new(),
                    dynamic_paths: false,
                    vars_used,
                    vars_set: BTreeSet::from([f.var.clone()]),
                    connector: conn,
                    loop_depth: depth,
                    item,
                    upstream: None,
                    span: f.span,
                    tool_span: f.span,
                    key: key.clone(),
                });
                for l in &f.body {
                    walk_and_or(source, l, item, depth + 1, out);
                }
                upstream = Some(key);
            }
            ast::Command::While(w) => {
                for l in w.condition.iter().chain(&w.body) {
                    walk_and_or(source, l, item, depth + 1, out);
                }
                upstream = None;
            }
        }
    }
}

fn simple_shape(
    source: &str,
    sc: &ast::SimpleCommand,
    connector: Connector,
    item: usize,
    depth: u32,
    upstream: Option<String>,
    out: &mut Vec<CommandShape>,
) -> CommandShape {
    let mut vars_used = BTreeSet::new();
    // Commands inside $(...) first.
    for w in sc.words.iter().chain(sc.assignments.iter().map(|a| &a.value)) {
        for s in capture_scripts(w) {
            for (i, l) in s.items.iter().enumerate() {
                let _ = i;
                walk_and_or(source, l, item, depth, out);
            }
        }
    }
    for a in &sc.assignments {
        vars_used.extend(classify(&a.value).2);
    }

    let mut words = sc.words.iter();
    let name_word = words.next();
    let tool = name_word.and_then(|w| {
        let (class, text, _) = classify(w);
        (class == ArgClass::Literal).then_some(text)
    });
    let dynamic_name = name_word.is_some() && tool.is_none();
    let t = tool.clone().unwrap_or_default();

    let mut vars_set = BTreeSet::new();
    if sc.words.is_empty() {
        vars_set.extend(sc.assignments.iter().map(|a| a.name.clone()));
    }

    // Flags and operands.
    let mut flags = Vec::new();
    let mut operands = Vec::new();
    let rest: Vec<&ast::Word> = words.collect();
    let parse_flags = !matches!(t.as_str(), "echo" | "printf" | "test" | "[" | "export" | "exit");
    let vflags = value_flags(&t);
    let mut i = 0;
    let mut end_of_flags = !parse_flags;
    while i < rest.len() {
        let w = rest[i];
        let (class, text, vars) = classify(w);
        vars_used.extend(vars);
        if !end_of_flags && class == ArgClass::Literal && text == "--" {
            end_of_flags = true;
            i += 1;
            continue;
        }
        let is_flag = !end_of_flags && class == ArgClass::Literal && text.len() > 1 && text.starts_with('-');
        if !is_flag {
            operands.push(Arg { class, text, span: w.span });
            i += 1;
            continue;
        }
        if let Some(long) = text.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            // A few long spellings of short value flags.
            let short = match (t.as_str(), name.as_str()) {
                ("head" | "tail", "lines") => Some('n'),
                ("head" | "tail", "bytes") => Some('c'),
                ("grep", "recursive") => Some('r'),
                ("grep", "line-number") => Some('n'),
                ("sort", "numeric-sort") => Some('n'),
                ("sort", "reverse") => Some('r'),
                ("sort", "unique") => Some('u'),
                ("mkdir", "parents") => Some('p'),
                ("rm", "force") => Some('f'),
                ("rm", "recursive") => Some('r'),
                _ => None,
            };
            let (cname, canonical) = match (short, &value) {
                (Some(c), Some(v)) => (format!("-{c}"), format!("-{c} {v}")),
                (Some(c), None) => (format!("-{c}"), format!("-{c}")),
                (None, Some(v)) => (format!("--{name}"), format!("--{name}={v}")),
                (None, None) => (format!("--{name}"), format!("--{name}")),
            };
            let form = if value.is_some() { format!("--{name}=VALUE") } else { format!("--{name}") };
            flags.push(Flag { canonical, name: cname, form, raw: excerpt(source, w.span).to_string(), span: w.span });
            i += 1;
            continue;
        }
        let body = &text[1..];
        // head -5 / tail -5
        if matches!(t.as_str(), "head" | "tail") && body.chars().all(|c| c.is_ascii_digit()) {
            flags.push(Flag {
                canonical: format!("-n {body}"),
                name: "-n".into(),
                form: "-NUM".into(),
                raw: excerpt(source, w.span).to_string(),
                span: w.span,
            });
            i += 1;
            continue;
        }
        let chars: Vec<char> = body.chars().collect();
        let mut j = 0;
        let mut consumed_next = false;
        while j < chars.len() {
            let c = chars[j];
            let ca = alias(&t, c);
            if vflags.contains(c) {
                let attached: String = chars[j + 1..].iter().collect();
                let (value, form, span) = if !attached.is_empty() {
                    (attached, format!("-{c}VALUE"), w.span)
                } else if let Some(next) = rest.get(i + 1) {
                    consumed_next = true;
                    let (_, v, vars) = classify(next);
                    vars_used.extend(vars);
                    (v, format!("-{c} VALUE"), Span::new(w.span.start, next.span.end))
                } else {
                    (String::new(), format!("-{c}"), w.span)
                };
                flags.push(Flag {
                    canonical: format!("-{ca} {value}"),
                    name: format!("-{ca}"),
                    form,
                    raw: excerpt(source, span).to_string(),
                    span,
                });
                break;
            }
            flags.push(Flag {
                canonical: format!("-{ca}"),
                name: format!("-{ca}"),
                form: format!("-{c}"),
                raw: excerpt(source, w.span).to_string(),
                span: w.span,
            });
            j += 1;
        }
        i += if consumed_next { 2 } else { 1 };
    }

    // Paths by role.
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut removes = Vec::new();
    let mut mentions = Vec::new();
    let mut dynamic_paths = false;
    let has_flag = |n: &str| flags.iter().any(|f: &Flag| f.name == n);
    let path_ops: Vec<&Arg> = if text_operands(&t) || tool.is_none() {
        Vec::new()
    } else {
        // grep/sed take a pattern or script first unless -e/-f supplied it.
        let skip = match t.as_str() {
            "grep" if !has_flag("-e") && !has_flag("-f") => 1,
            "sed" if !has_flag("-e") && !has_flag("-f") => 1,
            _ => 0,
        };
        operands.iter().skip(skip).collect()
    };
    let push = |dst: &mut Vec<PathUse>, a: &Arg, dynamic: &mut bool| {
        if a.class == ArgClass::Literal {
            dst.push(PathUse { path: norm_path(&a.text), span: a.span });
        } else {
            *dynamic = true;
        }
    };
    match t.as_str() {
        "mkdir" | "tee" => path_ops.iter().for_each(|a| push(&mut writes, a, &mut dynamic_paths)),
        "rm" => path_ops.iter().for_each(|a| push(&mut removes, a, &mut dynamic_paths)),
        "cp" | "mv" => {
            if let Some((last, srcs)) = path_ops.split_last() {
                push(&mut writes, last, &mut dynamic_paths);
                for a in srcs {
                    push(&mut reads, a, &mut dynamic_paths);
                    if t == "mv" {
                        push(&mut removes, a, &mut dynamic_paths);
                    }
                }
            }
        }
        "uniq" => {
            for (k, a) in path_ops.iter().enumerate() {
                push(if k == 0 { &mut reads } else { &mut writes }, a, &mut dynamic_paths);
            }
        }
        "ls" | "test" | "[" | "cd" => {
            path_ops.iter().filter(|a| !a.text.starts_with('-') && a.text != "]").for_each(|a| push(&mut mentions, a, &mut dynamic_paths))
        }
        _ => path_ops.iter().for_each(|a| push(&mut reads, a, &mut dynamic_paths)),
    }
    if t == "sort" {
        for f in &flags {
            if f.name == "-o" {
                let v = f.canonical.trim_start_matches("-o ").to_string();
                writes.push(PathUse { path: norm_path(&v), span: f.span });
            }
        }
    }
    if t == "export" {
        for a in &operands {
            vars_set.insert(a.text.split('=').next().unwrap_or("").to_string());
        }
    }

    // Redirects.
    let mut redirects = Vec::new();
    for r in &sc.redirects {
        let fd = r.fd.map(|f| f.to_string()).unwrap_or_default();
        let op = match r.op {
            ast::RedirectOp::Read => "<",
            ast::RedirectOp::Write => ">",
            ast::RedirectOp::Append => ">>",
            ast::RedirectOp::DupOut => ">&",
            ast::RedirectOp::DupIn => "<&",
            ast::RedirectOp::Heredoc => "<<",
        };
        let target = match &r.target {
            ast::RedirectTarget::Word { word } => {
                let (class, text, vars) = classify(word);
                vars_used.extend(vars);
                let is_null = text == "/dev/null";
                if class == ArgClass::Literal && !is_null {
                    let pu = PathUse { path: norm_path(&text), span: word.span };
                    match r.op {
                        ast::RedirectOp::Read => reads.push(pu),
                        ast::RedirectOp::Write | ast::RedirectOp::Append => writes.push(pu),
                        _ => {}
                    }
                } else if class != ArgClass::Literal {
                    dynamic_paths = true;
                }
                text
            }
            ast::RedirectTarget::Fd { fd } => fd.to_string(),
            ast::RedirectTarget::Heredoc { body, .. } => {
                let (_, text, vars) = classify(body);
                vars_used.extend(vars);
                format!("[{}]", text)
            }
        };
        redirects.push(format!("{fd}{op}{target}"));
    }

    // Canonical key: tool, sorted canonical flags, operands, redirects.
    let mut canon: Vec<&str> = flags.iter().map(|f| f.canonical.as_str()).collect();
    canon.sort();
    canon.dedup();
    let mut key = String::new();
    for a in &sc.assignments {
        key.push_str(&format!("{}={} ", a.name, classify(&a.value).1));
    }
    if dynamic_name {
        key.push_str("$? ");
    }
    key.push_str(&t);
    for f in canon {
        key.push(' ');
        key.push_str(f);
    }
    if !flags.is_empty() || operands.iter().any(|a| a.text.starts_with('-')) {
        key.push_str(" --");
    }
    for a in &operands {
        key.push(' ');
        key.push_str(&quote_key(&a.text));
    }
    for r in &redirects {
        key.push(' ');
        key.push_str(&quote_key(r));
    }

    CommandShape {
        tool,
        flags,
        operands,
        redirects,
        reads,
        writes,
        removes,
        mentions,
        dynamic_paths,
        vars_used,
        vars_set,
        connector,
        loop_depth: depth,
        item,
        upstream,
        span: sc.span,
        tool_span: name_word.map(|w| w.span).unwrap_or(sc.span),
        key,
    }
}

/// Quotes a key component when it contains whitespace, so `a b` (one word)
/// and `a` `b` (two words) give different keys.
fn quote_key(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '\'') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::parse;

    fn one(src: &str) -> CommandShape {
        let s = parse(src);
        let mut v = shapes(src, &s);
        assert_eq!(v.len(), 1, "{v:#?}");
        v.remove(0)
    }

    #[test]
    fn quoting_whitespace_and_flag_order_do_not_change_the_key() {
        let a = one("grep -n -i foo a.txt");
        let b = one("grep  -i   -n 'foo' \"a.txt\"");
        let c = one("grep -in foo ./a.txt");
        assert_eq!(a.key, b.key);
        // ./a.txt is a different operand text but the same path.
        assert_ne!(a.key, c.key);
        assert_eq!(a.reads, vec![PathUse { path: "a.txt".into(), span: a.reads[0].span }]);
        assert_eq!(c.reads[0].path, "a.txt");
    }

    #[test]
    fn head_spellings_share_a_canonical_flag() {
        let a = one("head -5 f");
        let b = one("head -n 5 f");
        let c = one("head -n5 f");
        assert_eq!(a.key, b.key);
        assert_eq!(b.key, c.key);
        assert_eq!(a.flags[0].form, "-NUM");
        assert_eq!(b.flags[0].form, "-n VALUE");
        assert_eq!(c.flags[0].form, "-nVALUE");
    }

    #[test]
    fn classes_and_roles() {
        let s = one("cp $SRC out/*.txt dest/");
        assert_eq!(s.operands[0].class, ArgClass::Var);
        assert_eq!(s.operands[1].class, ArgClass::Glob);
        assert_eq!(s.writes[0].path, "dest");
        assert!(s.dynamic_paths);
        assert!(s.vars_used.contains("SRC"));
        let r = one("rm -f a.txt > log.txt");
        assert_eq!(r.removes[0].path, "a.txt");
        assert_eq!(r.writes[0].path, "log.txt");
        assert!(r.flags.iter().any(|f| f.canonical == "-f"));
    }

    #[test]
    fn shapes_are_deterministic() {
        let src = "cat a | sort -n > b; for f in x y; do cat $f; done; echo $(ls out)";
        let s = parse(src);
        assert_eq!(shapes(src, &s), shapes(src, &s));
        let keys: Vec<String> = shapes(src, &s).into_iter().map(|s| s.key).collect();
        assert_eq!(keys, ["cat a", "sort -n -- >b","for f in x y", "cat $f", "ls out", "echo $(...)"]);
    }
}
