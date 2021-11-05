// https://golang.org/ref/spec#Lexical_elements

use crate::token::{Pos, Position, Token};
use phf::{phf_map, Map};
use std::fmt;

// TODO: match the errors from the Go scanner
#[derive(Debug)]
pub enum ScannerError {
    ForbiddenCharacter(Pos, char),
    UnexpectedToken(Pos, char),
    UnterminatedChar(Pos),
    UnterminatedComment(Pos),
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
    buffer: &'a [char],
    //
    index: usize,  // chars index in the vec
    offset: usize, // bytes index in the file
    line: usize,
    column: usize,
    //
    start_index: usize,
    start_offset: usize,
    start_line: usize,
    start_column: usize,
    //
    insert_semi: bool,
    pending: Option<(Pos, Token, String)>,
}

impl<'a> Scanner<'a> {
    pub fn new(filename: &'a str, buffer: &'a [char]) -> Self {
        Scanner {
            filename,
            buffer,
            //
            index: 0,
            offset: 0,
            line: 1,
            column: 1,
            //
            start_index: 0,
            start_offset: 0,
            start_line: 0,
            start_column: 0,
            //
            insert_semi: false,
            pending: None,
        }
    }

    pub fn scan(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        let insert_semi = self.insert_semi;
        self.insert_semi = false;

        if let Some(pending) = self.pending.take() {
            return Ok(pending);
        }

        while let Some(c) = self.peek() {
            self.start_index = self.index;
            self.start_offset = self.offset;
            self.start_line = self.line;
            self.start_column = self.column;

            match c {
                '\0' => return Err(ScannerError::ForbiddenCharacter(self.pos(), c)),

                ' ' | '\t' | '\r' => {
                    self.next();
                    continue;
                }

                '\n' => {
                    self.newline();
                    self.next();
                    if insert_semi {
                        return Ok((self.pos(), Token::SEMICOLON, self.literal()));
                    }
                    continue;
                }

                '+' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::ADD_ASSIGN, String::from("")));
                        }
                        Some('+') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.pos(), Token::INC, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::ADD, String::from(""))),
                    }
                }

                '-' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::SUB_ASSIGN, String::from("")));
                        }
                        Some('-') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.pos(), Token::DEC, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::SUB, String::from(""))),
                    }
                }

                '*' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::MUL_ASSIGN, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::MUL, String::from(""))),
                    }
                }

                '/' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::QUO_ASSIGN, String::from("")));
                        }
                        Some('/') => {
                            let out = self.scan_line_comment()?;
                            // "Any other comment acts like a newline."
                            if insert_semi {
                                self.pending = Some(out);
                                return Ok((self.pos(), Token::SEMICOLON, String::from("\n")));
                            }
                            return Ok(out);
                        }
                        Some('*') => {
                            let out = self.scan_general_comment()?;
                            // "A general comment containing no newlines acts like a space."
                            if !out.2.contains("\n") {
                                self.insert_semi = insert_semi;
                                return Ok(out);
                            }
                            // "Any other comment acts like a newline."
                            if insert_semi {
                                self.pending = Some(out);
                                return Ok((self.pos(), Token::SEMICOLON, String::from("\n")));
                            }
                            return Ok(out);
                        }
                        _ => return Ok((self.pos(), Token::QUO, String::from(""))),
                    }
                }

                '%' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::REM_ASSIGN, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::REM, String::from(""))),
                    }
                }

                '&' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::AND_ASSIGN, String::from("")));
                        }
                        Some('&') => {
                            self.next();
                            return Ok((self.pos(), Token::LAND, String::from("")));
                        }
                        Some('^') => {
                            self.next();
                            match self.peek() {
                                Some('=') => {
                                    self.next();
                                    return Ok((
                                        self.pos(),
                                        Token::AND_NOT_ASSIGN,
                                        String::from(""),
                                    ));
                                }
                                _ => return Ok((self.pos(), Token::AND_NOT, String::from(""))),
                            }
                        }
                        _ => return Ok((self.pos(), Token::AND, String::from(""))),
                    }
                }

                '|' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::OR_ASSIGN, String::from("")));
                        }
                        Some('|') => {
                            self.next();
                            return Ok((self.pos(), Token::LOR, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::OR, String::from(""))),
                    }
                }

                '^' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::XOR_ASSIGN, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::XOR, String::from(""))),
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
                                    return Ok((self.pos(), Token::SHL_ASSIGN, String::from("")));
                                }
                                _ => return Ok((self.pos(), Token::SHL, String::from(""))),
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::LEQ, String::from("")));
                        }
                        Some('-') => {
                            self.next();
                            return Ok((self.pos(), Token::ARROW, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::LSS, String::from(""))),
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
                                    return Ok((self.pos(), Token::SHR_ASSIGN, String::from("")));
                                }
                                _ => {
                                    return Ok((self.pos(), Token::SHR, String::from("")));
                                }
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::GEQ, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::GTR, String::from(""))),
                    }
                }

                ':' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::DEFINE, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::COLON, String::from(""))),
                    }
                }

                '!' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::NEQ, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::NOT, String::from(""))),
                    }
                }

                ',' => {
                    self.next();
                    return Ok((self.pos(), Token::COMMA, String::from("")));
                }

                '(' => {
                    self.next();
                    return Ok((self.pos(), Token::LPAREN, String::from("")));
                }

                ')' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.pos(), Token::RPAREN, String::from("")));
                }

                '[' => {
                    self.next();
                    return Ok((self.pos(), Token::LBRACK, String::from("")));
                }

                ']' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.pos(), Token::RBRACK, String::from("")));
                }

                '{' => {
                    self.next();
                    return Ok((self.pos(), Token::LBRACE, String::from("")));
                }

                '}' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.pos(), Token::RBRACE, String::from("")));
                }

                ';' => {
                    self.next();
                    return Ok((self.pos(), Token::SEMICOLON, self.literal()));
                }

                '.' => {
                    self.next();
                    match self.peek() {
                        Some('.') => {
                            self.next();
                            match self.peek() {
                                Some('.') => {
                                    self.next();
                                    return Ok((self.pos(), Token::ELLIPSIS, String::from("")));
                                }
                                _ => return Ok((self.pos(), Token::ILLEGAL, self.literal())),
                            }
                        }
                        _ => return Ok((self.pos(), Token::PERIOD, String::from(""))),
                    }
                }

                '=' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.pos(), Token::EQL, String::from("")));
                        }
                        _ => return Ok((self.pos(), Token::ASSIGN, String::from(""))),
                    }
                }

                '_' | 'A'..='Z' | 'a'..='z' => return self.scan_pkg_or_keyword_or_ident(),
                '0'..='9' => return self.scan_int_or_float_or_imag(),
                '\'' => return self.scan_rune(),
                '"' => return self.scan_interpreted_string(),
                '`' => return self.scan_raw_string(),
                _ => return Err(ScannerError::UnexpectedToken(self.pos(), c)),
            };
        }

        self.start_offset += 1;
        self.start_column += 1;
        Ok((self.pos(), Token::EOF, String::from("")))
    }

    // https://golang.org/ref/spec#Keywords
    // https://golang.org/ref/spec#Identifiers
    fn scan_pkg_or_keyword_or_ident(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !(is_letter(c) || is_unicode_digit(c)) {
                break;
            }
            self.next()
        }

        let pos = self.pos();
        let literal = self.literal();
        if let Some(&token) = KEYWORDS.get(&literal) {
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
    fn scan_int_or_float_or_imag(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.insert_semi = true;

        match self.peek() {
            Some('0') => {
                self.next();
                match self.peek() {
                    Some('1'..='9') => self.scan_decimal(),
                    Some('b' | 'B') => self.scan_binary(),
                    Some('o' | 'O') => self.scan_octal(),
                    Some('x' | 'X') => self.scan_hexadecimal(),
                    _ => Ok((self.pos(), Token::INT, self.literal())),
                }
            }
            _ => self.scan_decimal(),
        }
    }

    fn scan_decimal(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '.' {
                return self.scan_decimal_float();
            }
            if !matches!(c, '0'..='9' | '_') {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::INT, self.literal()))
    }

    fn scan_binary(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='1' | '_') {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::INT, self.literal()))
    }

    fn scan_octal(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='7' | '_') {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::INT, self.literal()))
    }

    fn scan_hexadecimal(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '.' {
                return self.scan_hexadecimal_float();
            }
            if !matches!(c, '0'..='9' | 'A'..='F' | 'a'..='f' | '_') {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::INT, self.literal()))
    }

    fn scan_decimal_float(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='9' | '_') {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::FLOAT, self.literal()))
    }

    fn scan_hexadecimal_float(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='9' | 'A'..='F' | 'a'..='f' | '_') {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::FLOAT, self.literal()))
    }

    // https://golang.org/ref/spec#Rune_literals
    fn scan_rune(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.insert_semi = true;
        self.next();

        if matches!(self.peek(), Some('\\')) {
            self.next();
            if matches!(self.peek(), Some('\'')) {
                self.next();
            }
        }

        while let Some(c) = self.peek() {
            self.next();
            if c == '\'' {
                return Ok((self.pos(), Token::CHAR, self.literal()));
            }
        }

        Err(ScannerError::UnterminatedChar(self.pos()))
    }

    // https://golang.org/ref/spec#String_literals
    fn scan_interpreted_string(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.insert_semi = true;
        self.next();

        let mut escaped = false;
        while let Some(c) = self.peek() {
            self.next();
            match c {
                '"' => {
                    if !escaped {
                        return Ok((self.pos(), Token::STRING, self.literal()));
                    }
                }
                '\\' => escaped = !escaped,
                _ => escaped = false,
            }
        }

        Err(ScannerError::UnterminatedString(self.pos()))
    }

    // https://golang.org/ref/spec#String_literals
    fn scan_raw_string(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '\n' => {
                    self.newline();
                    self.next();
                }
                '`' => {
                    self.next();
                    return Ok((self.pos(), Token::STRING, self.literal()));
                }
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedString(self.pos()))
    }

    // https://golang.org/ref/spec#Comments
    fn scan_general_comment(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            match c {
                '\n' => {
                    self.newline();
                    self.next();
                }
                '*' => {
                    self.next();
                    if matches!(self.peek(), Some('/')) {
                        self.next();
                        return Ok((self.pos(), Token::COMMENT, self.literal()));
                    }
                }
                _ => self.next(),
            }
        }

        Err(ScannerError::UnterminatedComment(self.pos()))
    }

    // https://golang.org/ref/spec#Comments
    fn scan_line_comment(&mut self) -> Result<(Pos, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.next();
        }

        Ok((self.pos(), Token::COMMENT, self.literal()))
    }

    fn peek(&mut self) -> Option<char> {
        self.buffer.get(self.index).copied()
    }

    fn next(&mut self) {
        let utf8_len = self.peek().map(|c| c.len_utf8()).unwrap_or(1);
        self.index += 1;
        self.offset += utf8_len;
        self.column += utf8_len;
    }

    fn pos(&self) -> Pos {
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

    fn literal(&self) -> String {
        String::from_iter(self.buffer[self.start_index..self.index].iter())
    }

    fn newline(&mut self) {
        self.line += 1;
        self.column = 0;
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
