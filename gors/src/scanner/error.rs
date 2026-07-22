use std::fmt;

/// Error type for scanner failures.
///
/// Contains the kind of error, along with line, column, and offset
/// information for error reporting.
#[derive(Debug, Clone)]
pub struct ScannerError {
    pub kind: ScannerErrorKind,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerErrorKind {
    HexadecimalNotFound,
    OctalNotFound,
    UnterminatedComment,
    UnterminatedEscapedChar,
    UnterminatedRune,
    UnterminatedString,
    InvalidDirective,
    IllegalCharacter,
    InvalidUnicodeCodePoint,
}

impl ScannerError {
    pub fn message(&self) -> &'static str {
        match self.kind {
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
}

impl std::error::Error for ScannerError {}

impl fmt::Display for ScannerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: {}",
            self.line,
            self.column,
            self.message()
        )
    }
}

pub type Result<T> = std::result::Result<T, ScannerError>;
