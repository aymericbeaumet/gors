//! Token observations published from the parser's existing scanner pass.

use crate::scanner;
use crate::token::Token;

/// How one consumed token was spelled in the source stream.
///
/// Inserted semicolons and EOF have no physical source spelling. Keeping
/// those cases explicit lets semantic projection normalize explicit and
/// inserted semicolons without losing evidence that the scanner observed
/// both transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenSpelling<'source> {
    /// Exact source spelling, or the fixed spelling of punctuation.
    Source(&'source str),
    /// A semicolon inserted by Go's lexical rules.
    InsertedSemicolon,
    /// The terminal scanner observation.
    EndOfFile,
}

/// One non-comment token consumed by the parser.
///
/// This is borrowed, ephemeral parser output. Compiler queries must project it
/// into owned semantic syntax before the source snapshot can be evicted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenObservation<'source> {
    token: Token,
    spelling: TokenSpelling<'source>,
    byte_offset: usize,
    byte_len: usize,
}

impl<'source> TokenObservation<'source> {
    pub(crate) fn from_step(step: scanner::Step<'source>) -> Self {
        let (position, token, literal) = step;
        debug_assert_ne!(token, Token::COMMENT);
        let spelling = match token {
            Token::SEMICOLON if literal == "\n" => TokenSpelling::InsertedSemicolon,
            Token::EOF => TokenSpelling::EndOfFile,
            _ => {
                let source_spelling = if literal.is_empty() {
                    let canonical: &'static str = (&token).into();
                    canonical
                } else {
                    literal
                };
                TokenSpelling::Source(source_spelling)
            }
        };
        let byte_len = match spelling {
            TokenSpelling::Source(spelling) => spelling.len(),
            TokenSpelling::InsertedSemicolon | TokenSpelling::EndOfFile => 0,
        };
        Self {
            token,
            spelling,
            byte_offset: position.offset,
            byte_len,
        }
    }

    /// Lexical token kind.
    #[must_use]
    pub const fn token(self) -> Token {
        self.token
    }

    /// Exact spelling classification observed during the parser's scan.
    #[must_use]
    pub const fn spelling(self) -> TokenSpelling<'source> {
        self.spelling
    }

    /// Physical zero-based byte offset in the immutable source snapshot.
    #[must_use]
    pub const fn byte_offset(self) -> usize {
        self.byte_offset
    }

    /// Physical byte length, zero for inserted semicolons and EOF.
    #[must_use]
    pub const fn byte_len(self) -> usize {
        self.byte_len
    }

    /// Physical byte boundary immediately after this observation.
    #[must_use]
    pub const fn byte_end(self) -> usize {
        self.byte_offset.saturating_add(self.byte_len)
    }
}
