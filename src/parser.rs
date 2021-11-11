use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use scanner::{Scanner, ScannerError};
use std::fmt;

#[derive(Debug)]
pub enum ParserError {
    UnexpectedToken,
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
    let mut p = Parser::new(s);
    p.parse_file()
}

struct Parser<'a> {
    scanner: Scanner<'a>,
    current: Option<(Position<'a>, Token, &'a str)>,
}

impl<'a> Parser<'a> {
    fn new(scanner: Scanner<'a>) -> Self {
        Self {
            scanner,
            current: None,
        }
    }

    // https://golang.org/ref/spec#Source_file_organization
    //
    // SourceFile    = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    // PackageClause = "package" PackageName .
    // PackageName   = identifier .
    fn parse_file(&mut self) -> ParserResult<ast::File<'a>> {
        self.next()?;

        let package = self.expect(Token::PACKAGE)?;
        self.next()?;

        let package_name = self.expect(Token::IDENT)?;
        self.next()?;

        self.expect(Token::SEMICOLON)?;

        Ok(ast::File {
            doc: None,
            package: package.0,
            name: ast::Ident {
                name_pos: package_name.0,
                name: package_name.2,
                obj: None,
            },
            decls: vec![],
        })
    }

    // https://golang.org/ref/spec#Import_declarations
    //
    // ImportDecl       = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    // ImportSpec       = [ "." | PackageName ] ImportPath .
    // ImportPath       = string_lit .
    fn parse_import_decls(s: &mut Scanner<'a>) -> ParserResult<Vec<ast::Decl<'a>>> {
        let mut out = vec![];

        Ok(out)
    }

    // https://golang.org/ref/spec#Declarations_and_scope
    // TopLevelDecl  = Declaration | FunctionDecl | MethodDecl .
    // Declaration   = ConstDecl | TypeDecl | VarDecl .
    fn parse_top_level_decls(s: &mut Scanner<'a>) -> ParserResult<Vec<ast::Decl<'a>>> {
        let mut out = vec![];

        Ok(out)
    }

    fn expect(&self, expected: Token) -> Result<(Position<'a>, Token, &'a str), ParserError> {
        if let Some(current) = self.current {
            if current.1 == expected {
                return Ok(current);
            }
        }
        return Err(ParserError::UnexpectedToken);
    }

    fn next(&mut self) -> Result<(), ParserError> {
        self.current = Some(self.scanner.scan()?);
        Ok(())
    }
}
