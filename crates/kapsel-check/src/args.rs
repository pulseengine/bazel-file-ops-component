//! Built-in argument analysis: which words are options, which are paths, and
//! the path-danger test behind KS-R003. Anything this module does not
//! understand is reported as `unknown`, and the caller then marks the grants
//! `dynamic` instead of guessing (a grant list that is too short would let a
//! path through unreviewed).

use kapsel_ast::Span;
use kapsel_plan::{PPart, PWord};

/// The options of one built-in. A long option maps to the short letter that
/// names what it means (`--output` is `o`); `_` marks a value that is not a path.
pub struct Spec {
    /// Short flags without a value.
    pub sb: &'static str,
    /// Short flags that take a value (attached or as the next argument).
    pub sv: &'static str,
    /// Long flags without a value.
    pub lb: &'static [&'static str],
    /// Long options with a value (`--name=v` or `--name v`) and their key.
    pub lv: &'static [(&'static str, char)],
    /// `-5` style numeric options (head, tail).
    pub digits: bool,
}

const NONE: &[&str] = &[];

pub fn spec_for(tool: &str) -> Option<Spec> {
    let s = |sb, sv, lb, lv, digits| {
        Some(Spec {
            sb,
            sv,
            lb,
            lv,
            digits,
        })
    };
    match tool {
        "cd" => s("LP@e", "", NONE, &[], false),
        "cat" => s(
            "nbsAevtTuE",
            "",
            &[
                "number",
                "number-nonblank",
                "squeeze-blank",
                "show-all",
                "show-ends",
                "show-tabs",
                "show-nonprinting",
            ],
            &[],
            false,
        ),
        "head" => s(
            "qvz",
            "nc",
            &["quiet", "silent", "verbose"],
            &[("lines", '_'), ("bytes", '_')],
            true,
        ),
        "tail" => s(
            "qvzfF",
            "nc",
            &["quiet", "silent", "verbose", "follow"],
            &[("lines", '_'), ("bytes", '_')],
            true,
        ),
        "wc" => s(
            "lwcmL",
            "",
            &["lines", "words", "bytes", "chars", "max-line-length"],
            &[],
            false,
        ),
        "sort" => s(
            "bdfgiMhnRrsuVcCmz",
            "ktSTo",
            &[
                "unique",
                "reverse",
                "numeric-sort",
                "ignore-case",
                "stable",
                "check",
                "merge",
                "dictionary-order",
                "ignore-leading-blanks",
                "version-sort",
                "human-numeric-sort",
                "general-numeric-sort",
                "month-sort",
                "random-sort",
            ],
            &[
                ("key", 'k'),
                ("field-separator", 't'),
                ("output", 'o'),
                ("buffer-size", '_'),
                ("temporary-directory", 'T'),
            ],
            false,
        ),
        "uniq" => s(
            "cdiuD",
            "fsw",
            &["count", "repeated", "unique", "ignore-case"],
            &[
                ("skip-fields", '_'),
                ("skip-chars", '_'),
                ("check-chars", '_'),
            ],
            false,
        ),
        "cut" => s(
            "snz",
            "dfcb",
            &["only-delimited", "complement", "zero-terminated"],
            &[
                ("delimiter", '_'),
                ("fields", '_'),
                ("characters", '_'),
                ("bytes", '_'),
                ("output-delimiter", '_'),
            ],
            false,
        ),
        "ls" => s(
            "alAFhHiLrRStd1pGgonqsxCfkmuUcw",
            "",
            &[
                "all",
                "almost-all",
                "human-readable",
                "recursive",
                "reverse",
                "classify",
                "directory",
                "inode",
            ],
            &[("color", '_'), ("sort", '_'), ("time", '_')],
            false,
        ),
        "grep" => s(
            "EFGPivwxcLlnoqsrRHhIaUzbTZ",
            "efmABCdD",
            &[
                "ignore-case",
                "invert-match",
                "word-regexp",
                "line-regexp",
                "count",
                "files-with-matches",
                "files-without-match",
                "line-number",
                "only-matching",
                "quiet",
                "silent",
                "no-messages",
                "recursive",
                "dereference-recursive",
                "with-filename",
                "no-filename",
                "extended-regexp",
                "fixed-strings",
                "basic-regexp",
                "perl-regexp",
                "null-data",
                "text",
                "binary",
                "null",
                "no-ignore-case",
            ],
            &[
                ("regexp", 'e'),
                ("file", 'f'),
                ("max-count", '_'),
                ("after-context", '_'),
                ("before-context", '_'),
                ("context", '_'),
                ("include", '_'),
                ("exclude", '_'),
                ("exclude-dir", '_'),
                ("include-dir", '_'),
                ("color", '_'),
                ("colour", '_'),
                ("directories", '_'),
                ("devices", '_'),
            ],
            false,
        ),
        "sed" => s(
            "nrEsuz",
            "ef",
            &[
                "quiet",
                "silent",
                "regexp-extended",
                "separate",
                "null-data",
                "in-place",
                "posix",
            ],
            &[("expression", 'e'), ("file", 'f')],
            false,
        ),
        "cp" => s(
            "rRfipPaulvnHLTx",
            "t",
            &[
                "recursive",
                "force",
                "interactive",
                "preserve",
                "verbose",
                "no-clobber",
                "archive",
                "update",
                "no-target-directory",
            ],
            &[("target-directory", 't')],
            false,
        ),
        "mv" => s(
            "fiuvnT",
            "t",
            &[
                "force",
                "interactive",
                "update",
                "verbose",
                "no-clobber",
                "no-target-directory",
            ],
            &[("target-directory", 't')],
            false,
        ),
        "mkdir" => s("pv", "m", &["parents", "verbose"], &[("mode", '_')], false),
        "rm" => s(
            "rRfidvI",
            "",
            &[
                "recursive",
                "force",
                "interactive",
                "verbose",
                "dir",
                "preserve-root",
                "no-preserve-root",
            ],
            &[],
            false,
        ),
        "tee" => s("aip", "", &["append", "ignore-interrupts"], &[], false),
        _ => None,
    }
}

pub struct Split<'a> {
    /// Non-option arguments, with the source span of the word.
    pub positional: Vec<(&'a PWord, Span)>,
    /// Option arguments as written (the literal head of the word).
    pub opts: Vec<String>,
    /// Option key and its value (`-n 5`, `-ofile`, `--output=file`).
    pub values: Vec<(char, PWord, Span)>,
    /// Some option was not understood; the caller must assume anything.
    pub unknown: bool,
    /// An option-looking word has an expansion that is not consumed as an
    /// option value (`-$X`): it may be any flag, so operands cannot be told.
    pub opaque: bool,
}

/// The leading literal text of a word (adjacent literal parts joined, quoted
/// or not) and how many parts it spans.
pub fn leading_literal(w: &PWord) -> (String, usize) {
    let mut text = String::new();
    let mut n = 0;
    for p in &w.parts {
        match p {
            PPart::Lit { text: t, .. } => {
                text.push_str(t);
                n += 1;
            }
            _ => break,
        }
    }
    (text, n)
}

/// The word after the first `off` bytes of its leading literal text.
fn word_after(w: &PWord, mut off: usize) -> PWord {
    let mut parts = Vec::new();
    for p in &w.parts {
        match p {
            PPart::Lit { text, quoted } if off > 0 => {
                if off < text.len() {
                    parts.push(PPart::Lit {
                        text: text[off..].to_string(),
                        quoted: *quoted,
                    });
                    off = 0;
                } else {
                    off -= text.len();
                }
            }
            other => parts.push(other.clone()),
        }
    }
    PWord { parts }
}

/// Splits builtin arguments into options and positionals using `spec`. `--`
/// ends the options. A word whose leading literal text (adjacent literal
/// parts joined) starts with `-` is an option, even when an expansion
/// follows (`-$X`, `"-"rf`). An option word with an expansion that is not
/// consumed as a value is `unknown` and `opaque`.
pub fn split_args<'a>(spec: &Spec, args: &'a [PWord], spans: &[Span]) -> Split<'a> {
    let span_of = |i: usize| spans.get(i).copied().unwrap_or_default();
    let mut sp = Split {
        positional: Vec::new(),
        opts: Vec::new(),
        values: Vec::new(),
        unknown: false,
        opaque: false,
    };
    let mut after_dd = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let at = i;
        i += 1;
        if after_dd {
            sp.positional.push((a, span_of(at)));
            continue;
        }
        let (head, lead_parts) = leading_literal(a);
        let nonlit = lead_parts < a.parts.len();
        if !head.starts_with('-') || (head.len() == 1 && !nonlit) {
            sp.positional.push((a, span_of(at)));
            continue;
        }
        if a.as_literal().as_deref() == Some("--") {
            after_dd = true;
            continue;
        }
        sp.opts.push(a.as_literal().unwrap_or_else(|| head.clone()));
        let mut attached_used = false;
        if let Some(body) = head.strip_prefix("--") {
            let (name, eq) = match body.split_once('=') {
                Some((n, _)) => (n, true),
                None => (body, false),
            };
            if let Some((_, key)) = spec.lv.iter().find(|(n, _)| *n == name) {
                // `--name` with an expansion glued on and no `=` is some
                // other option name: never take the next argument for it.
                if !eq && nonlit {
                    sp.unknown = true;
                    sp.opaque = true;
                    continue;
                }
                let value = if eq {
                    attached_used = true;
                    word_after(a, 2 + name.len() + 1)
                } else if i < args.len() {
                    i += 1;
                    args[i - 1].clone()
                } else {
                    sp.unknown = true;
                    continue;
                };
                if *key != '_' {
                    sp.values.push((*key, value, span_of(at)));
                }
            } else if !(spec.lb.contains(&name) && !eq) {
                sp.unknown = true;
            }
        } else {
            for (idx, c) in head[1..].char_indices() {
                let after = 1 + idx + c.len_utf8();
                if spec.sv.contains(c) {
                    let attached = word_after(a, after);
                    let value = if !attached.parts.is_empty() {
                        attached_used = true;
                        attached
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        sp.unknown = true;
                        PWord::literal("")
                    };
                    sp.values.push((c, value, span_of(at)));
                    break;
                } else if c == 'i' && spec.lb.contains(&"in-place") {
                    // sed -i[SUFFIX]: the rest of the word is the suffix.
                    break;
                } else if spec.sb.contains(c) || (spec.digits && c.is_ascii_digit()) {
                    continue;
                } else {
                    sp.unknown = true;
                    break;
                }
            }
        }
        // Any expansion in an option word that was not taken as a value may
        // change what the option is.
        let value_has_expansion = attached_used;
        if nonlit && !value_has_expansion {
            sp.unknown = true;
            sp.opaque = true;
        }
    }
    sp
}

pub fn sed_in_place(o: &str) -> bool {
    o == "--in-place"
        || o.starts_with("--in-place=")
        || (o.starts_with('-')
            && !o.starts_with("--")
            && o[1..]
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .any(|c| c == 'i'))
}

// ---- KS-R003 -------------------------------------------------------------

#[derive(PartialEq)]
enum Anchor {
    Root,
    Home,
    Rel,
}

/// Normalizes a literal path: collapses `//`, `/./` and `x/..`. Returns the
/// anchor and the remaining components (leading `..` kept for relative and
/// home paths; above the root they vanish).
fn normalize(path: &str) -> (Anchor, Vec<&str>) {
    let anchor = if path.starts_with('/') {
        Anchor::Root
    } else if path == "~" || path.starts_with("~/") {
        Anchor::Home
    } else {
        Anchor::Rel
    };
    let mut comps: Vec<&str> = Vec::new();
    for (i, c) in path.split('/').enumerate() {
        if c.is_empty() || c == "." || (i == 0 && anchor == Anchor::Home) {
            continue;
        }
        if c == ".." {
            match comps.last() {
                Some(&l) if l != ".." => {
                    comps.pop();
                }
                _ => {
                    if anchor != Anchor::Root {
                        comps.push("..");
                    }
                }
            }
        } else {
            comps.push(c);
        }
    }
    (anchor, comps)
}

fn danger_of(path: &str) -> Option<&'static str> {
    let (anchor, comps) = normalize(path);
    let all_dotdot = comps.iter().all(|c| *c == "..");
    match anchor {
        Anchor::Root if comps.is_empty() => Some("is the filesystem root `/`"),
        Anchor::Home if comps.is_empty() => Some("is the home directory `~`"),
        Anchor::Home if comps[0] == ".." => Some("climbs out of the home directory `~`"),
        Anchor::Rel if comps.is_empty() => Some("is the current directory `.`"),
        Anchor::Rel if all_dotdot => Some("is the parent directory `..` or above"),
        _ => None,
    }
}

/// Why a recursive `rm` of this word is refused, if it is.
pub fn rm_danger(w: &PWord) -> Option<String> {
    let first = w
        .parts
        .iter()
        .find(|p| !matches!(p, PPart::Lit { text, .. } if text.is_empty()));
    match first {
        Some(PPart::Var { name, .. }) => {
            return Some(format!(
                "begins with `${name}`, which may be empty or unexpected"
            ));
        }
        Some(PPart::Capture { .. }) => {
            return Some(
                "begins with a command substitution, whose output may be empty".to_string(),
            );
        }
        _ => {}
    }
    if w.parts
        .iter()
        .all(|p| matches!(p, PPart::Lit { text, .. } if text.is_empty()))
    {
        return Some("is empty".to_string());
    }
    // Flatten with a marker for globs and for other expansions.
    let mut flat = String::new();
    let mut has_glob = false;
    for p in &w.parts {
        match p {
            PPart::Lit { text, .. } => flat.push_str(text),
            PPart::Glob { .. } => {
                has_glob = true;
                flat.push('\u{1}');
            }
            _ => flat.push('\u{2}'),
        }
    }
    if !has_glob {
        return danger_of(&flat).map(str::to_string);
    }
    // A glob: the directory it expands in is everything before the first
    // component that holds a glob character.
    let comps: Vec<&str> = flat.split('/').collect();
    let at = comps.iter().position(|c| c.contains('\u{1}')).unwrap_or(0);
    let prefix_raw = comps[..at].join("/");
    let prefix = if flat.starts_with('/') && prefix_raw.is_empty() {
        "/".to_string()
    } else {
        prefix_raw
    };
    if prefix.is_empty() {
        return Some("starts with a glob, which expands in the current directory".to_string());
    }
    danger_of(&prefix).map(|why| format!("is a glob directly under a path that {why}"))
}

/// A sed script with a `w`/`W`/`e` command or an `s` flag of `w` or `e`: it
/// writes files or runs commands beyond what the arguments name.
pub fn sed_script_acts(script: &str) -> bool {
    script.split(['\n', ';']).any(|cmd| {
        let cmd = cmd.trim_start();
        if cmd.starts_with(['w', 'W', 'e']) {
            return true;
        }
        match cmd
            .strip_prefix('s')
            .and_then(|r| r.chars().next().map(|d| (d, r)))
        {
            Some((d, r)) => {
                let flags = r[d.len_utf8()..].rsplit(d).next().unwrap_or("");
                r.matches(d).count() >= 3 && flags.chars().any(|c| c == 'w' || c == 'e')
            }
            None => false,
        }
    })
}

// ---- REQ-KS-063 built-ins: exact flags --------------------------------------

/// What one value of an exact flag is.
#[derive(Clone, Copy, PartialEq)]
pub enum XVal {
    /// Text that is not a path (a pattern, a count, a delimiter).
    Text,
    /// A path the built-in reads.
    Read,
}

/// One exact flag and the values it takes.
pub type XFlag = (&'static str, &'static [XVal]);

/// The flags of the REQ-KS-063 built-ins (edit, json, yaml, toml, field,
/// lines, base64, sha256sum). Each flag is matched as a whole word, never as
/// a cluster, and takes a fixed number of values. A word that starts with `-`
/// and reads as a number (`-1`) is an operand: `field -1`, `json set .n -1 f`.
/// The host parses with the same table (`kapsel-host/src/builtins/mod.rs`,
/// `xflags_for`); `xflags_tables_agree` in the host tests keeps them equal.
pub fn xflags_for(tool: &str) -> Option<&'static [XFlag]> {
    use XVal::{Read, Text};
    const T2: &[XVal] = &[Text, Text];
    const T1: &[XVal] = &[Text];
    const R1: &[XVal] = &[Read];
    const NO: &[XVal] = &[];
    Some(match tool {
        "edit" => &[
            ("-s", T2),
            ("-e", T2),
            ("-m", NO),
            ("-c", T1),
            ("--allow-none", NO),
            ("--lines", T1),
            ("--with", R1),
            ("--with-text", T1),
        ],
        "json" => &[
            ("-c", NO),
            ("-r", NO),
            ("-cr", NO),
            ("-rc", NO),
            ("--str", NO),
        ],
        "yaml" => &[("-j", NO), ("--str", NO)],
        "toml" => &[("--str", NO)],
        "field" => &[("-d", T1), ("-o", T1)],
        "lines" => &[],
        "base64" => &[("-d", NO), ("--decode", NO), ("-w", T1)],
        "sha256sum" => &[("-c", NO), ("--check", NO)],
        _ => return None,
    })
}

/// True for `-1`, `-2.5`: an operand, not a flag.
pub fn is_negative_number(s: &str) -> bool {
    s.strip_prefix('-').is_some_and(|r| {
        r.chars().next().is_some_and(|c| c.is_ascii_digit())
            && r.chars().all(|c| c.is_ascii_digit() || c == '.')
    })
}

/// Splits arguments by an exact-flag table. `values` carries `('r', word)`
/// for each [`XVal::Read`] value and `('_', word)` for text; `opts` the flags.
pub fn xsplit<'a>(table: &[XFlag], args: &'a [PWord], spans: &[Span]) -> Split<'a> {
    let span_of = |i: usize| spans.get(i).copied().unwrap_or_default();
    let mut sp = Split {
        positional: Vec::new(),
        opts: Vec::new(),
        values: Vec::new(),
        unknown: false,
        opaque: false,
    };
    let mut after_dd = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let at = i;
        i += 1;
        if after_dd {
            sp.positional.push((a, span_of(at)));
            continue;
        }
        let (head, lead_parts) = leading_literal(a);
        let nonlit = lead_parts < a.parts.len();
        if !head.starts_with('-') || (head == "-" && !nonlit) {
            sp.positional.push((a, span_of(at)));
            continue;
        }
        if nonlit {
            // `-$X`: may be any flag.
            sp.unknown = true;
            sp.opaque = true;
            continue;
        }
        if head == "--" {
            after_dd = true;
            continue;
        }
        if is_negative_number(&head) {
            sp.positional.push((a, span_of(at)));
            continue;
        }
        let Some((_, vals)) = table.iter().find(|(f, _)| *f == head) else {
            sp.unknown = true;
            continue;
        };
        sp.opts.push(head.clone());
        for v in *vals {
            let Some(w) = args.get(i) else {
                sp.unknown = true;
                break;
            };
            let key = if *v == XVal::Read { 'r' } else { '_' };
            sp.values.push((key, w.clone(), span_of(i)));
            i += 1;
        }
    }
    sp
}
