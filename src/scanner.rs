use crate::token::{Position, Token};
use phf::{phf_map, Map};
use std::fmt;

// TODO: switch to an iterator
// TODO: remove any allocation
pub fn scan(filename: &str, buffer: &str) -> Result<Vec<(Position, Token, String)>, ScannerError> {
    let mut s = Scanner::new(filename, buffer.chars().collect());
    let mut out = vec![];

    loop {
        let (pos, tok, lit) = s.scan()?;
        let stop = tok == Token::EOF;
        out.push((pos, tok, lit));
        if stop {
            break;
        }
    }

    Ok(out)
}

// TODO: match the errors from the Go scanner
#[derive(Debug)]
pub enum ScannerError {
    ForbiddenCharacter(char),
    UnexpectedToken(char),
    UnterminatedChar,
    UnterminatedComment,
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
    filename: &'a str,
    buffer: Vec<char>,
    //
    offset: usize,
    line: usize,
    column: usize,
    //
    start_offset: usize,
    start_line: usize,
    start_column: usize,
    asi: bool,
}

impl<'a> Scanner<'a> {
    fn new(filename: &'a str, buffer: Vec<char>) -> Self {
        Scanner {
            filename,
            buffer,
            //
            offset: 0,
            line: 1,
            column: 1,
            //
            start_offset: 0,
            start_line: 0,
            start_column: 0,
            asi: false,
        }
    }

    fn scan(&mut self) -> Result<(Position, Token, String), ScannerError> {
        let asi = self.asi;
        self.asi = false;

        while let Some(c) = self.peek() {
            self.start_offset = self.offset;
            self.start_line = self.line;
            self.start_column = self.column;

            match c {
                '\0' => return Err(ScannerError::ForbiddenCharacter(c)),

                ' ' | '\t' | '\r' => {
                    self.next();
                    continue;
                }

                '\n' => {
                    self.line += 1;
                    self.column = 0;
                    self.next();
                    if asi {
                        return Ok((self.position(), Token::SEMICOLON, self.literal()));
                    }
                    continue;
                }

                '+' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::ADD_ASSIGN, String::from("")));
                        }
                        Some('+') => {
                            self.asi = true;
                            self.next();
                            return Ok((self.position(), Token::INC, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::ADD, String::from("")));
                        }
                    }
                }

                '-' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::SUB_ASSIGN, String::from("")));
                        }
                        Some('-') => {
                            self.asi = true;
                            self.next();
                            return Ok((self.position(), Token::DEC, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::SUB, String::from("")));
                        }
                    }
                }

                '*' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::MUL_ASSIGN, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::MUL, String::from("")));
                        }
                    }
                }

                '/' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::QUO_ASSIGN, String::from("")));
                        }
                        Some('/') => return self.scan_line_comment(),
                        Some('*') => return self.scan_general_comment(),
                        _ => {
                            return Ok((self.position(), Token::QUO, String::from("")));
                        }
                    }
                }

                '%' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::REM_ASSIGN, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::REM, String::from("")));
                        }
                    }
                }

                '&' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::AND_ASSIGN, String::from("")));
                        }
                        Some('&') => {
                            self.next();
                            return Ok((self.position(), Token::LAND, String::from("")));
                        }
                        Some('^') => {
                            self.next();
                            match self.peek() {
                                Some('=') => {
                                    self.next();
                                    return Ok((
                                        self.position(),
                                        Token::AND_NOT_ASSIGN,
                                        String::from(""),
                                    ));
                                }
                                _ => {
                                    return Ok((self.position(), Token::AND_NOT, String::from("")));
                                }
                            }
                        }
                        _ => {
                            return Ok((self.position(), Token::AND, String::from("")));
                        }
                    }
                }

                '|' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::OR_ASSIGN, String::from("")));
                        }
                        Some('|') => {
                            self.next();
                            return Ok((self.position(), Token::LOR, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::OR, String::from("")));
                        }
                    }
                }

                '^' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::XOR_ASSIGN, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::XOR, String::from("")));
                        }
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
                                    return Ok((
                                        self.position(),
                                        Token::SHL_ASSIGN,
                                        String::from(""),
                                    ));
                                }
                                _ => {
                                    return Ok((self.position(), Token::SHL, String::from("")));
                                }
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::LEQ, String::from("")));
                        }
                        Some('-') => {
                            self.next();
                            return Ok((self.position(), Token::ARROW, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::LSS, String::from("")));
                        }
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
                                    return Ok((
                                        self.position(),
                                        Token::SHR_ASSIGN,
                                        String::from(""),
                                    ));
                                }
                                _ => {
                                    return Ok((self.position(), Token::SHR, String::from("")));
                                }
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::GEQ, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::GTR, String::from("")));
                        }
                    }
                }

                ':' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::DEFINE, String::from("")));
                        }
                        _ => return Ok((self.position(), Token::COLON, String::from(""))),
                    }
                }

                '!' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::NEQ, String::from("")));
                        }
                        _ => return Ok((self.position(), Token::NOT, String::from(""))),
                    }
                }

                ',' => {
                    self.next();
                    return Ok((self.position(), Token::COMMA, String::from("")));
                }

                '(' => {
                    self.next();
                    return Ok((self.position(), Token::LPAREN, String::from("")));
                }

                ')' => {
                    self.asi = true;
                    self.next();
                    return Ok((self.position(), Token::RPAREN, String::from("")));
                }

                '[' => {
                    self.next();
                    return Ok((self.position(), Token::LBRACK, String::from("")));
                }

                ']' => {
                    self.asi = true;
                    self.next();
                    return Ok((self.position(), Token::RBRACK, String::from("")));
                }

                '{' => {
                    self.next();
                    return Ok((self.position(), Token::LBRACE, String::from("")));
                }

                '}' => {
                    self.asi = true;
                    self.next();
                    return Ok((self.position(), Token::RBRACE, String::from("")));
                }

                ';' => {
                    self.next();
                    return Ok((self.position(), Token::SEMICOLON, self.literal()));
                }

                '.' => {
                    self.next();
                    match self.peek() {
                        Some('.') => {
                            self.next();
                            match self.peek() {
                                Some('.') => {
                                    self.next();
                                    return Ok((
                                        self.position(),
                                        Token::ELLIPSIS,
                                        String::from(""),
                                    ));
                                }
                                _ => return Ok((self.position(), Token::ILLEGAL, self.literal())),
                            }
                        }
                        _ => return Ok((self.position(), Token::PERIOD, String::from(""))),
                    }
                }

                '=' => {
                    self.next();
                    match self.peek() {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::EQL, String::from("")));
                        }
                        _ => {
                            return Ok((self.position(), Token::ASSIGN, String::from("")));
                        }
                    }
                }

                '_' | 'A'..='Z' | 'a'..='z' => return self.scan_pkg_or_keyword_or_ident(),
                '0'..='9' => return self.scan_int_or_float_or_imag(),
                '\'' => return self.scan_rune(),
                '"' => return self.scan_interpreted_string(),
                '`' => return self.scan_raw_string(),
                _ => return Err(ScannerError::UnexpectedToken(c)),
            };
        }

        self.start_offset += 1;
        self.start_column += 1;
        Ok((self.position(), Token::EOF, String::from("")))
    }

    // https://golang.org/ref/spec#Keywords
    // https://golang.org/ref/spec#Identifiers
    fn scan_pkg_or_keyword_or_ident(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !(is_letter(c) || is_unicode_digit(c)) {
                break;
            }
            self.next()
        }

        let position = self.position();
        let literal = self.literal();
        if let Some(&token) = KEYWORDS.get(&literal) {
            self.asi = matches!(
                token,
                Token::BREAK | Token::CONTINUE | Token::FALLTHROUGH | Token::RETURN
            );
            Ok((position, token, literal))
        } else {
            self.asi = true;
            Ok((position, Token::IDENT, literal))
        }
    }

    // https://golang.org/ref/spec#Integer_literals
    // https://golang.org/ref/spec#Floating-point_literals
    // https://golang.org/ref/spec#Imaginary_literals
    fn scan_int_or_float_or_imag(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.asi = true;

        match self.peek() {
            Some('0') => {
                self.next();
                match self.peek() {
                    Some('1'..='9') => self.scan_decimal(),
                    Some('b') => self.scan_binary(),
                    Some('o') => self.scan_octal(),
                    Some('x' | 'X') => self.scan_hexadecimal(),
                    _ => return Ok((self.position(), Token::INT, self.literal())),
                }
            }
            _ => self.scan_decimal(),
        }
    }

    fn scan_decimal(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='9') {
                break;
            }
            self.next();
        }

        Ok((self.position(), Token::INT, self.literal()))
    }

    fn scan_binary(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='1') {
                break;
            }
            self.next();
        }

        Ok((self.position(), Token::INT, self.literal()))
    }

    fn scan_octal(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='7') {
                break;
            }
            self.next();
        }

        Ok((self.position(), Token::INT, self.literal()))
    }

    fn scan_hexadecimal(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if !matches!(c, '0'..='9' | 'A'..='F' | 'a'..='f') {
                break;
            }
            self.next();
        }

        Ok((self.position(), Token::INT, self.literal()))
    }

    // https://golang.org/ref/spec#Rune_literals
    // TODO: add support for utf8 / escape
    fn scan_rune(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.asi = true;
        self.next();

        if self.peek().is_some() {
            self.next();
            if let Some(d) = self.peek() {
                if d == '\'' {
                    self.next();
                    return Ok((self.position(), Token::CHAR, self.literal()));
                }
            }
        };

        Err(ScannerError::UnterminatedChar)
    }

    // https://golang.org/ref/spec#String_literals
    // TODO: add support for utf8 / multiline / escape
    fn scan_interpreted_string(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.asi = true;
        self.next();

        let mut escaped = false;
        while let Some(c) = self.peek() {
            self.next();
            match c {
                '"' => {
                    if !escaped {
                        return Ok((self.position(), Token::STRING, self.literal()));
                    }
                }
                '\\' => escaped = !escaped,
                _ => escaped = false,
            }
        }

        Err(ScannerError::UnterminatedString)
    }

    // https://golang.org/ref/spec#String_literals
    // TODO: add support for utf8 / multiline / escape
    fn scan_raw_string(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.asi = true;
        self.next();

        while let Some(c) = self.peek() {
            self.next();
            if c == '`' {
                return Ok((self.position(), Token::STRING, self.literal()));
            }
        }

        Err(ScannerError::UnterminatedString)
    }

    // https://golang.org/ref/spec#Comments
    fn scan_general_comment(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '*' {
                self.next();

                if let Some(c) = self.peek() {
                    if c == '/' {
                        return Ok((self.position(), Token::COMMENT, self.literal()));
                    }
                }
            }
        }

        Err(ScannerError::UnterminatedComment)
    }

    // https://golang.org/ref/spec#Comments
    fn scan_line_comment(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.next();
        }

        Ok((self.position(), Token::COMMENT, self.literal()))
    }

    fn peek(&mut self) -> Option<char> {
        self.buffer.get(self.offset).copied()
    }

    fn next(&mut self) {
        self.offset += 1;
        self.column += 1;
    }

    fn position(&self) -> Position {
        Position {
            filename: self.filename.to_owned(),
            offset: self.start_offset,
            line: self.start_line,
            column: self.start_column,
        }
    }

    fn literal(&self) -> String {
        String::from_iter(self.buffer[self.start_offset..self.offset].iter())
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
