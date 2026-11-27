//! Lowering from the lossless rowan tree to `kapsel_ast`.
//!
//! Only called on trees without parse errors, so every node has the shape the
//! parser builds. A shape it does not expect becomes an internal `syntax`
//! error instead of a panic.

use std::collections::{HashMap, VecDeque};

use crate::kinds::SyntaxKind::*;
use crate::{SyntaxElement, SyntaxNode};
use kapsel_ast::*;
use rowan::{NodeOrToken, WalkEvent};

pub(crate) fn lower(root: &SyntaxNode) -> Result<Script, Vec<ParseError>> {
    let mut l = Lower {
        bodies: HashMap::new(),
        errors: Vec::new(),
    };
    l.pair_heredocs(root);
    let script = l.script(root);
    if l.errors.is_empty() {
        Ok(script)
    } else {
        Err(l.errors)
    }
}

struct Lower {
    /// Redirect start offset -> its heredoc body node.
    bodies: HashMap<u32, SyntaxNode>,
    errors: Vec<ParseError>,
}

/// Span of a node without leading and trailing trivia.
pub(crate) fn span_of(node: &SyntaxNode) -> Span {
    let mut first = None;
    let mut last = None;
    for t in node
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
    {
        if !t.kind().is_trivia() {
            first.get_or_insert(t.text_range().start());
            last = Some(t.text_range().end());
        }
    }
    match (first, last) {
        (Some(s), Some(e)) => Span::new(s.into(), e.into()),
        _ => {
            let r = node.text_range();
            Span::new(r.start().into(), r.end().into())
        }
    }
}

fn is_heredoc_redirect(node: &SyntaxNode) -> bool {
    node.kind() == REDIRECT
        && node
            .children_with_tokens()
            .any(|e| matches!(e.kind(), DLT | DLT_DASH))
}

impl Lower {
    /// Bodies are read first-in first-out at each newline, and preorder is
    /// text order, so the n-th body read belongs to the n-th open heredoc.
    fn pair_heredocs(&mut self, root: &SyntaxNode) {
        let mut open = VecDeque::new();
        for ev in root.preorder() {
            if let WalkEvent::Enter(n) = ev {
                if is_heredoc_redirect(&n) {
                    open.push_back(u32::from(n.text_range().start()));
                } else if n.kind() == HEREDOC_BODY
                    && let Some(r) = open.pop_front()
                {
                    self.bodies.insert(r, n);
                }
            }
        }
    }

    fn bug(&mut self, node: &SyntaxNode, what: &str) {
        self.errors.push(ParseError {
            construct: "syntax".into(),
            message: format!("syntax: internal parser error: {what}"),
            span: span_of(node),
            expected: Vec::new(),
            help: Some("please report this input to kapsel".into()),
        });
    }

    fn script(&mut self, node: &SyntaxNode) -> Script {
        Script {
            items: self.and_ors(node),
            span: span_of(node),
        }
    }

    fn and_ors(&mut self, node: &SyntaxNode) -> Vec<AndOrList> {
        node.children()
            .filter(|c| c.kind() == AND_OR)
            .map(|c| self.and_or(&c))
            .collect()
    }

    fn and_or(&mut self, node: &SyntaxNode) -> AndOrList {
        let mut first = None;
        let mut rest = Vec::new();
        let mut op = None;
        for e in node.children_with_tokens() {
            match e {
                NodeOrToken::Token(t) if t.kind() == AND_AND => op = Some(AndOrOp::And),
                NodeOrToken::Token(t) if t.kind() == OR_OR => op = Some(AndOrOp::Or),
                NodeOrToken::Node(n) if n.kind() == PIPELINE => {
                    let p = self.pipeline(&n);
                    match (first.is_none(), op.take()) {
                        (true, _) => first = Some(p),
                        (false, Some(o)) => rest.push((o, p)),
                        (false, None) => self.bug(&n, "pipeline without operator"),
                    }
                }
                _ => {}
            }
        }
        let first = first.unwrap_or_else(|| {
            self.bug(node, "empty and-or list");
            Pipeline {
                negated: false,
                commands: Vec::new(),
                span: span_of(node),
            }
        });
        AndOrList {
            first,
            rest,
            span: span_of(node),
        }
    }

    fn pipeline(&mut self, node: &SyntaxNode) -> Pipeline {
        let negated = node.children_with_tokens().any(|e| e.kind() == BANG);
        let mut commands = Vec::new();
        for c in node.children() {
            match c.kind() {
                SIMPLE_CMD => commands.push(Command::Simple(self.simple(&c))),
                FOR_LOOP => commands.push(Command::For(self.for_loop(&c))),
                WHILE_LOOP => commands.push(Command::While(self.while_loop(&c))),
                _ => self.bug(&c, "unexpected node in pipeline"),
            }
        }
        if commands.is_empty() {
            self.bug(node, "empty pipeline");
        }
        Pipeline {
            negated,
            commands,
            span: span_of(node),
        }
    }

    fn simple(&mut self, node: &SyntaxNode) -> SimpleCommand {
        let mut cmd = SimpleCommand {
            assignments: Vec::new(),
            words: Vec::new(),
            redirects: Vec::new(),
            span: span_of(node),
        };
        for c in node.children() {
            match c.kind() {
                ASSIGNMENT => {
                    let name = c
                        .children_with_tokens()
                        .find(|e| e.kind() == NAME)
                        .and_then(NodeOrToken::into_token)
                        .map(|t| t.text().to_string())
                        .unwrap_or_default();
                    let value = match c.children().find(|n| n.kind() == WORD) {
                        Some(w) => self.word(&w),
                        None => {
                            self.bug(&c, "assignment without value");
                            Word {
                                parts: Vec::new(),
                                span: span_of(&c),
                            }
                        }
                    };
                    cmd.assignments.push(Assignment {
                        name,
                        value,
                        span: span_of(&c),
                    });
                }
                WORD => cmd.words.push(self.word(&c)),
                REDIRECT => {
                    if let Some(r) = self.redirect(&c) {
                        cmd.redirects.push(r);
                    }
                }
                _ => self.bug(&c, "unexpected node in simple command"),
            }
        }
        cmd
    }

    fn for_loop(&mut self, node: &SyntaxNode) -> ForLoop {
        let var = node
            .children_with_tokens()
            .find(|e| e.kind() == NAME)
            .and_then(NodeOrToken::into_token)
            .map(|t| t.text().to_string())
            .unwrap_or_default();
        let items = node
            .children()
            .filter(|c| c.kind() == WORD)
            .map(|c| self.word(&c))
            .collect();
        let body = match node.children().find(|c| c.kind() == LIST) {
            Some(l) => self.and_ors(&l),
            None => Vec::new(),
        };
        ForLoop {
            var,
            items,
            body,
            span: span_of(node),
        }
    }

    fn while_loop(&mut self, node: &SyntaxNode) -> WhileLoop {
        let until = node
            .children_with_tokens()
            .find(|e| e.kind() == KEYWORD)
            .and_then(NodeOrToken::into_token)
            .is_some_and(|t| t.text() == "until");
        let lists: Vec<_> = node.children().filter(|c| c.kind() == LIST).collect();
        if lists.len() != 2 {
            self.bug(node, "loop without condition and body");
        }
        let condition = lists.first().map(|l| self.and_ors(l)).unwrap_or_default();
        let body = lists.get(1).map(|l| self.and_ors(l)).unwrap_or_default();
        WhileLoop {
            until,
            condition,
            body,
            span: span_of(node),
        }
    }

    fn redirect(&mut self, node: &SyntaxNode) -> Option<Redirect> {
        let mut fd = None;
        let mut op = None;
        let mut op_kind = None;
        for e in node.children_with_tokens() {
            let Some(t) = e.into_token() else { continue };
            match t.kind() {
                IO_NUMBER => fd = t.text().parse().ok(),
                k @ (LT | GT | DGT | GT_AMP | LT_AMP | DLT | DLT_DASH) => {
                    op_kind = Some(k);
                    op = Some(match k {
                        LT => RedirectOp::Read,
                        GT => RedirectOp::Write,
                        DGT => RedirectOp::Append,
                        GT_AMP => RedirectOp::DupOut,
                        LT_AMP => RedirectOp::DupIn,
                        _ => RedirectOp::Heredoc,
                    });
                }
                _ => {}
            }
        }
        let (Some(op), Some(op_kind)) = (op, op_kind) else {
            self.bug(node, "redirect without operator");
            return None;
        };
        let Some(word) = node.children().find(|c| c.kind() == WORD) else {
            self.bug(node, "redirect without target");
            return None;
        };
        let target = match op {
            RedirectOp::Heredoc => {
                let quoted = word
                    .descendants_with_tokens()
                    .any(|e| matches!(e.kind(), SQUOTE | DQUOTE | ESCAPE));
                let start = u32::from(node.text_range().start());
                let Some(body) = self.bodies.get(&start).cloned() else {
                    self.bug(node, "heredoc without body");
                    return None;
                };
                RedirectTarget::Heredoc {
                    body: self.heredoc_body(&body, quoted),
                    quoted,
                    strip_tabs: op_kind == DLT_DASH,
                }
            }
            RedirectOp::DupOut | RedirectOp::DupIn => match word.text().to_string().parse() {
                Ok(fd) => RedirectTarget::Fd { fd },
                Err(_) => {
                    self.bug(node, "dup target is not a number");
                    return None;
                }
            },
            _ => RedirectTarget::Word {
                word: self.word(&word),
            },
        };
        Some(Redirect {
            fd,
            op,
            target,
            span: span_of(node),
        })
    }

    fn heredoc_body(&mut self, body: &SyntaxNode, quoted: bool) -> Word {
        let start = u32::from(body.text_range().start());
        let end = body
            .children_with_tokens()
            .find(|e| e.kind() == HEREDOC_END)
            .map_or(body.text_range().end(), |e| e.text_range().start());
        let span = Span::new(start, end.into());
        let parts = if quoted {
            let text: String = body
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .filter(|t| t.kind() == LIT)
                .map(|t| t.text().to_string())
                .collect();
            vec![WordPart::Literal { text }]
        } else {
            self.parts(body)
        };
        Word { parts, span }
    }

    fn word(&mut self, node: &SyntaxNode) -> Word {
        Word {
            parts: self.parts(node),
            span: span_of(node),
        }
    }

    fn parts(&mut self, node: &SyntaxNode) -> Vec<WordPart> {
        let mut parts = Vec::new();
        for e in node.children_with_tokens() {
            self.part(e, &mut parts);
        }
        parts
    }

    fn part(&mut self, e: SyntaxElement, parts: &mut Vec<WordPart>) {
        match e {
            NodeOrToken::Token(t) => match t.kind() {
                LIT => push_lit(parts, t.text()),
                ESCAPE => push_lit(parts, t.text().get(1..).unwrap_or("")),
                SQUOTE => {
                    let s = t.text();
                    let inner = s.get(1..s.len().saturating_sub(1).max(1)).unwrap_or("");
                    parts.push(WordPart::SingleQuoted {
                        text: inner.to_string(),
                    });
                }
                GLOB_STAR => parts.push(WordPart::Glob {
                    glob: GlobPart::Star,
                }),
                GLOB_QUESTION => parts.push(WordPart::Glob {
                    glob: GlobPart::Question,
                }),
                GLOB_CLASS => {
                    let s = t.text();
                    let set = s.get(1..s.len().saturating_sub(1).max(1)).unwrap_or("");
                    parts.push(WordPart::Glob {
                        glob: GlobPart::Class {
                            set: set.to_string(),
                        },
                    });
                }
                LINE_CONT | DQUOTE | HEREDOC_TABS | HEREDOC_END => {}
                k => {
                    if let Some(p) = t.parent() {
                        self.bug(&p, &format!("unexpected token {k:?} in word"));
                    }
                }
            },
            NodeOrToken::Node(n) => match n.kind() {
                DQUOTED => {
                    let inner = self.parts(&n);
                    parts.push(WordPart::DoubleQuoted { parts: inner });
                }
                PARAM => {
                    let name = n
                        .children_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                        .find_map(|t| match t.kind() {
                            PARAM_TOKEN => Some(t.text().get(1..).unwrap_or("").to_string()),
                            PARAM_NAME => Some(t.text().to_string()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    parts.push(WordPart::Param { name });
                }
                CMD_SUBST => {
                    let script = match n.children().find(|c| c.kind() == SCRIPT) {
                        Some(s) => self.script(&s),
                        None => {
                            self.bug(&n, "command substitution without script");
                            Script {
                                items: Vec::new(),
                                span: span_of(&n),
                            }
                        }
                    };
                    parts.push(WordPart::CommandSubst {
                        script: Box::new(script),
                    });
                }
                k => self.bug(&n, &format!("unexpected node {k:?} in word")),
            },
        }
    }
}

fn push_lit(parts: &mut Vec<WordPart>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(WordPart::Literal { text }) = parts.last_mut() {
        text.push_str(s);
    } else {
        parts.push(WordPart::Literal {
            text: s.to_string(),
        });
    }
}
