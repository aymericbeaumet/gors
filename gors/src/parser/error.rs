//! Located parser failures and private parser-control errors.

use std::fmt;
use std::sync::Arc;

use crate::compiler::source::{
    LogicalColumn, LogicalLineColumn, SourceCoordinateMap, SourceCoordinateMapError, TextRange,
    TextSize, TextSizeOverflow,
};
use crate::scanner::{ScannerError, ScannerErrorKind};
use crate::token::Token;

/// The semantic reason that parsing stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParserErrorKind {
    /// The scanner rejected a token before the grammar could consume it.
    Scanner(ScannerErrorKind),
    /// The source ended while the grammar still required input.
    UnexpectedEndOfFile,
    /// The current token cannot occur in this grammar position.
    UnexpectedToken { token: Token, literal: Arc<str> },
    /// The source cannot fit the compiler's fixed-width coordinate domain.
    SourceTooLarge(TextSizeOverflow),
    /// A physical or adjusted coordinate cannot fit its typed domain.
    CoordinateMap(SourceCoordinateMapError),
    /// The scanner map did not cover the parser's physical failure anchor.
    CoordinateUnavailable {
        offset: TextSize,
        scanned_prefix: TextSize,
    },
}

/// A parser failure with authoritative physical and adjusted coordinates.
///
/// The coordinate map is the prefix consumed by the scanner that produced the
/// failure. The physical range always addresses the original bytes; adjusted
/// coordinates exist only for display and may name a virtual `//line` file.
#[derive(Clone, Debug)]
pub struct ParserError {
    kind: ParserErrorKind,
    physical_range: TextRange,
    adjusted_filename: Arc<str>,
    logical_position: LogicalLineColumn,
    coordinate_map: SourceCoordinateMap,
}

impl ParserError {
    pub(crate) fn located(
        kind: ParserErrorKind,
        physical_offset: TextSize,
        coordinate_map: SourceCoordinateMap,
    ) -> Self {
        match coordinate_map.adjusted_coordinate(physical_offset) {
            Ok(Some(coordinate)) => Self {
                kind,
                physical_range: TextRange::empty(physical_offset),
                adjusted_filename: Arc::from(coordinate.filename()),
                logical_position: coordinate.position(),
                coordinate_map,
            },
            Ok(None) => Self::unavailable_coordinate(physical_offset, coordinate_map),
            Err(error) => Self::coordinate_failure(error, physical_offset, coordinate_map),
        }
    }

    pub(crate) fn source_too_large(
        filename: &str,
        error: TextSizeOverflow,
        coordinate_map: SourceCoordinateMap,
    ) -> Self {
        Self {
            kind: ParserErrorKind::SourceTooLarge(error),
            physical_range: TextRange::empty(TextSize::ZERO),
            adjusted_filename: Arc::from(filename),
            logical_position: LogicalLineColumn::new(
                std::num::NonZeroU32::MIN,
                LogicalColumn::Known(std::num::NonZeroU32::MIN),
            ),
            coordinate_map,
        }
    }

    pub(crate) fn coordinate_failure(
        error: SourceCoordinateMapError,
        physical_offset: TextSize,
        coordinate_map: SourceCoordinateMap,
    ) -> Self {
        Self {
            kind: ParserErrorKind::CoordinateMap(error),
            physical_range: TextRange::empty(physical_offset),
            adjusted_filename: Arc::from(coordinate_map.initial_filename()),
            logical_position: LogicalLineColumn::new(
                std::num::NonZeroU32::MIN,
                LogicalColumn::Known(std::num::NonZeroU32::MIN),
            ),
            coordinate_map,
        }
    }

    fn unavailable_coordinate(
        physical_offset: TextSize,
        coordinate_map: SourceCoordinateMap,
    ) -> Self {
        Self {
            kind: ParserErrorKind::CoordinateUnavailable {
                offset: physical_offset,
                scanned_prefix: coordinate_map.text_len(),
            },
            physical_range: TextRange::empty(physical_offset),
            adjusted_filename: Arc::from(coordinate_map.initial_filename()),
            logical_position: LogicalLineColumn::new(
                std::num::NonZeroU32::MIN,
                LogicalColumn::Known(std::num::NonZeroU32::MIN),
            ),
            coordinate_map,
        }
    }

    /// Structured parser failure reason.
    #[must_use]
    pub const fn kind(&self) -> &ParserErrorKind {
        &self.kind
    }

    /// Human-readable diagnostic message without a location prefix.
    #[must_use]
    pub fn message(&self) -> String {
        match &self.kind {
            ParserErrorKind::Scanner(kind) => scanner_message(*kind).to_string(),
            ParserErrorKind::UnexpectedEndOfFile => "unexpected end of file".to_string(),
            ParserErrorKind::UnexpectedToken { token, literal } => {
                let token_name: &str = token.into();
                if literal.is_empty() {
                    format!("unexpected token '{token_name}'")
                } else if token_name == literal.as_ref() {
                    format!("unexpected token '{literal}'")
                } else {
                    format!("unexpected {token_name} '{literal}'")
                }
            }
            ParserErrorKind::SourceTooLarge(error) => error.to_string(),
            ParserErrorKind::CoordinateMap(error) => error.to_string(),
            ParserErrorKind::CoordinateUnavailable {
                offset,
                scanned_prefix,
            } => format!(
                "parser byte offset {} is beyond scanned prefix {}",
                offset.get(),
                scanned_prefix.get()
            ),
        }
    }

    /// Exact zero-width physical byte anchor in the original source.
    #[must_use]
    pub const fn physical_range(&self) -> TextRange {
        self.physical_range
    }

    /// Initial or `//line`-adjusted filename at the failure site.
    #[must_use]
    pub fn adjusted_filename(&self) -> &str {
        &self.adjusted_filename
    }

    /// Typed Go display line and column at the failure site.
    #[must_use]
    pub const fn logical_position(&self) -> LogicalLineColumn {
        self.logical_position
    }

    /// Scanner-built map for the consumed source prefix.
    #[must_use]
    pub const fn source_coordinate_map(&self) -> &SourceCoordinateMap {
        &self.coordinate_map
    }

    /// Whether this syntax failure originated in tokenization.
    #[must_use]
    pub const fn is_scanner_error(&self) -> bool {
        matches!(self.kind, ParserErrorKind::Scanner(_))
    }
}

impl std::error::Error for ParserError {}

impl fmt::Display for ParserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_location(formatter, &self.adjusted_filename, self.logical_position)?;
        if self.is_scanner_error() {
            write!(formatter, ": {}", self.message())
        } else {
            write!(formatter, ": syntax error: {}", self.message())
        }
    }
}

fn write_location(
    formatter: &mut fmt::Formatter<'_>,
    filename: &str,
    position: LogicalLineColumn,
) -> fmt::Result {
    if filename.is_empty() {
        write!(formatter, "{}", position.line())?;
    } else {
        write!(formatter, "{}:{}", filename, position.line())?;
    }
    if let LogicalColumn::Known(column) = position.column() {
        write!(formatter, ":{column}")?;
    }
    Ok(())
}

const fn scanner_message(kind: ScannerErrorKind) -> &'static str {
    match kind {
        ScannerErrorKind::HexadecimalNotFound => "hexadecimal digit not found",
        ScannerErrorKind::OctalNotFound => "octal digit not found",
        ScannerErrorKind::UnterminatedComment => "comment not terminated",
        ScannerErrorKind::UnterminatedEscapedChar => "invalid escape sequence",
        ScannerErrorKind::UnterminatedRune => "rune literal not terminated",
        ScannerErrorKind::UnterminatedString => "string literal not terminated",
        ScannerErrorKind::InvalidDirective => "invalid compiler directive",
        ScannerErrorKind::IllegalCharacter => "illegal character",
        ScannerErrorKind::InvalidUnicodeCodePoint => {
            "escape sequence is invalid Unicode code point"
        }
    }
}

/// Parser-control failure before the public file boundary attaches locations.
#[derive(Debug, Clone)]
pub enum RawParserError {
    Scanner(ScannerError),
    UnexpectedEndOfFile { offset: usize },
    UnexpectedToken,
}

impl From<ScannerError> for RawParserError {
    fn from(error: ScannerError) -> Self {
        Self::Scanner(error)
    }
}

pub type Result<T> = std::result::Result<T, RawParserError>;
