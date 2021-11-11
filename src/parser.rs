use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use scanner::{Scanner, ScannerError};
use std::fmt;

#[derive(Debug)]
pub enum ParserError {
    UnexpectedToken(Token),
    ScannerError(ScannerError),
}

impl std::error::Error for ParserError {}

impl From<ScannerError> for ParserError {
    fn from(e: ScannerError) -> Self {
        Self::ScannerError(e)
    }
}

pub type ParserResult<T> = Result<T, ParserError>;

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scanner error: {:?}", self)
    }
}

pub fn parse_file<'a>(filepath: &'a str, buffer: &'a str) -> ParserResult<ast::File<'a>> {
    let mut s = Scanner::new(filepath, buffer);
    parse_source_file(&mut s)
}

// https://golang.org/ref/spec#Source_file_organization
//
// SourceFile    = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
// PackageClause = "package" PackageName .
// PackageName   = identifier .
fn parse_source_file<'a>(s: &mut Scanner<'a>) -> ParserResult<ast::File<'a>> {
    let package = expect(s, Token::PACKAGE)?;
    let package_name = expect(s, Token::IDENT)?;
    expect(s, Token::SEMICOLON)?;

    Ok(ast::File {
        doc: None,
        package: package.0,
        name: Some(ast::Ident {
            name_pos: package_name.0,
            name: package_name.2,
            obj: None,
        }),
    })
}

fn expect<'a>(
    s: &mut Scanner<'a>,
    expected: Token,
) -> Result<(Position<'a>, Token, &'a str), ParserError> {
    let (pos, tok, lit) = s.scan()?;
    if tok != expected {
        return Err(ParserError::UnexpectedToken(tok));
    }
    Ok((pos, tok, lit))
}
