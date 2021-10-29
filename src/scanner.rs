use crate::token::{Position, Token};
use phf::{phf_set, Set};
use std::fmt;

pub fn scan(filename: &str, buffer: &str) -> Result<Vec<(Position, Token, String)>, ScannerError> {
    let mut s = Scanner::new(filename, buffer.chars().collect());
    let mut out = vec![];

    while let Some(token) = s.scan()? {
        out.push(token)
    }

    Ok(out)
}

#[derive(Debug)]
pub enum ScannerError {
    UnexpectedToken(char),
}

impl std::error::Error for ScannerError {}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScannerError")
    }
}

pub struct Scanner<'a> {
    filename: &'a str,
    buffer: Vec<char>,
    //
    start: usize,
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Scanner<'a> {
    fn new(filename: &'a str, buffer: Vec<char>) -> Self {
        Scanner {
            filename,
            buffer,
            //
            start: 0,
            pos: 0,
            line: 1,
            column: 0,
        }
    }

    fn scan(&mut self) -> Result<Option<(Position, Token, String)>, ScannerError> {
        while let Some(c) = self.peek() {
            self.start = self.pos;

            match c {
                ' ' | '\t' | '\r' => {
                    self.next();
                    continue;
                }

                '\n' => {
                    self.line += 1;
                    self.column = 0;
                    self.next();
                    continue;
                }

                '(' => {
                    self.next();
                    return Ok(Some(self.wrap(Token::LPAREN)));
                }

                ')' => {
                    self.next();
                    return Ok(Some(self.wrap(Token::RPAREN)));
                }

                '{' => {
                    self.next();
                    return Ok(Some(self.wrap(Token::LBRACE)));
                }

                '}' => {
                    self.next();
                    return Ok(Some(self.wrap(Token::RBRACE)));
                }

                '_' | 'A'..='Z' | 'a'..='z' => return self.scan_keyword_or_identifier(),

                '0'..='9' => return self.scan_int_or_float_or_imag(),

                _ => return Err(ScannerError::UnexpectedToken(c)),
            };
        }

        Ok(None)
    }

    // https://golang.org/ref/spec#Keywords
    // https://golang.org/ref/spec#Identifiers
    fn scan_keyword_or_identifier(
        &mut self,
    ) -> Result<Option<(Position, Token, String)>, ScannerError> {
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
            Ok(Some((position, Token::PACKAGE, literal)))
        } else if KEYWORDS.contains(&literal) {
            Ok(Some((position, Token::KEYWORD, literal)))
        } else {
            Ok(Some((position, Token::IDENT, literal)))
        }
    }

    // https://golang.org/ref/spec#Integer_literals
    // https://golang.org/ref/spec#Floating-point_literals
    // https://golang.org/ref/spec#Imaginary_literals
    fn scan_int_or_float_or_imag(
        &mut self,
    ) -> Result<Option<(Position, Token, String)>, ScannerError> {
        Ok(None)
    }

    fn peek(&mut self) -> Option<char> {
        self.buffer.get(self.pos).map(|c| *c)
    }

    fn next(&mut self) {
        self.pos += 1;
        self.column += 1;
    }

    fn wrap(&self, token: Token) -> (Position, Token, String) {
        (self.position(), token, self.literal())
    }

    fn position(&self) -> Position {
        Position {
            filename: self.filename.to_owned(),
            offset: self.start,
            line: self.line,
            column: self.column,
        }
    }

    fn literal(&self) -> String {
        String::from_iter(self.buffer[self.start..self.pos].iter())
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
