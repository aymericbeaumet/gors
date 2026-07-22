//! Go lexical scanner.
//!
//! This module implements lexical analysis for Go source code as defined
//! in the [Go language specification](https://golang.org/ref/spec#Lexical_elements).

mod error;
mod lexemes;
mod lexical;
#[cfg(test)]
mod tests;

pub use error::{Result, ScannerError, ScannerErrorKind};

use crate::token::{Position, Token};
use lexical::{is_hex_digit, is_letter, is_octal_digit};

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
    directory: &'a str,
    file: &'a str,
    buffer: &'a str,
    //
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    current_char: Option<char>,
    current_char_len: usize,
    //
    offset: usize,
    line: usize,
    column: usize,
    start_offset: usize,
    start_line: usize,
    start_column: usize,
    //
    hide_column: bool,
    insert_semi: bool,
    pending_line_info: Option<LineInfo<'a>>,
    pending_semi: bool, // true if a semicolon should be returned immediately on next scan
    pending_semi_pos: Option<(usize, usize, usize)>, // (offset, line, column) for semicolon after multi-line comment
}

type LineInfo<'a> = (Option<&'a str>, usize, Option<usize>, bool);

impl<'a> Scanner<'a> {
    /// Create a new Scanner for the given source file.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the source file (may include path)
    /// * `buffer` - The Go source code to scan
    pub fn new(filename: &'a str, buffer: &'a str) -> Self {
        let (directory, file) = filename.rsplit_once('/').unwrap_or(("", filename));
        let mut s = Scanner {
            directory,
            file,
            buffer,
            //
            chars: buffer.chars().peekable(),
            current_char: None,
            current_char_len: 0,
            //
            offset: 0,
            line: 1,
            column: 1,
            start_offset: 0,
            start_line: 1,
            start_column: 1,
            //
            hide_column: false,
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

    #[allow(clippy::cognitive_complexity)] // Allow complex scan function
    pub fn scan(&mut self) -> Result<Step<'a>> {
        // Check for pending semicolon (from multi-line comment with newlines)
        if self.pending_semi {
            self.pending_semi = false;
            let pos = if let Some((offset, line, column)) = self.pending_semi_pos.take() {
                Position {
                    directory: self.directory,
                    file: self.file,
                    offset,
                    line,
                    column: if self.hide_column { 0 } else { column },
                }
            } else {
                self.position()
            };
            return Ok((pos, Token::SEMICOLON, "\n"));
        }

        let insert_semi = self.insert_semi;
        self.insert_semi = false;

        while let Some(c) = self.current_char {
            self.reset_start();

            match c {
                ' ' | '\t' | '\r' => {
                    self.next();
                }

                '\n' => {
                    self.next();
                    if insert_semi {
                        let pos = if let Some((offset, line, column)) = self.pending_semi_pos.take()
                        {
                            Position {
                                directory: self.directory,
                                file: self.file,
                                offset,
                                line,
                                column: if self.hide_column { 0 } else { column },
                            }
                        } else {
                            self.position()
                        };
                        return Ok((pos, Token::SEMICOLON, "\n"));
                    }
                }

                _ => break,
            }
        }

        if let Some(c) = self.current_char {
            match c {
                '+' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::ADD_ASSIGN, ""));
                        }
                        Some('+') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.position(), Token::INC, ""));
                        }
                        _ => return Ok((self.position(), Token::ADD, "")),
                    }
                }

                '-' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::SUB_ASSIGN, ""));
                        }
                        Some('-') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.position(), Token::DEC, ""));
                        }
                        _ => return Ok((self.position(), Token::SUB, "")),
                    }
                }

                '*' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::MUL_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::MUL, "")),
                    }
                }

                '/' => match self.peek() {
                    Some('=') => {
                        self.next();
                        self.next();
                        return Ok((self.position(), Token::QUO_ASSIGN, ""));
                    }
                    Some('/') => {
                        // Line comments: scan the comment first, preserve semicolon insertion
                        // for the newline that follows. This matches Go's scanner behavior.
                        if insert_semi {
                            self.insert_semi = true;
                        }
                        return self.scan_line_comment();
                    }
                    Some('*') => {
                        // General comments: scan the comment first, preserve semicolon insertion
                        // if this comment extends to end of line. This matches Go's scanner behavior.
                        let track_semi_pos = insert_semi && self.find_line_end();
                        // Note: we don't set self.insert_semi here - scan_general_comment will
                        // set pending_semi if the comment contains newlines, which handles the
                        // semicolon insertion. If the comment doesn't contain newlines but
                        // find_line_end() returned true, we need to preserve insert_semi.
                        return self.scan_general_comment(track_semi_pos);
                    }
                    _ => {
                        self.next();
                        return Ok((self.position(), Token::QUO, ""));
                    }
                },

                '%' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::REM_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::REM, "")),
                    }
                }

                '&' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::AND_ASSIGN, ""));
                        }
                        Some('&') => {
                            self.next();
                            return Ok((self.position(), Token::LAND, ""));
                        }
                        Some('^') => {
                            self.next();
                            match self.current_char {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.position(), Token::AND_NOT_ASSIGN, ""));
                                }
                                _ => return Ok((self.position(), Token::AND_NOT, "")),
                            }
                        }
                        _ => return Ok((self.position(), Token::AND, "")),
                    }
                }

                '|' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::OR_ASSIGN, ""));
                        }
                        Some('|') => {
                            self.next();
                            return Ok((self.position(), Token::LOR, ""));
                        }
                        _ => return Ok((self.position(), Token::OR, "")),
                    }
                }

                '^' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::XOR_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::XOR, "")),
                    }
                }

                '<' => {
                    self.next();
                    match self.current_char {
                        Some('<') => {
                            self.next();
                            match self.current_char {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.position(), Token::SHL_ASSIGN, ""));
                                }
                                _ => return Ok((self.position(), Token::SHL, "")),
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::LEQ, ""));
                        }
                        Some('-') => {
                            self.next();
                            return Ok((self.position(), Token::ARROW, ""));
                        }
                        _ => return Ok((self.position(), Token::LSS, "")),
                    }
                }

                '>' => {
                    self.next();
                    match self.current_char {
                        Some('>') => {
                            self.next();
                            match self.current_char {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.position(), Token::SHR_ASSIGN, ""));
                                }
                                _ => {
                                    return Ok((self.position(), Token::SHR, ""));
                                }
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::GEQ, ""));
                        }
                        _ => return Ok((self.position(), Token::GTR, "")),
                    }
                }

                ':' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::DEFINE, ""));
                        }
                        _ => return Ok((self.position(), Token::COLON, "")),
                    }
                }

                '!' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::NEQ, ""));
                        }
                        _ => return Ok((self.position(), Token::NOT, "")),
                    }
                }

                ',' => {
                    self.next();
                    return Ok((self.position(), Token::COMMA, ""));
                }

                '(' => {
                    self.next();
                    return Ok((self.position(), Token::LPAREN, ""));
                }

                ')' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.position(), Token::RPAREN, ""));
                }

                '[' => {
                    self.next();
                    return Ok((self.position(), Token::LBRACK, ""));
                }

                ']' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.position(), Token::RBRACK, ""));
                }

                '{' => {
                    self.next();
                    return Ok((self.position(), Token::LBRACE, ""));
                }

                '}' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.position(), Token::RBRACE, ""));
                }

                '~' => {
                    self.next();
                    return Ok((self.position(), Token::TILDE, ""));
                }

                ';' => {
                    self.next();
                    return Ok((self.position(), Token::SEMICOLON, ";"));
                }

                '.' => {
                    self.next();
                    match self.current_char {
                        Some('0'..='9') => return self.scan_int_or_float_or_imag(true),
                        Some('.') => match self.peek() {
                            Some('.') => {
                                self.next();
                                self.next();
                                return Ok((self.position(), Token::ELLIPSIS, ""));
                            }
                            _ => return Ok((self.position(), Token::PERIOD, "")),
                        },
                        _ => return Ok((self.position(), Token::PERIOD, "")),
                    }
                }

                '=' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::EQL, ""));
                        }
                        _ => return Ok((self.position(), Token::ASSIGN, "")),
                    }
                }

                '0'..='9' => return self.scan_int_or_float_or_imag(false),
                '\'' => return self.scan_rune(),
                '"' => return self.scan_interpreted_string(),
                '`' => return self.scan_raw_string(),
                c if is_letter(c) => return self.scan_pkg_or_keyword_or_ident(),
                _ => {
                    return Err(ScannerError {
                        kind: ScannerErrorKind::IllegalCharacter,
                        line: self.line,
                        column: self.column,
                        offset: self.offset,
                    });
                }
            };
        }

        self.reset_start();
        if insert_semi {
            let pos = if let Some((offset, line, column)) = self.pending_semi_pos.take() {
                Position {
                    directory: self.directory,
                    file: self.file,
                    offset,
                    line,
                    column: if self.hide_column { 0 } else { column },
                }
            } else {
                self.position()
            };
            Ok((pos, Token::SEMICOLON, "\n"))
        } else {
            Ok((self.position(), Token::EOF, ""))
        }
    }

    fn consume_pending_line_info(&mut self) {
        if let Some(line_info) = self.pending_line_info.take() {
            if let Some(file) = line_info.0 {
                self.file = file;
            }

            self.line = line_info.1;

            if let Some(column) = line_info.2 {
                self.column = column;
            }

            self.hide_column = line_info.3;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn next(&mut self) {
        self.offset += self.current_char_len;
        self.column += self.current_char_len;
        let last_char = self.current_char;

        self.current_char = self.chars.next();
        if let Some(c) = self.current_char {
            self.current_char_len = c.len_utf8();
            if matches!(last_char, Some('\n')) {
                self.line += 1;
                self.column = 1;
                self.consume_pending_line_info();
            }
        } else {
            self.current_char_len = 0
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
            directory: self.directory,
            file: self.file,
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
    }

    fn literal(&self) -> &'a str {
        &self.buffer[self.start_offset..self.offset]
    }

    fn error(&self, kind: ScannerErrorKind) -> ScannerError {
        ScannerError {
            kind,
            line: self.line,
            column: self.column,
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
