// https://cs.opensource.google/go/go/+/refs/tags/go1.17.2:src/go/token/token.go

#![allow(clippy::upper_case_acronyms)]
#![allow(non_camel_case_types)]

use serde::{ser::SerializeMap, Serialize, Serializer};

#[derive(Clone, Copy, Debug)]
pub struct Position<'a> {
    pub directory: &'a str,
    pub file: &'a str,

    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl<'a> Serialize for Position<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("Filename", &[self.directory, "/", self.file].join(""))?; // TODO: remove alloc
        map.serialize_entry("Offset", &self.offset)?;
        map.serialize_entry("Line", &self.line)?;
        map.serialize_entry("Column", &self.column)?;
        map.end()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Token {
    ILLEGAL,

    EOF,
    COMMENT,

    IDENT,  // main
    INT,    // 12345
    FLOAT,  // 123.45
    IMAG,   // 123.45i
    CHAR,   // 'a'
    STRING, // "abc"

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
        use Token::*;

        serializer.serialize_str(match self {
            ILLEGAL => "ILLEGAL",

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
        })
    }
}
