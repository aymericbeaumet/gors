//! Compiler-owned source coordinates.
//!
//! Physical byte offsets are the authoritative coordinates for compiler stage
//! products. They are fixed-width, path-independent, and safe for slicing only
//! after validation against the corresponding source revision. Adjusted
//! coordinates are a separate display projection used by Go `//line`
//! directives; they must never be used to address source bytes.
//!
//! [`SourceCoordinateMap`] maps physical offsets to an adjusted filename and
//! [`LogicalLineColumn`] using neutral line-directive segments captured by the
//! scanner's existing pass. Keeping the two domains distinct prevents virtual
//! filenames, one-based lines, and hidden Go columns from leaking into
//! incremental keys or byte-range arithmetic.

pub(crate) mod coordinate_map;
mod logical;
mod physical;

pub use coordinate_map::{
    AdjustedSourceCoordinate, LineDirectiveSegment, SourceCoordinateMap, SourceCoordinateMapError,
};
pub use logical::{LogicalColumn, LogicalLineColumn};
pub use physical::{
    FileRange, InvalidTextRange, PhysicalLineColumn, PhysicalLineColumnOverflow, TextRange,
    TextSize, TextSizeOverflow,
};

#[cfg(test)]
mod tests;
