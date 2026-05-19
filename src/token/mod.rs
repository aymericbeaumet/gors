//! Go token types and source positions.
//!
//! This module defines token types corresponding to Go lexical elements
//! and position tracking for source locations.
//!
//! Based on the [Go token package](https://cs.opensource.google/go/go/+/refs/tags/go1.17.2:src/go/token/token.go).

#![allow(non_camel_case_types)] // For consistency with the Go tokens

use serde::{Serialize, Serializer, ser::SerializeMap};
use std::fmt;

/// Source position within a file.
///
/// Tracks the location of a token or AST node in the source code,
/// including file path, byte offset, line number, and column number.
#[derive(Clone, Copy, Debug, Default)]
pub struct Position<'a> {
    pub directory: &'a str,
    pub file: &'a str,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl<'a> fmt::Display for Position<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let filename = if self.file.is_empty() {
            String::new()
        } else if self.file.starts_with('/') {
            self.file.to_string()
        } else {
            format!("{}/{}", self.directory, self.file)
        };

        if filename.is_empty() {
            write!(f, "{}", self.line)?;
        } else {
            write!(f, "{}:{}", filename, self.line)?;
        }

        if self.column != 0 {
            write!(f, ":{}", self.column)?;
        }

        Ok(())
    }
}

impl<'a> Serialize for Position<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(4))?;

        if self.file.is_empty() {
            map.serialize_entry("Filename", "")?;
        } else if self.file.starts_with('/') {
            map.serialize_entry("Filename", self.file)?;
        } else {
            // Allocation is required here to construct the full path string
            map.serialize_entry("Filename", &format!("{}/{}", self.directory, self.file))?;
        }
        map.serialize_entry("Offset", &self.offset)?;
        map.serialize_entry("Line", &self.line)?;
        map.serialize_entry("Column", &self.column)?;
        map.end()
    }
}

/// Go token types.
///
/// Represents all lexical tokens in the Go language, including:
/// - Literals (identifiers, numbers, strings)
/// - Operators and delimiters
/// - Keywords
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Token {
    /// End of file
    EOF,
    /// Comment (single-line or multi-line)
    COMMENT,

    // Literals
    /// Identifier (e.g., `main`)
    IDENT,
    /// Integer literal (e.g., `12345`)
    INT,
    /// Float literal (e.g., `123.45`)
    FLOAT,
    /// Imaginary literal (e.g., `123.45i`)
    IMAG,
    /// Character literal (e.g., `'a'`)
    CHAR,
    /// String literal (e.g., `"abc"`)
    STRING,

    ADD, // +
    SUB, // -
    MUL, // *
    QUO, // /
    REM, // %

    AND,     // &
    OR,      // |
    XOR,     // ^
    SHL,     // <<
    SHR,     // >>
    AND_NOT, // &^

    ADD_ASSIGN, // +=
    SUB_ASSIGN, // -=
    MUL_ASSIGN, // *=
    QUO_ASSIGN, // /=
    REM_ASSIGN, // %=

    AND_ASSIGN,     // &=
    OR_ASSIGN,      // |=
    XOR_ASSIGN,     // ^=
    SHL_ASSIGN,     // <<=
    SHR_ASSIGN,     // >>=
    AND_NOT_ASSIGN, // &^=

    LAND,  // &&
    LOR,   // ||
    ARROW, // <-
    INC,   // ++
    DEC,   // --

    EQL,    // ==
    LSS,    // <
    GTR,    // >
    ASSIGN, // =
    NOT,    // !

    NEQ,      // !=
    LEQ,      // <=
    GEQ,      // >=
    DEFINE,   // :=
    ELLIPSIS, // ...
    TILDE,    // ~ (Go 1.18+ generics: underlying type constraint)

    LPAREN, // (
    LBRACK, // [
    LBRACE, // {
    COMMA,  // ,
    PERIOD, // .

    RPAREN,    // )
    RBRACK,    // ]
    RBRACE,    // }
    SEMICOLON, // ;
    COLON,     // :

    BREAK,
    CASE,
    CHAN,
    CONST,
    CONTINUE,

    DEFAULT,
    DEFER,
    ELSE,
    FALLTHROUGH,
    FOR,

    FUNC,
    GO,
    GOTO,
    IF,
    IMPORT,

    INTERFACE,
    MAP,
    PACKAGE,
    RANGE,
    RETURN,

    SELECT,
    STRUCT,
    SWITCH,
    TYPE,
    VAR,
}

impl Token {
    pub const fn is_assign_op(&self) -> bool {
        use Token::*;
        matches!(
            self,
            ADD_ASSIGN
                | SUB_ASSIGN
                | MUL_ASSIGN
                | QUO_ASSIGN
                | REM_ASSIGN
                | AND_ASSIGN
                | OR_ASSIGN
                | XOR_ASSIGN
                | SHL_ASSIGN
                | SHR_ASSIGN
                | AND_NOT_ASSIGN
        )
    }

    // https://go.dev/ref/spec#Operator_precedence
    pub fn precedence(&self) -> u8 {
        use Token::*;
        match self {
            MUL | QUO | REM | SHL | SHR | AND | AND_NOT => 5,
            ADD | SUB | OR | XOR => 4,
            EQL | NEQ | LSS | LEQ | GTR | GEQ => 3,
            LAND => 2,
            LOR => 1,
            _ => unreachable!(
                "precedence() is only supported for binary operators, called with: {:?}",
                self
            ),
        }
    }

    pub const fn lowest_precedence() -> u8 {
        0
    }
}

impl From<&Token> for &'static str {
    fn from(token: &Token) -> Self {
        use Token::*;

        match token {
            EOF => "EOF",
            COMMENT => "COMMENT",

            IDENT => "IDENT",
            INT => "INT",
            FLOAT => "FLOAT",
            IMAG => "IMAG",
            CHAR => "CHAR",
            STRING => "STRING",

            ADD => "+",
            SUB => "-",
            MUL => "*",
            QUO => "/",
            REM => "%",

            AND => "&",
            OR => "|",
            XOR => "^",
            SHL => "<<",
            SHR => ">>",
            AND_NOT => "&^",

            ADD_ASSIGN => "+=",
            SUB_ASSIGN => "-=",
            MUL_ASSIGN => "*=",
            QUO_ASSIGN => "/=",
            REM_ASSIGN => "%=",

            AND_ASSIGN => "&=",
            OR_ASSIGN => "|=",
            XOR_ASSIGN => "^=",
            SHL_ASSIGN => "<<=",
            SHR_ASSIGN => ">>=",
            AND_NOT_ASSIGN => "&^=",

            LAND => "&&",
            LOR => "||",
            ARROW => "<-",
            INC => "++",
            DEC => "--",

            EQL => "==",
            LSS => "<",
            GTR => ">",
            ASSIGN => "=",
            NOT => "!",

            NEQ => "!=",
            LEQ => "<=",
            GEQ => ">=",
            DEFINE => ":=",
            ELLIPSIS => "...",
            TILDE => "~",

            LPAREN => "(",
            LBRACK => "[",
            LBRACE => "{",
            COMMA => ",",
            PERIOD => ".",

            RPAREN => ")",
            RBRACK => "]",
            RBRACE => "}",
            SEMICOLON => ";",
            COLON => ":",

            BREAK => "break",
            CASE => "case",
            CHAN => "chan",
            CONST => "const",
            CONTINUE => "continue",

            DEFAULT => "default",
            DEFER => "defer",
            ELSE => "else",
            FALLTHROUGH => "fallthrough",
            FOR => "for",

            FUNC => "func",
            GO => "go",
            GOTO => "goto",
            IF => "if",
            IMPORT => "import",

            INTERFACE => "interface",
            MAP => "map",
            PACKAGE => "package",
            RANGE => "range",
            RETURN => "return",

            SELECT => "select",
            STRUCT => "struct",
            SWITCH => "switch",
            TYPE => "type",
            VAR => "var",
        }
    }
}

impl Serialize for Token {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.into())
    }
}
