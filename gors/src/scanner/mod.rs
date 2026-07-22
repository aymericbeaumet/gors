//! Go lexical scanner.
//!
//! This module implements lexical analysis for Go source code as defined
//! in the [Go language specification](https://golang.org/ref/spec#Lexical_elements).

mod error;
mod lexemes;
mod lexical;
mod scan;
#[cfg(test)]
mod tests;

pub use error::{Result, ScannerError, ScannerErrorKind};

use crate::compiler::source::coordinate_map::{
    FilenameUpdate, SourceCoordinateMapBuilder, is_rooted_source_name, source_directory_prefix,
};
use crate::compiler::source::{SourceCoordinateMap, SourceCoordinateMapError};
use crate::token::{Position, SourceOrigin, Token};
use lexical::{is_hex_digit, is_octal_digit};

/// A scan step containing position, token, and literal value.
///
/// Each step represents a single token from the source code along with
/// its position and the original literal text.
pub type Step<'a> = (Position<'a>, Token, &'a str);

/// Go source code scanner (lexer).
///
/// The Scanner performs lexical analysis on Go source code, breaking it
/// into tokens according to the Go language specification. It handles:
///
/// - Keywords and identifiers
/// - Numeric, string, and character literals
/// - Operators and delimiters
/// - Comments (single-line and multi-line)
/// - Automatic semicolon insertion
/// - Line directives (`//line` and `/*line`)
#[derive(Debug)]
pub struct Scanner<'a> {
    origin: SourceOrigin<'a>,
    line_directive_base: &'a str,
    buffer: &'a str,
    //
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    current_char: Option<char>,
    current_char_len: usize,
    //
    offset: usize,
    line: usize,
    column: usize,
    physical_line: usize,
    physical_column: usize,
    start_offset: usize,
    start_line: usize,
    start_column: usize,
    start_physical_column: usize,
    //
    hide_column: bool,
    coordinate_map: SourceCoordinateMapBuilder,
    insert_semi: bool,
    pending_line_info: Option<LineInfo<'a>>,
    pending_semi: bool, // true if a semicolon should be returned immediately on next scan
    pending_semi_pos: Option<(usize, usize, usize)>, // (offset, line, column) for semicolon after multi-line comment
}

#[derive(Clone, Copy, Debug)]
struct LineInfo<'a> {
    filename: LineFilename<'a>,
    line: usize,
    column: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
enum LineFilename<'a> {
    Set(&'a str),
    Retain,
    Clear,
}

impl<'a> Scanner<'a> {
    /// Create a new Scanner for the given source file.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the source file (may include path)
    /// * `buffer` - The Go source code to scan
    pub fn new(filename: &'a str, buffer: &'a str) -> Self {
        let mut s = Scanner {
            origin: SourceOrigin::initial(filename),
            line_directive_base: source_directory_prefix(filename),
            buffer,
            //
            chars: buffer.chars().peekable(),
            current_char: None,
            current_char_len: 0,
            //
            offset: 0,
            line: 1,
            column: 1,
            physical_line: 1,
            physical_column: 1,
            start_offset: 0,
            start_line: 1,
            start_column: 1,
            start_physical_column: 1,
            //
            hide_column: false,
            coordinate_map: SourceCoordinateMapBuilder::new(filename),
            insert_semi: false,
            pending_line_info: None,
            pending_semi: false,
            pending_semi_pos: None,
        };
        s.next(); // read the first character
        if s.current_char == Some('\u{feff}') && s.offset == 0 {
            s.next();
        }
        s
    }

    /// Build the coordinate map recorded by this scanner's existing pass.
    ///
    /// Before EOF this map covers only the consumed prefix through the current
    /// byte offset. After EOF it covers the complete source, including EOF.
    pub fn source_coordinate_map(
        &self,
    ) -> std::result::Result<SourceCoordinateMap, SourceCoordinateMapError> {
        self.coordinate_map.build(self.offset)
    }

    fn consume_pending_line_info(&mut self) {
        if let Some(line_info) = self.pending_line_info.take() {
            // Match go/token.File.AddLineColumnInfo: a directive transition at
            // EOF is ignored because alternative offsets must be < file size.
            if self.offset >= self.buffer.len() {
                return;
            }
            let filename_update = match line_info.filename {
                LineFilename::Set(filename) => FilenameUpdate::Set(filename),
                LineFilename::Retain => FilenameUpdate::Retain,
                LineFilename::Clear => FilenameUpdate::Clear,
            };
            match line_info.filename {
                LineFilename::Set(filename) => {
                    let relative_to = if is_rooted_source_name(filename) {
                        ""
                    } else {
                        self.line_directive_base
                    };
                    self.origin = SourceOrigin::line_directive(filename, relative_to);
                }
                LineFilename::Retain => {}
                LineFilename::Clear => {
                    self.origin = SourceOrigin::line_directive("", "");
                }
            }

            self.line = line_info.line;

            if let Some(column) = line_info.column {
                self.column = column;
            }

            self.hide_column = line_info.column.is_none();
            self.coordinate_map.record_directive(
                self.offset,
                self.physical_line,
                self.physical_column,
                filename_update,
                line_info.line,
                line_info.column,
            );
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn next(&mut self) {
        self.offset += self.current_char_len;
        self.column += self.current_char_len;
        self.physical_column += self.current_char_len;
        let last_char = self.current_char;

        self.current_char = self.chars.next();
        self.current_char_len = self.current_char.map_or(0, char::len_utf8);
        if matches!(last_char, Some('\n')) {
            if self.offset < self.buffer.len() {
                self.line += 1;
                self.column = 1;
                self.physical_line += 1;
                self.physical_column = 1;
                self.coordinate_map.record_line_start(self.offset);
            }
            self.consume_pending_line_info();
        }

        log::trace!(
            "self.current_char={:?} offset={} line={} column={}",
            self.current_char,
            self.offset,
            self.line,
            self.column,
        );
    }

    const fn position(&self) -> Position<'a> {
        Position {
            origin: self.origin,
            offset: self.start_offset,
            line: self.start_line,
            column: if self.hide_column {
                0
            } else {
                self.start_column
            },
        }
    }

    fn reset_start(&mut self) {
        self.start_offset = self.offset;
        self.start_line = self.line;
        self.start_column = self.column;
        self.start_physical_column = self.physical_column;
    }

    fn literal(&self) -> &'a str {
        &self.buffer[self.start_offset..self.offset]
    }

    fn error(&self, kind: ScannerErrorKind) -> ScannerError {
        ScannerError {
            kind,
            file: self.origin.filename().into_owned(),
            line: self.line,
            column: if self.hide_column { 0 } else { self.column },
            offset: self.offset,
        }
    }

    fn require_escaped_char<const DELIM: char>(&mut self) -> Result<()> {
        self.next();

        let c = self
            .current_char
            .ok_or_else(|| self.error(ScannerErrorKind::UnterminatedEscapedChar))?;

        // Note: This check is separate because Rust doesn't support const generic parameters
        // in match patterns (e.g., `DELIM => ...`). See rust-lang/rust#76001.
        if c == DELIM {
            self.next();
            return Ok(());
        }

        match c {
            'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | '\\' => self.next(),
            'x' => {
                self.next();
                self.require_hex_digits::<2>()?
            }
            'u' => {
                self.next();
                let start = self.offset;
                self.require_hex_digits::<4>()?;
                self.validate_unicode_code_point(&self.buffer[start..start + 4])?;
            }
            'U' => {
                self.next();
                let start = self.offset;
                self.require_hex_digits::<8>()?;
                self.validate_unicode_code_point(&self.buffer[start..start + 8])?;
            }
            '0'..='7' => self.require_octal_digits::<3>()?,
            _ => return Err(self.error(ScannerErrorKind::UnterminatedEscapedChar)),
        }

        Ok(())
    }

    fn validate_unicode_code_point(&self, hex_str: &str) -> Result<()> {
        let cp = u32::from_str_radix(hex_str, 16)
            .map_err(|_| self.error(ScannerErrorKind::InvalidUnicodeCodePoint))?;
        if (0xD800..=0xDFFF).contains(&cp) || cp > 0x10FFFF {
            return Err(self.error(ScannerErrorKind::InvalidUnicodeCodePoint));
        }
        Ok(())
    }

    fn require_octal_digits<const COUNT: usize>(&mut self) -> Result<()> {
        for _ in 0..COUNT {
            let c = self
                .current_char
                .ok_or_else(|| self.error(ScannerErrorKind::OctalNotFound))?;

            if !is_octal_digit(c) {
                return Err(self.error(ScannerErrorKind::OctalNotFound));
            }

            self.next();
        }

        Ok(())
    }

    fn require_hex_digits<const COUNT: usize>(&mut self) -> Result<()> {
        for _ in 0..COUNT {
            let c = self
                .current_char
                .ok_or_else(|| self.error(ScannerErrorKind::HexadecimalNotFound))?;

            if !is_hex_digit(c) {
                return Err(self.error(ScannerErrorKind::HexadecimalNotFound));
            }

            self.next();
        }

        Ok(())
    }
}

impl<'a> IntoIterator for Scanner<'a> {
    type Item = Result<Step<'a>>;
    type IntoIter = IntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        Self::IntoIter::new(self)
    }
}

pub struct IntoIter<'a> {
    scanner: Scanner<'a>,
    done: bool,
}

impl<'a> IntoIter<'a> {
    const fn new(scanner: Scanner<'a>) -> Self {
        Self {
            scanner,
            done: false,
        }
    }

    /// Coordinate map recorded by the owned scanner without rescanning.
    ///
    /// After this iterator yields EOF the map covers the complete source.
    pub fn source_coordinate_map(
        &self,
    ) -> std::result::Result<SourceCoordinateMap, SourceCoordinateMapError> {
        self.scanner.source_coordinate_map()
    }
}

impl<'a> Iterator for IntoIter<'a> {
    type Item = Result<Step<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        match self.scanner.scan() {
            Ok((pos, tok, lit)) => {
                if tok == Token::EOF {
                    self.done = true;
                }
                Some(Ok((pos, tok, lit)))
            }
            Err(err) => {
                self.done = true;
                Some(Err(err))
            }
        }
    }
}
