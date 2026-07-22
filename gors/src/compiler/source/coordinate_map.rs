//! Go line-directive projections over authoritative physical byte offsets.

use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;

use super::{
    LogicalColumn, LogicalLineColumn, PhysicalLineColumn, PhysicalLineColumnOverflow, TextSize,
    TextSizeOverflow,
};

/// One line-directive transition anchored in physical source coordinates.
///
/// The adjusted filename is already resolved lexically against the initial
/// source name. It is never normalized through the host platform.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LineDirectiveSegment {
    physical_start: TextSize,
    physical_position: PhysicalLineColumn,
    adjusted_filename: Arc<str>,
    logical_start: LogicalLineColumn,
}

impl LineDirectiveSegment {
    #[must_use]
    pub const fn physical_start(&self) -> TextSize {
        self.physical_start
    }

    #[must_use]
    pub const fn physical_position(&self) -> PhysicalLineColumn {
        self.physical_position
    }

    #[must_use]
    pub fn adjusted_filename(&self) -> &str {
        &self.adjusted_filename
    }

    #[must_use]
    pub const fn logical_start(&self) -> LogicalLineColumn {
        self.logical_start
    }
}

/// Adjusted display coordinate produced from one physical byte offset.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdjustedSourceCoordinate<'a> {
    filename: &'a str,
    position: LogicalLineColumn,
}

impl<'a> AdjustedSourceCoordinate<'a> {
    #[must_use]
    pub const fn new(filename: &'a str, position: LogicalLineColumn) -> Self {
        Self { filename, position }
    }

    #[must_use]
    pub const fn filename(self) -> &'a str {
        self.filename
    }

    #[must_use]
    pub const fn position(self) -> LogicalLineColumn {
        self.position
    }
}

/// Immutable physical-to-adjusted coordinate map for one scanned source.
///
/// The scanner constructs line starts and directive segments during its normal
/// single pass. Offsets remain authoritative even when a directive resets the
/// displayed line number or hides displayed columns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCoordinateMap {
    initial_filename: Arc<str>,
    text_len: TextSize,
    line_starts: Arc<[TextSize]>,
    segments: Arc<[LineDirectiveSegment]>,
}

impl SourceCoordinateMap {
    #[must_use]
    pub fn initial_filename(&self) -> &str {
        &self.initial_filename
    }

    #[must_use]
    pub const fn text_len(&self) -> TextSize {
        self.text_len
    }

    #[must_use]
    pub fn segments(&self) -> &[LineDirectiveSegment] {
        &self.segments
    }

    /// Resolve a consumed byte offset to its physical one-based coordinate.
    ///
    /// `Ok(None)` means the offset is beyond the scanned prefix represented by
    /// this map. EOF itself is included.
    pub fn physical_coordinate(
        &self,
        byte_offset: TextSize,
    ) -> Result<Option<PhysicalLineColumn>, SourceCoordinateMapError> {
        if byte_offset > self.text_len {
            return Ok(None);
        }
        let line = self
            .line_starts
            .partition_point(|line_start| *line_start <= byte_offset);
        let Some(line_start) = self.line_starts.get(line.saturating_sub(1)).copied() else {
            return Ok(None);
        };
        PhysicalLineColumn::try_from_usize(
            line,
            byte_offset
                .to_usize()
                .saturating_sub(line_start.to_usize())
                .saturating_add(1),
        )
        .map(Some)
        .map_err(SourceCoordinateMapError::Physical)
    }

    /// Apply the most recent Go line directive at or before `byte_offset`.
    pub fn adjusted_coordinate(
        &self,
        byte_offset: TextSize,
    ) -> Result<Option<AdjustedSourceCoordinate<'_>>, SourceCoordinateMapError> {
        let Some(physical) = self.physical_coordinate(byte_offset)? else {
            return Ok(None);
        };
        let segment = self
            .segments
            .partition_point(|segment| segment.physical_start <= byte_offset)
            .checked_sub(1)
            .and_then(|index| self.segments.get(index));
        let Some(segment) = segment else {
            let position = LogicalLineColumn::new(
                physical.line(),
                LogicalColumn::Known(physical.byte_column()),
            );
            return Ok(Some(AdjustedSourceCoordinate::new(
                &self.initial_filename,
                position,
            )));
        };

        let physical_line = physical.line().get();
        let base_physical_line = segment.physical_position.line().get();
        let line_delta = physical_line.saturating_sub(base_physical_line);
        let logical_line = u64::from(segment.logical_start.line().get()) + u64::from(line_delta);
        let logical_line = u32::try_from(logical_line)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(SourceCoordinateMapError::LogicalLine {
                value: logical_line,
            })?;
        let logical_column = match segment.logical_start.column() {
            LogicalColumn::Hidden => LogicalColumn::Hidden,
            LogicalColumn::Known(base) if line_delta == 0 => {
                let physical_delta = physical
                    .byte_column()
                    .get()
                    .saturating_sub(segment.physical_position.byte_column().get());
                let value = u64::from(base.get()) + u64::from(physical_delta);
                let column = u32::try_from(value)
                    .ok()
                    .and_then(NonZeroU32::new)
                    .ok_or(SourceCoordinateMapError::LogicalColumn { value })?;
                LogicalColumn::Known(column)
            }
            LogicalColumn::Known(_) => LogicalColumn::Known(physical.byte_column()),
        };
        Ok(Some(AdjustedSourceCoordinate::new(
            segment.adjusted_filename(),
            LogicalLineColumn::new(logical_line, logical_column),
        )))
    }
}

/// Coordinate-map construction or projection exceeded a fixed-width domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCoordinateMapError {
    TextSize(TextSizeOverflow),
    Physical(PhysicalLineColumnOverflow),
    LogicalLine { value: u64 },
    LogicalColumn { value: u64 },
}

impl fmt::Display for SourceCoordinateMapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextSize(error) => error.fmt(formatter),
            Self::Physical(error) => error.fmt(formatter),
            Self::LogicalLine { value } => {
                write!(
                    formatter,
                    "adjusted line {value} exceeds the u32 coordinate limit"
                )
            }
            Self::LogicalColumn { value } => write!(
                formatter,
                "adjusted byte column {value} exceeds the u32 coordinate limit"
            ),
        }
    }
}

impl std::error::Error for SourceCoordinateMapError {}

impl From<TextSizeOverflow> for SourceCoordinateMapError {
    fn from(error: TextSizeOverflow) -> Self {
        Self::TextSize(error)
    }
}

#[derive(Clone, Debug)]
pub struct SourceCoordinateMapBuilder {
    initial_filename: Arc<str>,
    line_starts: Vec<usize>,
    segments: Vec<RecordedSegment>,
}

#[derive(Clone, Debug)]
struct RecordedSegment {
    physical_start: usize,
    physical_line: usize,
    physical_column: usize,
    adjusted_filename: Arc<str>,
    logical_line: usize,
    logical_column: Option<usize>,
}

impl SourceCoordinateMapBuilder {
    pub fn new(initial_filename: &str) -> Self {
        Self {
            initial_filename: Arc::from(initial_filename),
            line_starts: vec![0],
            segments: Vec::new(),
        }
    }

    pub fn record_line_start(&mut self, physical_start: usize) {
        if self.line_starts.last().copied() != Some(physical_start) {
            self.line_starts.push(physical_start);
        }
    }

    pub fn record_directive(
        &mut self,
        physical_start: usize,
        physical_line: usize,
        physical_column: usize,
        adjusted_filename: Arc<str>,
        logical_line: usize,
        logical_column: Option<usize>,
    ) {
        debug_assert!(
            self.segments
                .last()
                .is_none_or(|segment| segment.physical_start < physical_start),
            "line-directive transitions must be recorded in byte order"
        );
        self.segments.push(RecordedSegment {
            physical_start,
            physical_line,
            physical_column,
            adjusted_filename,
            logical_line,
            logical_column,
        });
    }

    pub fn build(&self, text_len: usize) -> Result<SourceCoordinateMap, SourceCoordinateMapError> {
        let text_len = TextSize::try_from(text_len)?;
        let line_starts = self
            .line_starts
            .iter()
            .copied()
            .map(TextSize::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let segments = self
            .segments
            .iter()
            .map(|segment| {
                let logical_line = checked_nonzero_u32(segment.logical_line).ok_or(
                    SourceCoordinateMapError::LogicalLine {
                        value: segment.logical_line as u64,
                    },
                )?;
                let logical_column = match segment.logical_column {
                    Some(column) => LogicalColumn::Known(checked_nonzero_u32(column).ok_or(
                        SourceCoordinateMapError::LogicalColumn {
                            value: column as u64,
                        },
                    )?),
                    None => LogicalColumn::Hidden,
                };
                Ok(LineDirectiveSegment {
                    physical_start: TextSize::try_from(segment.physical_start)?,
                    physical_position: PhysicalLineColumn::try_from_usize(
                        segment.physical_line,
                        segment.physical_column,
                    )
                    .map_err(SourceCoordinateMapError::Physical)?,
                    adjusted_filename: Arc::clone(&segment.adjusted_filename),
                    logical_start: LogicalLineColumn::new(logical_line, logical_column),
                })
            })
            .collect::<Result<Vec<_>, SourceCoordinateMapError>>()?;
        Ok(SourceCoordinateMap {
            initial_filename: Arc::clone(&self.initial_filename),
            text_len,
            line_starts: line_starts.into(),
            segments: segments.into(),
        })
    }
}

fn checked_nonzero_u32(value: usize) -> Option<NonZeroU32> {
    u32::try_from(value).ok().and_then(NonZeroU32::new)
}
