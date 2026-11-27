//! Parser for kapsel's POSIX subset (REQ-KS-010, DD-KS-002).
//!
//! Two stages, as in spar and rivet:
//!
//! 1. [`parse_green`] scans and parses the input into a lossless rowan green
//!    tree plus a list of [`ParseError`](ast::ParseError)s. Every byte of the
//!    input is a token of the tree, so a refusal can quote exactly what it
//!    refuses (court stenography). The grammar is closed: anything outside
//!    the subset is an `ERROR` node and an error naming the construct, never a
//!    guess (air-traffic phraseology: "say again").
//! 2. [`Parse::lower`] turns an error-free tree into the typed
//!    [`kapsel_ast::Script`].
//!
//! [`parse`] does both. [`is_incomplete`] tells the interactive prompt to read
//! a continuation line.

pub use kapsel_ast as ast;

mod kinds;
mod lower;
mod parser;

pub use kinds::{KapselLang, SyntaxKind};

pub type SyntaxNode = rowan::SyntaxNode<KapselLang>;
pub type SyntaxToken = rowan::SyntaxToken<KapselLang>;
pub type SyntaxElement = rowan::SyntaxElement<KapselLang>;

/// Result of [`parse_green`]: the lossless tree and every error found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    green: rowan::GreenNode,
    errors: Vec<ast::ParseError>,
    incomplete: bool,
}

impl Parse {
    /// The root `SCRIPT` node. Its text is the input, byte for byte.
    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn green(&self) -> &rowan::GreenNode {
        &self.green
    }

    /// Errors sorted by span start; outer constructs before inner ones.
    pub fn errors(&self) -> &[ast::ParseError] {
        &self.errors
    }

    /// True when there are no errors.
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// True when the input ended inside a construct: an open quote, heredoc,
    /// `$(`, loop or refused compound, or after `|`, `&&`, `||` or a
    /// trailing backslash.
    pub fn is_incomplete(&self) -> bool {
        self.incomplete
    }

    /// Lowers the tree to the typed AST, or returns the parse errors.
    pub fn lower(&self) -> Result<ast::Script, Vec<ast::ParseError>> {
        if !self.errors.is_empty() {
            return Err(self.errors.clone());
        }
        lower::lower(&self.syntax_node())
    }
}

/// Parses a whole script or `-c` string into a lossless tree.
pub fn parse_green(text: &str) -> Parse {
    let out = parser::parse(text);
    Parse {
        green: out.green,
        errors: out.errors,
        incomplete: out.incomplete,
    }
}

/// Parses a whole script or `-c` string.
///
/// Returns every error found, not only the first, so a refusal lists them all.
pub fn parse(source: &str) -> Result<ast::Script, Vec<ast::ParseError>> {
    parse_green(source).lower()
}

/// True when `source` ends inside a construct and the interactive prompt
/// should read another line before parsing for real.
pub fn is_incomplete(source: &str) -> bool {
    parse_green(source).is_incomplete()
}

/// Span of a tree node without leading and trailing trivia; the same rule
/// the lowering uses for AST spans.
pub fn node_span(node: &SyntaxNode) -> ast::Span {
    lower::span_of(node)
}
