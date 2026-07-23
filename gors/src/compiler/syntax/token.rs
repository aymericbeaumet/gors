//! Owned, trivia-free semantic tokens.

use std::sync::Arc;

use crate::parser::{TokenObservation, TokenSpelling};
use crate::token::Token;

/// One owned Go token retained by semantic source projection.
///
/// Fixed keywords and punctuation are represented by their token kind alone.
/// Identifiers and literals retain their exact spelling. Comments, physical
/// coordinates, EOF, and the explicit-versus-inserted semicolon distinction
/// are deliberately absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticToken {
    kind: Token,
    spelling: Option<Arc<str>>,
}

impl SemanticToken {
    pub(super) fn from_observation(observation: TokenObservation<'_>) -> Option<Self> {
        let kind = observation.token();
        if matches!(kind, Token::COMMENT | Token::EOF) {
            return None;
        }
        let spelling = if Self::retains_spelling(kind) {
            match observation.spelling() {
                TokenSpelling::Source(spelling) => Some(Arc::from(spelling)),
                TokenSpelling::InsertedSemicolon | TokenSpelling::EndOfFile => {
                    debug_assert!(false, "spelling-sensitive token lacks a source spelling");
                    Some(Arc::from(""))
                }
            }
        } else {
            None
        };
        Some(Self { kind, spelling })
    }

    const fn retains_spelling(kind: Token) -> bool {
        matches!(
            kind,
            Token::IDENT | Token::INT | Token::FLOAT | Token::IMAG | Token::CHAR | Token::STRING
        )
    }

    /// Lexical token kind.
    #[must_use]
    pub const fn kind(&self) -> Token {
        self.kind
    }

    /// Exact identifier or literal spelling, when semantically relevant.
    #[must_use]
    pub fn spelling(&self) -> Option<&str> {
        self.spelling.as_deref()
    }

    pub(super) fn kind_name(&self) -> &'static str {
        (&self.kind).into()
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.spelling.as_ref().map_or(0, |spelling| spelling.len())
    }
}
