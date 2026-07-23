//! Immutable semantic token streams and canonical fingerprints.

use std::sync::Arc;

use crate::compiler::fingerprint::{Fingerprint, fingerprint_parts};
use crate::parser::TokenObservation;

use super::SemanticToken;

/// An owned, ordered, trivia-insensitive Go token stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticTokenStream {
    tokens: Arc<[SemanticToken]>,
    fingerprint: Fingerprint,
}

impl SemanticTokenStream {
    pub(super) fn from_observations(observations: &[TokenObservation<'_>]) -> Self {
        let tokens = observations
            .iter()
            .filter_map(|observation| SemanticToken::from_observation(*observation))
            .collect::<Vec<_>>();
        Self::new(tokens)
    }

    fn new(tokens: Vec<SemanticToken>) -> Self {
        let canonical = canonical_encoding(&tokens);
        Self {
            tokens: tokens.into(),
            fingerprint: fingerprint_parts(b"semantic-token-stream-v1", &[canonical.as_slice()]),
        }
    }

    /// Canonical semantic tokens in source order.
    #[must_use]
    pub fn tokens(&self) -> &[SemanticToken] {
        &self.tokens
    }

    /// Domain-separated fingerprint excluding trivia and physical layout.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.tokens.iter().fold(32_usize, |total, token| {
            total
                .saturating_add(std::mem::size_of::<SemanticToken>())
                .saturating_add(token.retained_bytes())
        })
    }
}

fn canonical_encoding(tokens: &[SemanticToken]) -> Vec<u8> {
    let mut output = Vec::new();
    write_len(&mut output, tokens.len());
    for token in tokens {
        write_blob(&mut output, token.kind_name().as_bytes());
        match token.spelling() {
            Some(spelling) => {
                output.push(1);
                write_blob(&mut output, spelling.as_bytes());
            }
            None => output.push(0),
        }
    }
    output
}

fn write_blob(output: &mut Vec<u8>, bytes: &[u8]) {
    write_len(output, bytes.len());
    output.extend_from_slice(bytes);
}

fn write_len(output: &mut Vec<u8>, len: usize) {
    output.extend_from_slice(&(len as u64).to_be_bytes());
}
