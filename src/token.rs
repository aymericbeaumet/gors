use serde::Serialize;

// https://cs.opensource.google/go/go/+/refs/tags/go1.17.2:src/go/token/token.go

#[derive(Serialize)]
pub enum Token {
    IDENT,
    PACKAGE,
    KEYWORD,

    LPAREN, // (
    RPAREN, // )
    LBRACE, // {
    RBRACE, // }
}

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
