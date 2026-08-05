//! Successful parser products.

use crate::ast;
use crate::source::SourceCoordinateMap;

use super::TokenObservation;

/// One borrowed Go syntax tree paired with its scanner-built coordinate map.
///
/// The AST and token observations are intentionally ephemeral. The coordinate
/// map owns its data and can be retained by compiler queries after semantic
/// projection has finished.
#[derive(Debug)]
pub struct ParsedFile<'source> {
    ast: ast::File<'source>,
    coordinate_map: SourceCoordinateMap,
    token_observations: Box<[TokenObservation<'source>]>,
}

impl<'source> ParsedFile<'source> {
    pub(crate) const fn new(
        ast: ast::File<'source>,
        coordinate_map: SourceCoordinateMap,
        token_observations: Box<[TokenObservation<'source>]>,
    ) -> Self {
        Self {
            ast,
            coordinate_map,
            token_observations,
        }
    }

    /// Borrow the parser-owned Go AST.
    #[must_use]
    pub const fn ast(&self) -> &ast::File<'source> {
        &self.ast
    }

    /// Borrow the complete scanner-built physical-to-adjusted map.
    #[must_use]
    pub const fn source_coordinate_map(&self) -> &SourceCoordinateMap {
        &self.coordinate_map
    }

    /// Non-comment tokens consumed by the parser's one scanner pass.
    #[must_use]
    pub const fn token_observations(&self) -> &[TokenObservation<'source>] {
        &self.token_observations
    }

    /// Split the ephemeral syntax tree and tokens from the independently owned map.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        ast::File<'source>,
        SourceCoordinateMap,
        Box<[TokenObservation<'source>]>,
    ) {
        (self.ast, self.coordinate_map, self.token_observations)
    }
}
