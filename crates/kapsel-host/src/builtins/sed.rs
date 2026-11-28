//! sed: -n -e -E/-r -i[SUFFIX]; addresses N, $, /re/, a,b, a,+N, !; commands p d q s.

use regex::bytes::Regex;

use super::regex_conv;
use super::{Ctx, Spec, getopt};

enum Addr {
    Line(usize),
    Last,
    Re(Regex),
}

enum Addr2 {
    A(Addr),
    Plus(usize),
}

enum RepPart {
    Lit(Vec<u8>),
    Group(usize),
}

enum Kind {
    P,
    D,
    Q,
    S {
        re: Regex,
        rep: Vec<RepPart>,
        global: bool,
        print: bool,
        nth: usize,
    },
}

struct Cmd {
    a1: Option<Addr>,
    a2: Option<Addr2>,
    neg: bool,
    kind: Kind,
    active: bool,
    end_line: usize,
}

const SUPPORTED: &str = "commands p d q s///[g p i I N]; addresses N $ /re/ N,M N,+M /re/,/re/, and ! after the address";

struct P<'a> {
    s: &'a [u8],
    i: usize,
    ere: bool,
}

impl P<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }
    fn delimited(&mut self, delim: u8, is_regex: bool) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(format!(
                    "unterminated expression (missing closing '{}')",
                    delim as char
                ));
            };
            self.i += 1;
            if c == delim {
                return Ok(out);
            }
            if c == b'\\' {
                let Some(n) = self.peek() else {
                    return Err("trailing backslash".into());
                };
                self.i += 1;
                if n == delim && is_regex {
                    out.push(n);
                } else {
                    out.push(b'\\');
                    out.push(n);
                }
                continue;
            }
            out.push(c);
        }
    }
    fn number(&mut self) -> Option<usize> {
        let st = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        if st == self.i {
            return None;
        }
        std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()
    }
    fn addr(&mut self) -> Result<Option<Addr>, String> {
        match self.peek() {
            Some(b'0'..=b'9') => Ok(Some(Addr::Line(self.number().unwrap()))),
            Some(b'$') => {
                self.i += 1;
                Ok(Some(Addr::Last))
            }
            Some(b'/') => {
                self.i += 1;
                let pat = self.delimited(b'/', true)?;
                let icase = if self.peek() == Some(b'I') {
                    self.i += 1;
                    true
                } else {
                    false
                };
                let pat = String::from_utf8_lossy(&pat).into_owned();
                let re = regex_conv::compile(&pat, self.ere, icase)
                    .map_err(|e| format!("invalid regex '{pat}': {e}"))?;
                Ok(Some(Addr::Re(re)))
            }
            _ => Ok(None),
        }
    }
}

fn parse_replacement(r: &[u8]) -> Vec<RepPart> {
    let mut parts = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < r.len() {
        match r[i] {
            b'&' => {
                if !cur.is_empty() {
                    parts.push(RepPart::Lit(std::mem::take(&mut cur)));
                }
                parts.push(RepPart::Group(0));
            }
            b'\\' if i + 1 < r.len() => {
                i += 1;
                match r[i] {
                    c @ b'0'..=b'9' => {
                        if !cur.is_empty() {
                            parts.push(RepPart::Lit(std::mem::take(&mut cur)));
                        }
                        parts.push(RepPart::Group((c - b'0') as usize));
                    }
                    b'n' => cur.push(b'\n'),
                    b't' => cur.push(b'\t'),
                    c => cur.push(c),
                }
            }
            c => cur.push(c),
        }
        i += 1;
    }
    if !cur.is_empty() {
        parts.push(RepPart::Lit(cur));
    }
    parts
}

fn parse_script(script: &[u8], ere: bool) -> Result<Vec<Cmd>, String> {
    let mut p = P {
        s: script,
        i: 0,
        ere,
    };
    let mut cmds = Vec::new();
    loop {
        while matches!(p.peek(), Some(b' ' | b'\t' | b'\n' | b';')) {
            p.i += 1;
        }
        if p.peek().is_none() {
            break;
        }
        let a1 = p.addr()?;
        let mut a2 = None;
        if a1.is_some() {
            p.skip_ws();
            if p.peek() == Some(b',') {
                p.i += 1;
                p.skip_ws();
                if p.peek() == Some(b'+') {
                    p.i += 1;
                    a2 = Some(Addr2::Plus(
                        p.number().ok_or("expected a number after ','+")?,
                    ));
                } else {
                    a2 = Some(Addr2::A(
                        p.addr()?
                            .ok_or("unexpected ',': expected an address after it")?,
                    ));
                }
            }
        }
        p.skip_ws();
        let mut neg = false;
        while p.peek() == Some(b'!') {
            neg = true;
            p.i += 1;
            p.skip_ws();
        }
        let Some(c) = p.peek() else {
            return Err("missing command".into());
        };
        p.i += 1;
        let kind = match c {
            b'p' => Kind::P,
            b'd' => Kind::D,
            b'q' => Kind::Q,
            b's' => {
                let Some(delim) = p.peek() else {
                    return Err("unterminated 's' command".into());
                };
                p.i += 1;
                let pat = p.delimited(delim, true)?;
                let rep = p.delimited(delim, false)?;
                let (mut global, mut print, mut icase, mut nth) = (false, false, false, 0usize);
                loop {
                    match p.peek() {
                        Some(b'g') => global = true,
                        Some(b'p') => print = true,
                        Some(b'i' | b'I') => icase = true,
                        Some(b'0'..=b'9') => {
                            nth = p.number().unwrap();
                            continue;
                        }
                        _ => break,
                    }
                    p.i += 1;
                }
                let pat = String::from_utf8_lossy(&pat).into_owned();
                if pat.is_empty() {
                    return Err(
                        "empty regex in 's' (reusing the last regex is not supported)".into(),
                    );
                }
                let re = regex_conv::compile(&pat, ere, icase)
                    .map_err(|e| format!("invalid regex '{pat}': {e}"))?;
                Kind::S {
                    re,
                    rep: parse_replacement(&rep),
                    global,
                    print,
                    nth: nth.max(1),
                }
            }
            other => {
                return Err(format!(
                    "unsupported command '{}'; kapsel's sed supports: {SUPPORTED}",
                    other as char
                ));
            }
        };
        p.skip_ws();
        match p.peek() {
            None | Some(b';' | b'\n') => {}
            Some(x) => return Err(format!("extra characters after command: '{}'", x as char)),
        }
        cmds.push(Cmd {
            a1,
            a2,
            neg,
            kind,
            active: false,
            end_line: 0,
        });
    }
    Ok(cmds)
}

fn addr_match(a: &Addr, line: usize, last: bool, ps: &[u8]) -> bool {
    match a {
        Addr::Line(n) => *n == line,
        Addr::Last => last,
        Addr::Re(r) => r.is_match(ps),
    }
}

fn selected(c: &mut Cmd, line: usize, last: bool, ps: &[u8]) -> bool {
    let m = match (&c.a1, &c.a2) {
        (None, _) => true,
        (Some(a), None) => addr_match(a, line, last, ps),
        (Some(a1), Some(a2)) => {
            if !c.active {
                if addr_match(a1, line, last, ps) {
                    match a2 {
                        Addr2::Plus(k) => {
                            c.end_line = line + k;
                            c.active = *k > 0;
                        }
                        Addr2::A(Addr::Line(n)) => c.active = *n > line,
                        Addr2::A(Addr::Last) => c.active = !last,
                        Addr2::A(Addr::Re(_)) => c.active = true,
                    }
                    true
                } else {
                    false
                }
            } else {
                match a2 {
                    Addr2::Plus(_) => {
                        if line >= c.end_line {
                            c.active = false;
                        }
                    }
                    Addr2::A(Addr::Line(n)) => {
                        if line >= *n {
                            c.active = false;
                        }
                    }
                    Addr2::A(Addr::Last) => {
                        if last {
                            c.active = false;
                        }
                    }
                    Addr2::A(Addr::Re(r)) => {
                        if r.is_match(ps) {
                            c.active = false;
                        }
                    }
                }
                true
            }
        }
    };
    m != c.neg
}

struct Output {
    buf: Vec<u8>,
    pending_nl: bool,
}

impl Output {
    fn line(&mut self, ps: &[u8], has_nl: bool) {
        if self.pending_nl {
            self.buf.push(b'\n');
            self.pending_nl = false;
        }
        self.buf.extend_from_slice(ps);
        if has_nl {
            self.buf.push(b'\n');
        } else {
            self.pending_nl = true;
        }
    }
}

/// Runs the script over `data`. Returns the output.
fn process(cmds: &mut [Cmd], quiet: bool, data: &[u8]) -> Vec<u8> {
    let ls = super::text::lines(data);
    let total = ls.len();
    let ends_nl = data.ends_with(b"\n");
    let mut out = Output {
        buf: Vec::new(),
        pending_nl: false,
    };
    'lines: for (idx, l) in ls.iter().enumerate() {
        let lineno = idx + 1;
        let last = lineno == total;
        let has_nl = !last || ends_nl;
        let mut ps = l.to_vec();
        for c in cmds.iter_mut() {
            if !selected(c, lineno, last, &ps) {
                continue;
            }
            match &c.kind {
                Kind::P => out.line(&ps, has_nl),
                Kind::D => continue 'lines,
                Kind::Q => {
                    if !quiet {
                        out.line(&ps, has_nl);
                    }
                    return out.buf;
                }
                Kind::S {
                    re,
                    rep,
                    global,
                    print,
                    nth,
                } => {
                    let mut res = Vec::new();
                    let mut pos = 0;
                    let mut count = 0;
                    let mut replaced = false;
                    for caps in re.captures_iter(&ps) {
                        let m = caps.get(0).unwrap();
                        count += 1;
                        if count < *nth {
                            continue;
                        }
                        res.extend_from_slice(&ps[pos..m.start()]);
                        for part in rep {
                            match part {
                                RepPart::Lit(b) => res.extend_from_slice(b),
                                RepPart::Group(g) => {
                                    if let Some(gm) = caps.get(*g) {
                                        res.extend_from_slice(gm.as_bytes());
                                    }
                                }
                            }
                        }
                        pos = m.end();
                        replaced = true;
                        if !*global {
                            break;
                        }
                    }
                    if replaced {
                        res.extend_from_slice(&ps[pos..]);
                        ps = res;
                        if *print {
                            out.line(&ps, has_nl);
                        }
                    }
                }
            }
        }
        if !quiet {
            out.line(&ps, has_nl);
        }
    }
    out.buf
}

pub fn sed(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "ne:Eri?",
        long: &[
            ("quiet", 'n'),
            ("silent", 'n'),
            ("expression", 'e'),
            ("regexp-extended", 'E'),
            ("in-place", 'i'),
        ],
        numeric: false,
        supported: "-n -e SCRIPT -E (-r) -i[SUFFIX]",
    };
    let Ok(p) = getopt(ctx, a, &spec) else {
        return 1;
    };
    let mut operands = p.operands.clone();
    let mut script: Vec<String> = p.vals('e').iter().map(|s| s.to_string()).collect();
    if script.is_empty() {
        if operands.is_empty() {
            ctx.msg("no script specified; usage: sed [-n] [-E] [-i[SUFFIX]] [-e SCRIPT] SCRIPT [FILE...]");
            return 1;
        }
        script.push(operands.remove(0));
    }
    let ere = p.has('E') || p.has('r');
    let src = script.join("\n");
    let mut cmds = match parse_script(src.as_bytes(), ere) {
        Ok(c) => c,
        Err(e) => {
            ctx.msg(&format!("-e expression: {e}"));
            return 1;
        }
    };
    let quiet = p.has('n');
    let inplace = p.has('i');
    if inplace && operands.is_empty() {
        ctx.msg("no input files (-i needs file operands)");
        return 1;
    }
    let files = if operands.is_empty() {
        vec!["-".to_string()]
    } else {
        operands
    };
    if inplace {
        let suffix = p.val('i').map(|s| s.to_string());
        let mut status = 0;
        for f in &files {
            let Some(path) = ctx.wpath(f, true) else {
                status = status.max(1);
                continue;
            };
            let data = match std::fs::read(&path) {
                Ok(d) => d,
                Err(e) => {
                    ctx.io_fail(
                        f,
                        &format!("can't read {f}: {}", crate::shell::errstr(&e)),
                        &e,
                    );
                    status = 2;
                    continue;
                }
            };
            if let Some(sfx) = &suffix {
                let bname = format!("{f}{sfx}");
                let Some(bp) = ctx.wpath(&bname, true) else {
                    status = status.max(1);
                    continue;
                };
                if let Err(e) = std::fs::write(&bp, &data) {
                    ctx.io_msg(&bname, &e);
                    status = status.max(1);
                    continue;
                }
            }
            for c in cmds.iter_mut() {
                c.active = false;
            }
            let out = process(&mut cmds, quiet, &data);
            if let Err(e) = std::fs::write(&path, out) {
                ctx.io_msg(f, &e);
                status = status.max(1);
            }
        }
        return status;
    }
    let mut data = Vec::new();
    let mut status = 0;
    for f in &files {
        let d = if f == "-" {
            ctx.stdin_all()
        } else {
            // A path outside the grants is kapsel's refusal (exit 1); an
            // input that cannot be opened is GNU's exit 2, in GNU's words.
            let Some(path) = ctx.rpath(f) else {
                status = status.max(1);
                continue;
            };
            match std::fs::read(&path) {
                Ok(d) => d,
                Err(e) => {
                    ctx.io_fail(
                        f,
                        &format!("can't read {f}: {}", crate::shell::errstr(&e)),
                        &e,
                    );
                    status = 2;
                    continue;
                }
            }
        };
        if !data.is_empty() && !data.ends_with(b"\n") {
            data.push(b'\n');
        }
        data.extend_from_slice(&d);
    }
    let out = process(&mut cmds, quiet, &data);
    ctx.put(&out);
    status
}
