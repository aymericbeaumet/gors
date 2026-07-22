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
    filename_projection: FilenameProjection,
    logical_start: LogicalLineColumn,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum FilenameProjection {
    Initial,
    Relative(Arc<str>),
    Exact(Arc<str>),
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
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdjustedSourceCoordinate {
    filename: Arc<str>,
    position: LogicalLineColumn,
}

impl AdjustedSourceCoordinate {
    #[must_use]
    pub fn new(filename: impl Into<Arc<str>>, position: LogicalLineColumn) -> Self {
        Self {
            filename: filename.into(),
            position,
        }
    }

    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    #[must_use]
    pub const fn position(&self) -> LogicalLineColumn {
        self.position
    }
}

/// Immutable physical-to-adjusted coordinate map for one scanned source.
///
/// The scanner constructs line starts and directive segments during its normal
/// single pass. Offsets remain authoritative even when a directive resets the
/// displayed line number or hides displayed columns.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceCoordinateMap {
    initial_filename: Arc<str>,
    text_len: TextSize,
    line_starts: Arc<[TextSize]>,
    segments: Arc<[LineDirectiveSegment]>,
}

impl SourceCoordinateMap {
    pub(crate) fn empty(initial_filename: &str) -> Self {
        Self {
            initial_filename: Arc::from(initial_filename),
            text_len: TextSize::ZERO,
            line_starts: Arc::from([TextSize::ZERO]),
            segments: Arc::from([]),
        }
    }

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
    ) -> Result<Option<AdjustedSourceCoordinate>, SourceCoordinateMapError> {
        self.adjusted_coordinate_for(byte_offset, &self.initial_filename)
    }

    /// Project an adjusted coordinate through a current presentation filename.
    ///
    /// Relative line-directive names are resolved lexically against this
    /// filename's directory. Empty, rooted, Windows-absolute, and URI names
    /// remain exact. This lets checkout-independent query maps be presented
    /// through a moved presentation path without rerunning the scanner or
    /// invalidating semantic queries.
    pub fn adjusted_coordinate_for(
        &self,
        byte_offset: TextSize,
        presentation_filename: &str,
    ) -> Result<Option<AdjustedSourceCoordinate>, SourceCoordinateMapError> {
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
                presentation_filename,
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
        let filename = project_filename(&segment.filename_projection, presentation_filename);
        Ok(Some(AdjustedSourceCoordinate::new(
            filename,
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
    active_filename: FilenameProjection,
    line_starts: Vec<usize>,
    segments: Vec<RecordedSegment>,
}

#[derive(Clone, Copy, Debug)]
pub enum FilenameUpdate<'a> {
    Set(&'a str),
    Retain,
    Clear,
}

#[derive(Clone, Debug)]
struct RecordedSegment {
    physical_start: usize,
    physical_line: usize,
    physical_column: usize,
    filename_projection: FilenameProjection,
    logical_line: usize,
    logical_column: Option<usize>,
}

impl SourceCoordinateMapBuilder {
    pub fn new(initial_filename: &str) -> Self {
        Self {
            initial_filename: Arc::from(initial_filename),
            active_filename: FilenameProjection::Initial,
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
        filename_update: FilenameUpdate<'_>,
        logical_line: usize,
        logical_column: Option<usize>,
    ) {
        debug_assert!(
            self.segments
                .last()
                .is_none_or(|segment| segment.physical_start < physical_start),
            "line-directive transitions must be recorded in byte order"
        );
        self.active_filename = match filename_update {
            FilenameUpdate::Set(filename) if is_rooted_source_name(filename) => {
                FilenameProjection::Exact(Arc::from(filename))
            }
            FilenameUpdate::Set(filename) => FilenameProjection::Relative(Arc::from(filename)),
            FilenameUpdate::Retain => self.active_filename.clone(),
            FilenameUpdate::Clear => FilenameProjection::Exact(Arc::from("")),
        };
        self.segments.push(RecordedSegment {
            physical_start,
            physical_line,
            physical_column,
            filename_projection: self.active_filename.clone(),
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
                    adjusted_filename: project_filename(
                        &segment.filename_projection,
                        &self.initial_filename,
                    ),
                    filename_projection: segment.filename_projection.clone(),
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

fn project_filename(projection: &FilenameProjection, initial_filename: &str) -> Arc<str> {
    match projection {
        FilenameProjection::Initial => Arc::from(initial_filename),
        FilenameProjection::Exact(filename) => Arc::clone(filename),
        FilenameProjection::Relative(filename) => {
            let base = source_directory_prefix(initial_filename);
            if base.is_empty() {
                Arc::clone(filename)
            } else {
                Arc::from(format!("{base}{filename}"))
            }
        }
    }
}

/// Return the source directory with its original trailing separator.
///
/// Source names are lexical inputs, not host filesystem paths: recognizing
/// both separators keeps Windows paths and browser URIs deterministic on every
/// Cargo target.
pub fn source_directory_prefix(filename: &str) -> &str {
    let separator = filename
        .char_indices()
        .rev()
        .find(|(_, character)| matches!(character, '/' | '\\'));
    separator.map_or("", |(index, character)| {
        &filename[..index + character.len_utf8()]
    })
}

pub fn is_rooted_source_name(filename: &str) -> bool {
    let bytes = filename.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return true;
    }

    if matches!(
        bytes,
        [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic()
    ) {
        return true;
    }

    let Some(colon) = filename.find(':') else {
        return false;
    };
    colon > 1
        && filename[..colon].bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')))
        })
}
