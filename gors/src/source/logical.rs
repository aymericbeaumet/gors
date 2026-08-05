use std::num::NonZeroU32;

/// A Go-adjusted display column.
///
/// Go positions use column zero to mean that the column is intentionally
/// hidden, notably after a two-field `//line file:line` directive. Modeling
/// that state explicitly prevents it from being mistaken for either physical
/// byte column zero or the first visible column.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LogicalColumn {
    Hidden,
    Known(NonZeroU32),
}

impl LogicalColumn {
    #[must_use]
    pub const fn from_go_column(column: u32) -> Self {
        match NonZeroU32::new(column) {
            Some(column) => Self::Known(column),
            None => Self::Hidden,
        }
    }

    /// The Go position encoding, where zero means no displayed column.
    #[must_use]
    pub const fn to_go_column(self) -> u32 {
        match self {
            Self::Hidden => 0,
            Self::Known(column) => column.get(),
        }
    }

    #[must_use]
    pub const fn known(self) -> Option<NonZeroU32> {
        match self {
            Self::Hidden => None,
            Self::Known(column) => Some(column),
        }
    }
}

/// A one-based line and adjusted Go display column.
///
/// This value is a coordinate-map result, not a source byte address. The
/// source coordinate map pairs it with an adjusted filename while preserving
/// a physical byte range as authoritative provenance.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogicalLineColumn {
    line: NonZeroU32,
    column: LogicalColumn,
}

impl LogicalLineColumn {
    #[must_use]
    pub const fn new(line: NonZeroU32, column: LogicalColumn) -> Self {
        Self { line, column }
    }

    #[must_use]
    pub const fn line(self) -> NonZeroU32 {
        self.line
    }

    #[must_use]
    pub const fn column(self) -> LogicalColumn {
        self.column
    }
}
