use crate::token::{Position, Token};
use phf::{phf_set, Set};
use std::fmt;

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

#[derive(Debug)]
pub enum ScannerError {
    UnexpectedToken(char),
    UnterminatedString,
}

impl std::error::Error for ScannerError {}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScannerError")
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
                    return Ok((self.position(), Token::PERIOD, String::from("")));
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
        if let Some(c) = self.peek() {
            if is_letter(c) {
                self.next();
                while let Some(c) = self.peek() {
                    if !(is_letter(c) || is_unicode_digit(c)) {
                        break;
                    }
                    self.next()
                }
            }
        }

        let (position, literal) = (self.position(), self.literal());
        if literal == "package" {
            Ok((position, Token::PACKAGE, literal))
        } else if KEYWORDS.contains(&literal) {
            if matches!(
                literal.as_str(),
                "break" | "continue" | "fallthrough" | "return"
            ) {
                self.asi = true
            }
            Ok((position, Token::KEYWORD(literal.to_owned()), literal))
        } else {
            self.asi = true;
            Ok((position, Token::IDENT(literal.to_owned()), literal))
        }
    }

    // https://golang.org/ref/spec#Integer_literals
    // https://golang.org/ref/spec#Floating-point_literals
    // https://golang.org/ref/spec#Imaginary_literals
    fn scan_int_or_float_or_imag(&mut self) -> Result<(Position, Token, String), ScannerError> {
        //self.asi = true
        unimplemented!("")
    }

    // https://golang.org/ref/spec#Rune_literals
    fn scan_rune(&mut self) -> Result<(Position, Token, String), ScannerError> {
        //self.asi = true
        unimplemented!("")
    }

    // https://golang.org/ref/spec#String_literals
    // TODO: add support for utf8 / multiline / escape
    fn scan_interpreted_string(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            self.next();
            if c == '"' {
                self.asi = true;
                let literal = self.literal();
                return Ok((
                    self.position(),
                    Token::STRING(literal.to_owned()),
                    literal.to_owned(),
                ));
            }
        }

        Err(ScannerError::UnterminatedString)
    }

    // https://golang.org/ref/spec#String_literals
    // TODO: add support for utf8 / multiline / escape
    fn scan_raw_string(&mut self) -> Result<(Position, Token, String), ScannerError> {
        self.next();

        while let Some(c) = self.peek() {
            self.next();
            if c == '`' {
                self.asi = true;
                let literal = self.literal();
                return Ok((
                    self.position(),
                    Token::STRING(literal.to_owned()),
                    literal.to_owned(),
                ));
            }
        }

        Err(ScannerError::UnterminatedString)
    }

    fn peek(&mut self) -> Option<char> {
        self.buffer.get(self.offset).map(|c| *c)
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
    match c {
        '_' | 'A'..='Z' | 'a'..='z' => true,
        _ => false,
    }
}

// https://golang.org/ref/spec#Characters
pub fn is_unicode_digit(c: char) -> bool {
    return c >= '0' && c <= '9'; // TODO: unicode
}

// https://golang.org/ref/spec#Keywords
static KEYWORDS: Set<&'static str> = phf_set! {
  "break",
  "default",
  "case",
  "chan",
  "const",
  "continue",
  "func",
  "defer",
  "else",
  "fallthrough",
  "for",
  "interface",
  "go",
  "goto",
  "if",
  "import",
  "select",
  "map",
  "range",
  "return",
  "struct",
  "switch",
  "type",
  "var",
};
