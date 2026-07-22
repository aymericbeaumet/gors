use crate::scanner;
use crate::token::Token;
use std::fmt;

#[derive(Debug, Clone)]
pub enum ParserError {
    ScannerError(scanner::ScannerError),
    UnexpectedEndOfFile,
    UnexpectedToken,
    UnexpectedTokenAt {
        file: String,
        line: usize,
        column: usize,
        token: Token,
        literal: String,
    },
}

impl ParserError {
    /// Get a human-readable error message
    pub fn message(&self) -> String {
        match self {
            Self::ScannerError(e) => e.message().to_string(),
            Self::UnexpectedEndOfFile => "unexpected end of file".to_string(),
            Self::UnexpectedToken => "unexpected token".to_string(),
            Self::UnexpectedTokenAt { token, literal, .. } => {
                let token_str: &str = token.into();
                if literal.is_empty() {
                    format!("unexpected token '{}'", token_str)
                } else if token_str == literal {
                    format!("unexpected token '{}'", literal)
                } else {
                    format!("unexpected {} '{}'", token_str, literal)
                }
            }
        }
    }

    /// Get the location information if available
    pub fn location(&self) -> Option<(String, usize, usize)> {
        match self {
            Self::ScannerError(e) => Some((e.file.clone(), e.line, e.column)),
            Self::UnexpectedTokenAt {
                file, line, column, ..
            } => Some((file.clone(), *line, *column)),
            _ => None,
        }
    }
}

impl std::error::Error for ParserError {}

impl From<scanner::ScannerError> for ParserError {
    fn from(e: scanner::ScannerError) -> Self {
        Self::ScannerError(e)
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScannerError(e) => write!(f, "{}", e),
            Self::UnexpectedEndOfFile => write!(f, "syntax error: unexpected end of file"),
            Self::UnexpectedToken => write!(f, "syntax error: unexpected token"),
            Self::UnexpectedTokenAt {
                file,
                line,
                column,
                token,
                literal,
            } => {
                let loc = if file.is_empty() {
                    format!("{}:{}", line, column)
                } else {
                    format!("{}:{}:{}", file, line, column)
                };
                let token_str: &str = token.into();
                if literal.is_empty() {
                    write!(f, "{}: syntax error: unexpected token '{}'", loc, token_str)
                } else if token_str == literal {
                    write!(f, "{}: syntax error: unexpected token '{}'", loc, literal)
                } else {
                    write!(
                        f,
                        "{}: syntax error: unexpected {} '{}'",
                        loc, token_str, literal
                    )
                }
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, ParserError>;
