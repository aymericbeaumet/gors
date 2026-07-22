use std::fmt;
use std::num::NonZeroU32;

/// A zero-based byte offset within one source file.
///
/// The fixed-width representation bounds persistent stage products and keeps
/// their encoding independent of the compiler host's pointer width.
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct TextSize(u32);

impl TextSize {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u32::MAX);

    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    #[allow(clippy::cast_lossless)]
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for TextSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<u32> for TextSize {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<TextSize> for u32 {
    fn from(value: TextSize) -> Self {
        value.get()
    }
}

impl TryFrom<usize> for TextSize {
    type Error = TextSizeOverflow;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value)
            .map(Self)
            .map_err(|_| TextSizeOverflow { value })
    }
}

/// A host-sized byte offset that cannot be encoded in a [`TextSize`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextSizeOverflow {
    value: usize,
}

impl TextSizeOverflow {
    #[must_use]
    pub const fn value(self) -> usize {
        self.value
    }
}

impl fmt::Display for TextSizeOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "source byte offset {} exceeds the u32 coordinate limit {}",
            self.value,
            u32::MAX
        )
    }
}

impl std::error::Error for TextSizeOverflow {}

/// A zero-based, half-open byte range within one source file.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextRange {
    start: TextSize,
    end: TextSize,
}

impl TextRange {
    /// Construct `[start, end)`, rejecting a reversed range.
    pub const fn new(start: TextSize, end: TextSize) -> Result<Self, InvalidTextRange> {
        if start.0 <= end.0 {
            Ok(Self { start, end })
        } else {
            Err(InvalidTextRange { start, end })
        }
    }

    /// Construct the complete range of a source with the given byte length.
    #[must_use]
    pub const fn up_to(end: TextSize) -> Self {
        Self {
            start: TextSize::ZERO,
            end,
        }
    }

    /// Construct an empty range anchored at one exact byte boundary.
    #[must_use]
    pub const fn empty(at: TextSize) -> Self {
        Self { start: at, end: at }
    }

    #[must_use]
    pub const fn start(self) -> TextSize {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> TextSize {
        self.end
    }

    #[must_use]
    pub const fn len(self) -> TextSize {
        TextSize(self.end.0 - self.start.0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }

    /// Whether an offset names a byte in this half-open range.
    #[must_use]
    pub const fn contains(self, offset: TextSize) -> bool {
        self.start.0 <= offset.0 && offset.0 < self.end.0
    }
}

impl fmt::Debug for TextRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}..{:?}", self.start, self.end)
    }
}

/// A reversed half-open byte range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTextRange {
    start: TextSize,
    end: TextSize,
}

impl InvalidTextRange {
    #[must_use]
    pub const fn start(self) -> TextSize {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> TextSize {
        self.end
    }
}

impl fmt::Display for InvalidTextRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "source byte range start {} exceeds end {}",
            self.start.0, self.end.0
        )
    }
}

impl std::error::Error for InvalidTextRange {}

/// A one-based physical line and one-based UTF-8 byte column.
///
/// This matches Go token positions and the compiler's current diagnostic
/// convention. It is not an LSP or Monaco coordinate: those consumers must
/// perform an explicit zero-based and, where required, UTF-16 conversion.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhysicalLineColumn {
    line: NonZeroU32,
    byte_column: NonZeroU32,
}

impl PhysicalLineColumn {
    #[must_use]
    pub const fn new(line: NonZeroU32, byte_column: NonZeroU32) -> Self {
        Self { line, byte_column }
    }

    pub(crate) fn try_from_usize(
        line: usize,
        byte_column: usize,
    ) -> Result<Self, PhysicalLineColumnOverflow> {
        let line = u32::try_from(line)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(PhysicalLineColumnOverflow::Line { value: line })?;
        let byte_column = u32::try_from(byte_column)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(PhysicalLineColumnOverflow::ByteColumn { value: byte_column })?;
        Ok(Self::new(line, byte_column))
    }

    #[must_use]
    pub const fn line(self) -> NonZeroU32 {
        self.line
    }

    #[must_use]
    pub const fn byte_column(self) -> NonZeroU32 {
        self.byte_column
    }
}

/// A one-based physical display coordinate that cannot fit in `u32`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalLineColumnOverflow {
    Line { value: usize },
    ByteColumn { value: usize },
}

impl fmt::Display for PhysicalLineColumnOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line { value } => write!(
                formatter,
                "physical line {value} is zero or exceeds the u32 coordinate limit"
            ),
            Self::ByteColumn { value } => write!(
                formatter,
                "physical byte column {value} is zero or exceeds the u32 coordinate limit"
            ),
        }
    }
}

impl std::error::Error for PhysicalLineColumnOverflow {}
