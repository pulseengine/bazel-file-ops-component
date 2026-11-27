//! Token and node kinds of kapsel's lossless syntax tree (DD-KS-002).
//!
//! Every byte of the input belongs to exactly one token, so the tree's text is
//! the input (court stenography: the record keeps every character). Refused
//! and malformed regions are kept too, under [`SyntaxKind::ERROR`] nodes.

macro_rules! kinds {
    ($($(#[$doc:meta])* $k:ident),* $(,)?) => {
        /// Kind of a rowan token or node.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        pub enum SyntaxKind {
            $($(#[$doc])* $k),*
        }

        const ALL: &[SyntaxKind] = &[$(SyntaxKind::$k),*];
    };
}

kinds! {
    // ---- trivia tokens
    /// Spaces and tabs.
    WHITESPACE,
    /// `\n`; heredoc bodies follow the newline that ends their command line.
    NEWLINE,
    /// `# ...` up to, not including, the newline.
    COMMENT,
    /// Backslash-newline continuation.
    LINE_CONT,

    // ---- operator tokens
    /// `;`
    SEMI,
    /// `;;` (only inside a refused `case`).
    DSEMI,
    /// `&` (background, refused).
    AMP,
    /// `&&`
    AND_AND,
    /// `||`
    OR_OR,
    /// `|`
    PIPE,
    /// `|&` (refused).
    PIPE_AMP,
    /// `!` at the start of a pipeline.
    BANG,
    /// `(`
    LPAREN,
    /// `)`
    RPAREN,
    /// Single-digit fd before a redirect operator.
    IO_NUMBER,
    /// `<`
    LT,
    /// `>`
    GT,
    /// `>>`
    DGT,
    /// `>&`
    GT_AMP,
    /// `<&`
    LT_AMP,
    /// `<<`
    DLT,
    /// `<<-`
    DLT_DASH,
    /// `<<<` (refused).
    TLT,
    /// `<>` (refused).
    LT_GT,
    /// `>|` (refused).
    CLOBBER,
    /// `&>` and `&>>` (refused).
    AMP_GT,

    // ---- word-level tokens
    /// A reserved word in a position where it is recognised.
    KEYWORD,
    /// Variable name of an assignment or a `for` loop.
    NAME,
    /// `=` of an assignment.
    EQ,
    /// Literal text.
    LIT,
    /// Backslash and the escaped character.
    ESCAPE,
    /// A whole single-quoted string, quotes included.
    SQUOTE,
    /// `"` opening or closing a double-quoted string.
    DQUOTE,
    /// `$name`, `$?`, `$1`, ...
    PARAM_TOKEN,
    /// `${`
    DOLLAR_LBRACE,
    /// Name inside `${...}`.
    PARAM_NAME,
    /// `}` closing `${`.
    PARAM_RBRACE,
    /// `$(`
    DOLLAR_LPAREN,
    /// Unquoted `*`.
    GLOB_STAR,
    /// Unquoted `?`.
    GLOB_QUESTION,
    /// Unquoted `[...]`, brackets included.
    GLOB_CLASS,
    /// Leading tabs stripped from a `<<-` heredoc line.
    HEREDOC_TABS,
    /// The heredoc's closing delimiter line, newline included.
    HEREDOC_END,
    /// Text of a refused or malformed region.
    ERROR_TEXT,

    // ---- nodes
    /// A script, or the inside of `$(...)`.
    SCRIPT,
    /// `p1 && p2 || p3`
    AND_OR,
    /// `[!] c1 | c2`
    PIPELINE,
    /// Assignments, words and redirects.
    SIMPLE_CMD,
    /// `NAME=value`
    ASSIGNMENT,
    /// One shell word.
    WORD,
    /// `"..."`
    DQUOTED,
    /// `$name` or `${name}`.
    PARAM,
    /// `$(...)`
    CMD_SUBST,
    /// `[fd]op target`
    REDIRECT,
    /// Heredoc body lines and the delimiter line.
    HEREDOC_BODY,
    /// `for NAME in words; do list; done`
    FOR_LOOP,
    /// `while`/`until list; do list; done`
    WHILE_LOOP,
    /// A loop condition or body.
    LIST,
    /// A refused construct or malformed input; see the parse errors.
    ERROR,
}

impl SyntaxKind {
    /// Whitespace, newlines, comments and line continuations.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE | Self::NEWLINE | Self::COMMENT | Self::LINE_CONT
        )
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}

/// The rowan language tag for kapsel's tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KapselLang {}

impl rowan::Language for KapselLang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        ALL.get(raw.0 as usize)
            .copied()
            .unwrap_or(SyntaxKind::ERROR)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_roundtrip() {
        for (i, k) in ALL.iter().enumerate() {
            assert_eq!(*k as usize, i);
            assert_eq!(
                <KapselLang as rowan::Language>::kind_from_raw((*k).into()),
                *k
            );
        }
    }
}
