// https://golang.org/ref/spec#Lexical_elements

use crate::token::{Pos, Position, Token};
use phf::{phf_map, Map};
use std::fmt;

// TODO: match the errors from the Go scanner
#[derive(Debug)]
pub enum ScannerError {
    HexadecimalNotFound(Pos),
    InvalidInt(Pos),
    OctalNotFound(Pos),
    UnterminatedComment(Pos),
    UnterminatedEscapedChar(Pos),
    UnterminatedRune(Pos),
    UnterminatedString(Pos),
}

impl std::error::Error for ScannerError {}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scanner error: {:?}", self)
    }
}

#[derive(Debug)]
pub struct Scanner<'a> {
    filename: &'a str,
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
    freeze_column: bool,
    insert_semi: bool,
    pending: Option<(Pos, Token, &'a str)>,
}

impl<'a> Scanner<'a> {
    pub fn new(filename: &'a str, buffer: &'a str) -> Self {
        let mut s = Scanner {
            filename,
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
            start_line: 0,
            start_column: 0,
            //
            freeze_column: false,
            insert_semi: false,
            pending: None,
        };
        s.next(); // read the first character
        s
    }

    pub fn scan(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        let insert_semi = self.insert_semi;
        self.insert_semi = false;

        while let Some(c) = self.peek() {
            self.start_offset = self.offset;
            self.start_line = self.line;
            self.start_column = self.column;

            match c {
                ' ' | '\t' | '\r' => {
                    self.next();
                }

                '\n' => {
                    self.next();
                    if insert_semi {
                        if let Some(pending) = &self.pending {
                            if pending.1 == Token::COMMENT {
                                return Ok((pending.0, Token::SEMICOLON, "\n"));
                            }
                        }
                        return Ok((self.start_pos(), Token::SEMICOLON, "\n"));
                    }
                }

                _ => break,
            }
        }

        if let Some(pending) = self.pending.take() {
            return Ok(pending);
        }

        if let Some(c) = self.peek() {
            match c {
                '+' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::ADD_ASSIGN, ""));
                        }
                        Some('+') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.start_pos(), Token::INC, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::ADD, "")),
                    }
                }

                '-' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::SUB_ASSIGN, ""));
                        }
                        Some('-') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.start_pos(), Token::DEC, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::SUB, "")),
                    }
                }

                '*' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::MUL_ASSIGN, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::MUL, "")),
                    }
                }

                '/' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::QUO_ASSIGN, ""));
                        }
                        Some('/') => {
                            let out = self.scan_line_comment()?;
                            // "Any other comment acts like a newline."
                            if insert_semi {
                                self.pending = Some(out);
                                return Ok((self.start_pos(), Token::SEMICOLON, "\n"));
                            }
                            return Ok(out);
                        }
                        Some('*') => {
                            let out = self.scan_general_comment()?;
                            // "A general comment containing no newlines acts like a space."
                            if !out.2.contains('\n') {
                                self.pending = Some(out);
                                self.insert_semi = insert_semi;
                                return self.scan();
                            }
                            // "Any other comment acts like a newline."
                            if insert_semi {
                                self.pending = Some(out);
                                return Ok((self.start_pos(), Token::SEMICOLON, "\n"));
                            }
                            return Ok(out);
                        }
                        _ => return Ok((self.start_pos(), Token::QUO, "")),
                    }
                }

                '%' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::REM_ASSIGN, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::REM, "")),
                    }
                }

                '&' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::AND_ASSIGN, ""));
                        }
                        Some('&') => {
                            self.next();
                            return Ok((self.start_pos(), Token::LAND, ""));
                        }
                        Some('^') => {
                            self.next();
                            match self.peek() {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.start_pos(), Token::AND_NOT_ASSIGN, ""));
                                }
                                _ => return Ok((self.start_pos(), Token::AND_NOT, "")),
                            }
                        }
                        _ => return Ok((self.start_pos(), Token::AND, "")),
                    }
                }

                '|' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::OR_ASSIGN, ""));
                        }
                        Some('|') => {
                            self.next();
                            return Ok((self.start_pos(), Token::LOR, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::OR, "")),
                    }
                }

                '^' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::XOR_ASSIGN, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::XOR, "")),
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
                                    return Ok((self.start_pos(), Token::SHL_ASSIGN, ""));
                                }
                                _ => return Ok((self.start_pos(), Token::SHL, "")),
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::LEQ, ""));
                        }
                        Some('-') => {
                            self.next();
                            return Ok((self.start_pos(), Token::ARROW, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::LSS, "")),
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
                                    return Ok((self.start_pos(), Token::SHR_ASSIGN, ""));
                                }
                                _ => {
                                    return Ok((self.start_pos(), Token::SHR, ""));
                                }
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::GEQ, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::GTR, "")),
                    }
                }

                ':' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::DEFINE, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::COLON, "")),
                    }
                }

                '!' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::NEQ, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::NOT, "")),
                    }
                }

                ',' => {
                    self.next();
                    return Ok((self.start_pos(), Token::COMMA, ""));
                }

                '(' => {
                    self.next();
                    return Ok((self.start_pos(), Token::LPAREN, ""));
                }

                ')' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.start_pos(), Token::RPAREN, ""));
                }

                '[' => {
                    self.next();
                    return Ok((self.start_pos(), Token::LBRACK, ""));
                }

                ']' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.start_pos(), Token::RBRACK, ""));
                }

                '{' => {
                    self.next();
                    return Ok((self.start_pos(), Token::LBRACE, ""));
                }

                '}' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.start_pos(), Token::RBRACE, ""));
                }

                ';' => {
                    self.next();
                    return Ok((self.start_pos(), Token::SEMICOLON, ";"));
                }

                '.' => {
                    self.next();
                    match self.peek() {
                        Some('0'..='9') => return self.scan_int_or_float_or_imag(true),
                        Some('.') => {
                            self.next();
                            match self.peek() {
                                Some('.') => {
                                    self.next();
                                    return Ok((self.start_pos(), Token::ELLIPSIS, ""));
                                }
                                _ => return Ok((self.start_pos(), Token::ILLEGAL, self.literal())),
                            }
                        }
                        _ => return Ok((self.start_pos(), Token::PERIOD, "")),
                    }
                }

                '=' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.start_pos(), Token::EQL, ""));
                        }
                        _ => return Ok((self.start_pos(), Token::ASSIGN, "")),
                    }
                }

                '0'..='9' => return self.scan_int_or_float_or_imag(false),
                '\'' => return self.scan_rune(),
                '"' => return self.scan_interpreted_string(),
                '`' => return self.scan_raw_string(),
                _ => return self.scan_pkg_or_keyword_or_ident(),
            };
        }

        if insert_semi {
            Ok((self.pos(), Token::SEMICOLON, "\n"))
        } else {
            Ok((self.pos(), Token::EOF, ""))
        }
    }

    // https://golang.org/ref/spec#Keywords
    // https://golang.org/ref/spec#Identifiers
    fn scan_pkg_or_keyword_or_ident(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !(is_letter(c) || is_unicode_digit(c)) {
                break;
            }
            self.next()
        }

        let pos = self.start_pos();
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
    ) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.insert_semi = true;

        let mut token = if preceding_dot {
            Token::FLOAT
        } else {
            Token::INT
        };
        let (mut digits, mut exp) = ("_0123456789", "eE");

        if !preceding_dot && matches!(self.peek(), Some('0')) {
            self.next();
            let (d, e) = match self.peek() {
                Some('b' | 'B') => ("_01", ""),
                Some('o' | 'O') => ("_01234567", ""),
                Some('x' | 'X') => ("_0123456789abcdefABCDEF", "pP"),
                Some('0'..='9' | '_') => ("_0123456789", "eE"),
                Some('.') => {
                    token = Token::FLOAT;
                    ("_0123456789", "eE")
                }
                Some('i') => {
                    self.next();
                    return Ok((self.start_pos(), Token::IMAG, self.literal()));
                }
                _ => return Ok((self.start_pos(), token, self.literal())),
            };
            digits = d;
            exp = e;
            self.next();
        }

        while let Some(c) = self.peek() {
            if !digits.contains(c) {
                break;
            }
            self.next();
        }

        if matches!(self.peek(), Some('.')) {
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

        Ok((self.start_pos(), token, self.literal()))
    }

    // https://golang.org/ref/spec#Rune_literals
    fn scan_rune(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.insert_semi = true;
        self.next();

        match self.peek() {
            Some('\\') => self.require_escaped_char('\'')?,
            Some(_) => self.next(),
            _ => return Err(ScannerError::UnterminatedRune(self.start_pos())),
        }

        if matches!(self.peek(), Some('\'')) {
            self.next();
            return Ok((self.start_pos(), Token::CHAR, self.literal()));
        }

        Err(ScannerError::UnterminatedRune(self.start_pos()))
    }

    // https://golang.org/ref/spec#String_literals
    fn scan_interpreted_string(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '"' => {
                    self.next();
                    return Ok((self.start_pos(), Token::STRING, self.literal()));
                }
                '\\' => self.require_escaped_char('"')?,
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedString(self.start_pos()))
    }

    // https://golang.org/ref/spec#String_literals
    fn scan_raw_string(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '`' => {
                    self.next();
                    return Ok((self.start_pos(), Token::STRING, self.literal()));
                }
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedString(self.start_pos()))
    }

    // https://golang.org/ref/spec#Comments
    fn scan_general_comment(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '*' => {
                    self.next();
                    if matches!(self.peek(), Some('/')) {
                        self.next();
                        return Ok((self.start_pos(), Token::COMMENT, self.literal()));
                    }
                }
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedComment(self.start_pos()))
    }

    // https://golang.org/ref/spec#Comments
    fn scan_line_comment(&mut self) -> Result<(Pos, Token, &'a str), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.next();
        }

        let pos = self.start_pos();
        let lit = self.literal();

        // look for compiler directives
        if let Some(line_directive) = lit.strip_prefix("//line ") {
            if let Some(i) = line_directive.find(':') {
                let line: usize = line_directive[i + 1..]
                    .trim_end()
                    .parse()
                    .map_err(|_| ScannerError::InvalidInt(self.start_pos()))?;
                let line = line - 1; // because the trailing newline is going to increase the line count
                self.line = line;
                self.start_line = line;
                self.freeze_column = true;
            }
        }

        Ok((pos, Token::COMMENT, self.literal()))
    }

    fn peek(&mut self) -> Option<char> {
        log::trace!("self.peek()");
        self.current_char
    }

    fn next(&mut self) {
        self.offset += self.current_char_len;
        if !self.freeze_column {
            self.column += self.current_char_len;
        }

        let last_char = self.current_char;
        self.current_char = self.chars.next();
        if let Some(c) = self.current_char {
            self.current_char_len = c.len_utf8();
            if matches!(last_char, Some('\n')) {
                self.line += 1;
                self.column = if self.freeze_column { 0 } else { 1 };
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

    fn pos(&self) -> Pos {
        Pos {
            offset: self.offset,
            line: self.line,
            column: self.column,
        }
    }

    fn start_pos(&self) -> Pos {
        Pos {
            offset: self.start_offset,
            line: self.start_line,
            column: self.start_column,
        }
    }

    pub fn position(&self, pos: &Pos) -> Position {
        Position {
            filename: self.filename,
            offset: pos.offset,
            line: pos.line,
            column: pos.column,
        }
    }

    fn literal(&self) -> &'a str {
        &self.buffer[self.start_offset..self.offset]
    }

    fn require_escaped_char(&mut self, delim: char) -> Result<(), ScannerError> {
        self.next();

        let c = self
            .peek()
            .ok_or_else(|| ScannerError::UnterminatedEscapedChar(self.start_pos()))?;

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
                    return Err(ScannerError::UnterminatedEscapedChar(self.start_pos()));
                }
            }
        }

        Ok(())
    }

    fn require_octal_digits(&mut self, count: usize) -> Result<(), ScannerError> {
        for _ in 0..count {
            let c = self
                .peek()
                .ok_or_else(|| ScannerError::OctalNotFound(self.start_pos()))?;

            if !matches!(c, '0'..='7') {
                return Err(ScannerError::OctalNotFound(self.start_pos()));
            }

            self.next();
        }

        Ok(())
    }

    fn require_hex_digits(&mut self, count: usize) -> Result<(), ScannerError> {
        for _ in 0..count {
            let c = self
                .peek()
                .ok_or_else(|| ScannerError::HexadecimalNotFound(self.start_pos()))?;

            if !matches!(c, '0'..='9' | 'a'..='f' | 'A'..='F') {
                return Err(ScannerError::HexadecimalNotFound(self.start_pos()));
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
