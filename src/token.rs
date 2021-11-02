// https://cs.opensource.google/go/go/+/refs/tags/go1.17.2:src/go/token/token.go

use serde::{Serialize, Serializer};

#[derive(Serialize)]
pub struct Position {
    #[serde(rename = "Filename")]
    pub filename: String,
    #[serde(rename = "Offset")]
    pub offset: usize,
    #[serde(rename = "Line")]
    pub line: usize,
    #[serde(rename = "Column")]
    pub column: usize,
}

#[derive(PartialEq)]
pub enum Token {
    IDENT(String),
    PACKAGE,
    KEYWORD(String),
    STRING(String),
    RUNE(char),

    LPAREN, // (
    LBRACK, // [
    LBRACE, // {

    RPAREN, // )
    RBRACK, // ]
    RBRACE, // }

    SEMICOLON, // ;
    PERIOD,    // .

    EOF,
}

// String returns the string corresponding to the token tok.
// For operators, delimiters, and keywords the string is the actual
// token character sequence (e.g., for the token ADD, the string is
// "+"). For all other tokens the string corresponds to the token
// constant name (e.g. for the token IDENT, the string is "IDENT").
impl Serialize for Token {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Token::IDENT(_) => "IDENT",
            Token::PACKAGE => "package",
            Token::KEYWORD(keyword) => keyword,
            Token::STRING(_) => "STRING",
            Token::RUNE(_) => "CHAR",

            Token::LPAREN => "(",
            Token::LBRACK => "[",
            Token::LBRACE => "{",

            Token::RPAREN => ")",
            Token::RBRACK => "]",
            Token::RBRACE => "}",

            Token::SEMICOLON => ";",
            Token::PERIOD => ".",

            Token::EOF => "EOF",
        })
    }
}
