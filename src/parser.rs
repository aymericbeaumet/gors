use crate::ast;
use crate::scanner;
use crate::token;
use std::fmt;

#[derive(Debug)]
pub enum ParserError {
    ScannerError,
}

impl std::error::Error for ParserError {}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scanner error: {:?}", self)
    }
}

pub fn parse_file(filepath: &str, buffer: &str) -> Result<ast::File, ParserError> {
    let mut s = scanner::Scanner::new(filepath, buffer);

    loop {
        let (_, tok, _) = s.scan().map_err(|_| ParserError::ScannerError)?;

        if tok == token::Token::EOF {
            break;
        }
    }

    Ok(ast::File {})
}
