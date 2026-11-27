//! Parser for kapsel's POSIX subset (REQ-KS-010, DD-KS-002).
//!
//! Owner: parser workstream. Builds a lossless rowan tree and lowers it to
//! `kapsel_ast`. Everything outside the subset becomes a `ParseError` that
//! names the construct.

pub use kapsel_ast as ast;

/// Parses a whole script or `-c` string.
///
/// Returns every error found, not only the first, so a refusal lists them all.
pub fn parse(source: &str) -> Result<ast::Script, Vec<ast::ParseError>> {
    let _ = source;
    Err(vec![ast::ParseError {
        construct: "unimplemented".into(),
        message: "parser not implemented yet".into(),
        span: ast::Span::new(0, source.len() as u32),
    }])
}
