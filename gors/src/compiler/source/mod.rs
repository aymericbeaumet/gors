//! Compiler-owned source coordinates.
//!
//! Physical byte offsets are the authoritative coordinates for compiler stage
//! products. They are fixed-width, path-independent, and safe for slicing only
//! after validation against the corresponding source revision. Adjusted
//! coordinates are a separate display projection used by Go `//line`
//! directives; they must never be used to address source bytes.
//!
//! The eventual coordinate-map boundary will map physical [`FileRange`] values
//! to an adjusted filename and [`LogicalLineColumn`]. Keeping the two domains
//! distinct now prevents virtual filenames, one-based lines, and hidden Go
//! columns from leaking into incremental keys or byte-range arithmetic.

mod logical;
mod physical;

pub use logical::{LogicalColumn, LogicalLineColumn};
pub use physical::{
    FileRange, InvalidTextRange, PhysicalLineColumn, PhysicalLineColumnOverflow, TextRange,
    TextSize, TextSizeOverflow,
};

#[cfg(test)]
mod tests;
