//! Successful parser products.

use crate::ast;
use crate::source::SourceCoordinateMap;

/// One borrowed Go syntax tree paired with its scanner-built coordinate map.
///
/// The AST is intentionally ephemeral. The coordinate map owns its data and
/// can be retained by compiler queries after semantic projection has finished.
#[derive(Debug)]
pub struct ParsedFile<'source> {
    ast: ast::File<'source>,
    coordinate_map: SourceCoordinateMap,
}

impl<'source> ParsedFile<'source> {
    pub(crate) const fn new(ast: ast::File<'source>, coordinate_map: SourceCoordinateMap) -> Self {
        Self {
            ast,
            coordinate_map,
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

    /// Split the ephemeral syntax tree from its independently owned map.
    #[must_use]
    pub fn into_parts(self) -> (ast::File<'source>, SourceCoordinateMap) {
        (self.ast, self.coordinate_map)
    }
}
