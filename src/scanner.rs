// https://golang.org/ref/spec#Lexical_elements

use crate::token::{Position, Token};
use phf::{phf_map, Map};
use std::fmt;

// TODO: match the errors from the Go scanner
#[derive(Debug)]
pub enum ScannerError {
    HexadecimalNotFound,
    OctalNotFound,
    UnterminatedComment,
    UnterminatedEscapedChar,
    UnterminatedRune,
    UnterminatedString,
}

impl std::error::Error for ScannerError {}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scanner error: {:?}", self)
    }
}

#[derive(Debug)]
pub struct Scanner<'a> {
    directory: &'a str,
    file: &'a str,
    buffer: &'a str,
    //
    chars: std::str::Chars<'a>,
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
    //
    pending_line_info: Option<(Option<&'a str>, usize, Option<usize>, bool)>,
    pending_out: Option<(Position<'a>, Token, &'a str)>,
}

impl<'a> Scanner<'a> {
    pub fn new(filename: &'a str, buffer: &'a str) -> Self {
        let (directory, file) = filename.rsplit_once('/').unwrap();
        let mut s = Scanner {
            directory,
            file,
            buffer,
            //
            chars: buffer.chars(),
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
            //
            pending_line_info: None,
            pending_out: None,
        };
        s.next(); // read the first character
        s
    }

    pub fn scan(&mut self) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        let insert_semi = self.insert_semi;
        self.insert_semi = false;

        while let Some(c) = self.peek() {
            self.reset_start();

            match c {
                ' ' | '\t' | '\r' => {
                    self.next();
                }

                '\n' => {
                    self.next();
                    if insert_semi {
                        if let Some(pending_out) = &self.pending_out {
                            if pending_out.1 == Token::COMMENT {
                                return Ok((pending_out.0, Token::SEMICOLON, "\n"));
                            }
                        }
                        return Ok((self.position(), Token::SEMICOLON, "\n"));
                    }
                }

                _ => break,
            }
        }

        if let Some(pending_out) = self.pending_out.take() {
            return Ok(pending_out);
        }

        if let Some(c) = self.peek() {
            match c {
                '+' => {
                    self.next();
                    match self.peek() {
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
                    match self.peek() {
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
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::MUL_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::MUL, "")),
                    }
                }

                '/' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::QUO_ASSIGN, ""));
                        }
                        Some('/') => {
                            let out = self.scan_line_comment()?;
                            // "Any other comment acts like a newline."
                            if insert_semi {
                                self.pending_out = Some(out);
                                return Ok((self.position(), Token::SEMICOLON, "\n"));
                            }
                            return Ok(out);
                        }
                        Some('*') => {
                            let out = self.scan_general_comment()?;
                            // "A general comment containing no newlines acts like a space."
                            if !out.2.contains('\n') {
                                self.pending_out = Some(out);
                                self.insert_semi = insert_semi;
                                return self.scan();
                            }
                            // "Any other comment acts like a newline."
                            if insert_semi {
                                self.pending_out = Some(out);
                                return Ok((self.position(), Token::SEMICOLON, "\n"));
                            }
                            return Ok(out);
                        }
                        _ => return Ok((self.position(), Token::QUO, "")),
                    }
                }

                '%' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::REM_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::REM, "")),
                    }
                }

                '&' => {
                    self.next();
                    match self.peek() {
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
                            match self.peek() {
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
                    match self.peek() {
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
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::XOR_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::XOR, "")),
                    }
                }

                '<' => {
                    self.next();
                    match self.peek() {
                        Some('<') => {
                            self.next();
                            match self.peek() {
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
                    match self.peek() {
                        Some('>') => {
                            self.next();
                            match self.peek() {
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
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::DEFINE, ""));
                        }
                        _ => return Ok((self.position(), Token::COLON, "")),
                    }
                }

                '!' => {
                    self.next();
                    match self.peek() {
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

                ';' => {
                    self.next();
                    return Ok((self.position(), Token::SEMICOLON, ";"));
                }

                '.' => {
                    let pos = self.position();
                    self.next();
                    match self.peek() {
                        Some('0'..='9') => return self.scan_int_or_float_or_imag(true),
                        Some('.') => {
                            self.reset_start();
                            self.next();
                            match self.peek() {
                                Some('.') => {
                                    self.next();
                                    return Ok((pos, Token::ELLIPSIS, ""));
                                }
                                _ => {
                                    self.pending_out = Some(self.scan_int_or_float_or_imag(true)?);
                                    return Ok((pos, Token::PERIOD, ""));
                                }
                            }
                        }
                        _ => return Ok((self.position(), Token::PERIOD, "")),
                    }
                }

                '=' => {
                    self.next();
                    match self.peek() {
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
                _ => return self.scan_pkg_or_keyword_or_ident(),
            };
        }

        self.reset_start();
        if insert_semi {
            Ok((self.position(), Token::SEMICOLON, "\n"))
        } else {
            Ok((self.position(), Token::EOF, ""))
        }
    }

    // https://golang.org/ref/spec#Keywords
    // https://golang.org/ref/spec#Identifiers
    fn scan_pkg_or_keyword_or_ident(
        &mut self,
    ) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !(is_letter(c) || is_unicode_digit(c)) {
                break;
            }
            self.next()
        }

        let pos = self.position();
        let literal = self.literal();
        if let Some(&token) = KEYWORDS.get(literal) {
            self.insert_semi = matches!(
                token,
                Token::BREAK | Token::CONTINUE | Token::FALLTHROUGH | Token::RETURN
            );
            Ok((pos, token, literal))
        } else {
            self.insert_semi = true;
            Ok((pos, Token::IDENT, literal))
        }
    }

    // https://golang.org/ref/spec#Integer_literals
    // https://golang.org/ref/spec#Floating-point_literals
    // https://golang.org/ref/spec#Imaginary_literals
    fn scan_int_or_float_or_imag(
        &mut self,
        preceding_dot: bool,
    ) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.insert_semi = true;

        let mut token = Token::INT;
        let mut digits = "_0123456789";
        let mut exp = "eE";

        if !preceding_dot {
            if matches!(self.peek(), Some('0')) {
                self.next();
                match self.peek() {
                    Some('b' | 'B') => {
                        digits = "_01";
                        exp = "";
                        self.next();
                    }
                    Some('o' | 'O') => {
                        digits = "_01234567";
                        exp = "";
                        self.next();
                    }
                    Some('x' | 'X') => {
                        digits = "_0123456789abcdefABCDEF";
                        exp = "pP";
                        self.next();
                    }
                    _ => {}
                };
            }

            while let Some(c) = self.peek() {
                if !digits.contains(c) {
                    break;
                }
                self.next();
            }
        }

        if preceding_dot || matches!(self.peek(), Some('.')) {
            token = Token::FLOAT;
            self.next();
            while let Some(c) = self.peek() {
                if !digits.contains(c) {
                    break;
                }
                self.next();
            }
        }

        if !exp.is_empty() {
            if let Some(c) = self.peek() {
                if exp.contains(c) {
                    token = Token::FLOAT;
                    self.next();
                    if matches!(self.peek(), Some('-' | '+')) {
                        self.next();
                    }
                    while let Some(c) = self.peek() {
                        if !"_0123456789".contains(c) {
                            break;
                        }
                        self.next();
                    }
                }
            }
        }

        if matches!(self.peek(), Some('i')) {
            token = Token::IMAG;
            self.next();
        }

        Ok((self.position(), token, self.literal()))
    }

    // https://golang.org/ref/spec#Rune_literals
    fn scan_rune(&mut self) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.insert_semi = true;
        self.next();

        match self.peek() {
            Some('\\') => self.require_escaped_char('\'')?,
            Some(_) => self.next(),
            _ => return Err(ScannerError::UnterminatedRune),
        }

        if matches!(self.peek(), Some('\'')) {
            self.next();
            return Ok((self.position(), Token::CHAR, self.literal()));
        }

        Err(ScannerError::UnterminatedRune)
    }

    // https://golang.org/ref/spec#String_literals
    fn scan_interpreted_string(&mut self) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '"' => {
                    self.next();
                    return Ok((self.position(), Token::STRING, self.literal()));
                }
                '\\' => self.require_escaped_char('"')?,
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedString)
    }

    // https://golang.org/ref/spec#String_literals
    fn scan_raw_string(&mut self) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '`' => {
                    self.next();
                    return Ok((self.position(), Token::STRING, self.literal()));
                }
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedString)
    }

    // https://golang.org/ref/spec#Comments
    fn scan_general_comment(&mut self) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '*' => {
                    self.next();
                    if matches!(self.peek(), Some('/')) {
                        self.next();

                        let pos = self.position();
                        let lit = self.literal();

                        // look for compiler directives
                        self.directive(&lit["/*".len()..lit.len() - "*/".len()], true);

                        return Ok((pos, Token::COMMENT, lit));
                    }
                }
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedComment)
    }

    // https://golang.org/ref/spec#Comments
    fn scan_line_comment(&mut self) -> Result<(Position<'a>, Token, &'a str), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.next();
        }

        let pos = self.position();
        let lit = self.literal();

        // look for compiler directives (at the beginning of line)
        if self.start_column == 1 {
            self.directive(lit["//".len()..].trim_end(), false);
        }

        Ok((pos, Token::COMMENT, self.literal()))
    }

    // https://pkg.go.dev/cmd/compile#hdr-Compiler_Directives
    fn directive(&mut self, input: &'a str, immediate: bool) {
        if let Some(line_directive) = input.strip_prefix("line ") {
            self.pending_line_info = self.parse_line_directive(line_directive);
            if immediate {
                self.consume_pending_line_info();
            }
        }
    }

    fn parse_line_directive(
        &mut self,
        line_directive: &'a str,
    ) -> Option<(Option<&'a str>, usize, Option<usize>, bool)> {
        line_directive.rsplit_once(':').and_then(|(file, line)| {
            let line = line.parse().unwrap();

            if let Some((file, l)) = file.rsplit_once(':') {
                if let Ok(l) = l.parse() {
                    //line :line:col
                    //line filename:line:col
                    /*line :line:col*/
                    /*line filename:line:col*/
                    let file = if !file.is_empty() { Some(file) } else { None };
                    let col = Some(line);
                    let line = l;
                    let hide_column = false;
                    return Some((file, line, col, hide_column));
                }
            }

            //line :line
            //line filename:line
            /*line :line*/
            /*line filename:line*/
            Some((Some(file), line, None, true))
        })
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
        log::trace!("self.peek()");
        self.current_char
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
        }

        log::trace!(
            "self.next() offset={} line={} column={} current_char={:?}",
            self.offset,
            self.line,
            self.column,
            self.current_char,
        );
    }

    fn position(&self) -> Position<'a> {
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

    fn require_escaped_char(&mut self, delim: char) -> Result<(), ScannerError> {
        self.next();

        let c = self.peek().ok_or(ScannerError::UnterminatedEscapedChar)?;

        match c {
            'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | '\\' => self.next(),
            'x' => {
                self.next();
                self.require_hex_digits(2)?
            }
            'u' => {
                self.next();
                self.require_hex_digits(4)?;
            }
            'U' => {
                self.next();
                self.require_hex_digits(8)?;
            }
            '0'..='7' => self.require_octal_digits(3)?,
            _ => {
                // TODO: use const generics over &str when available and include in match above
                if c == delim {
                    self.next();
                } else {
                    return Err(ScannerError::UnterminatedEscapedChar);
                }
            }
        }

        Ok(())
    }

    fn require_octal_digits(&mut self, count: usize) -> Result<(), ScannerError> {
        for _ in 0..count {
            let c = self.peek().ok_or(ScannerError::OctalNotFound)?;

            if !matches!(c, '0'..='7') {
                return Err(ScannerError::OctalNotFound);
            }

            self.next();
        }

        Ok(())
    }

    fn require_hex_digits(&mut self, count: usize) -> Result<(), ScannerError> {
        for _ in 0..count {
            let c = self.peek().ok_or(ScannerError::HexadecimalNotFound)?;

            if !matches!(c, '0'..='9' | 'a'..='f' | 'A'..='F') {
                return Err(ScannerError::HexadecimalNotFound);
            }

            self.next();
        }

        Ok(())
    }
}

// https://golang.org/ref/spec#Letters_and_digits
pub fn is_letter(c: char) -> bool {
    matches!(c, '_' | 'A'..='Z' | 'a'..='z')
}

// https://golang.org/ref/spec#Characters
pub fn is_unicode_digit(c: char) -> bool {
    matches!(c, '0'..='9') // TODO: unicode
}

// https://golang.org/ref/spec#Keywords
static KEYWORDS: Map<&'static str, Token> = phf_map! {
  "break" => Token::BREAK,
  "case" => Token::CASE,
  "chan" => Token::CHAN,
  "const" => Token::CONST,
  "continue" => Token::CONTINUE,

  "default" => Token::DEFAULT,
  "defer" => Token::DEFER,
  "else" => Token::ELSE,
  "fallthrough" => Token::FALLTHROUGH,
  "for" => Token::FOR,

  "func" => Token::FUNC,
  "go" => Token::GO,
  "goto" => Token::GOTO,
  "if" => Token::IF,
  "import" => Token::IMPORT,

  "interface" => Token::INTERFACE,
  "map" => Token::MAP,
  "package" => Token::PACKAGE,
  "range" => Token::RANGE,
  "return" => Token::RETURN,

  "select" => Token::SELECT,
  "struct" => Token::STRUCT,
  "switch" => Token::SWITCH,
  "type" => Token::TYPE,
  "var" => Token::VAR,
};
