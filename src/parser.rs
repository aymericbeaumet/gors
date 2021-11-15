use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use scanner::{Scanner, ScannerError};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug)]
pub enum ParserError<'a> {
    UnexpectedToken((Position<'a>, Token, &'a str)),
    UnexpectedEndOfFile,
    ScannerError(ScannerError),
}

impl<'a> std::error::Error for ParserError<'a> {}

impl<'a> From<ScannerError> for ParserError<'a> {
    fn from(e: ScannerError) -> Self {
        Self::ScannerError(e)
    }
}

pub type ParserResult<'a, T> = Result<T, ParserError<'a>>;

impl<'a> fmt::Display for ParserError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scanner error: {:?}", self)
    }
}

pub fn parse_file<'a>(filepath: &'a str, buffer: &'a str) -> ParserResult<'a, ast::File<'a>> {
    let s = Scanner::new(filepath, buffer);
    let mut p = Parser::new(s);
    p.next()?;
    p.source_file()
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

    // SourceFile    = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    // PackageClause = "package" PackageName .
    // PackageName   = identifier .
    fn source_file(&mut self) -> ParserResult<'a, ast::File<'a>> {
        let package = self.expect(Token::PACKAGE)?;
        self.next()?;

        let package_name = self.identifier()?;

        self.expect(Token::SEMICOLON)?;
        self.next()?;

        let imports = vec_until(|| match self.import_decl() {
            Ok(Some(out)) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(out))
            }
            out => out,
        })?;

        let decls = vec_until(|| match self.top_level_decl() {
            Ok(Some(out)) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(out))
            }
            out => out,
        })?;

        self.expect(Token::EOF)?;

        Ok(ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls,
            scope: Some(ast::Scope {
                outer: Box::new(None),
                objects: HashMap::new(),
            }),
            imports,
            unresolved: vec![],
            comments: vec![],
        })
    }

    fn identifier(&mut self) -> ParserResult<'a, ast::Ident<'a>> {
        let ident = self.expect(Token::IDENT)?;
        self.next()?;

        Ok(ast::Ident {
            name_pos: ident.0,
            name: ident.2,
            obj: None,
        })
    }

    // ImportDecl       = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    // ImportSpec       = [ "." | PackageName ] ImportPath .
    // ImportPath       = string_lit .
    fn import_decl(&mut self) -> ParserResult<'a, Option<ast::ImportSpec>> {
        Ok(None)
    }

    // TopLevelDecl  = Declaration | FunctionDecl | MethodDecl .
    fn top_level_decl(&mut self) -> ParserResult<'a, Option<ast::Decl<'a>>> {
        if let Some(func_decl) = self.function_decl()? {
            return Ok(Some(ast::Decl::FuncDecl(func_decl)));
        }

        Ok(None)
    }

    // FunctionDecl = "func" FunctionName Signature [ FunctionBody ] .
    // FunctionName = identifier .
    fn function_decl(&mut self) -> ParserResult<'a, Option<ast::FuncDecl<'a>>> {
        if self.expect(Token::FUNC).is_err() {
            return Ok(None);
        }
        self.next()?;

        let function_name = self.identifier()?;
        let signature = self.signature()?;

        self.expect(Token::LBRACE)?;
        self.next()?;

        self.expect(Token::RBRACE)?;
        self.next()?;

        Ok(Some(ast::FuncDecl {
            name: function_name,
            type_: signature,
        }))
    }

    // Signature    = Parameters [ Result ] .
    // Result       = Parameters | Type .
    fn signature(&mut self) -> ParserResult<'a, ast::FuncType> {
        Ok(ast::FuncType {
            params: self.parameters()?,
        })
    }

    // Parameters     = "(" [ ParameterList [ "," ] ] ")" .
    // ParameterList  = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl  = [ IdentifierList ] [ "..." ] Type .
    fn parameters(&mut self) -> ParserResult<'a, ast::FieldList> {
        self.expect(Token::LPAREN)?;
        self.next()?;

        self.expect(Token::RPAREN)?;
        self.next()?;

        Ok(ast::FieldList {})
    }

    // FunctionBody = Block .

    fn expect(&self, expected: Token) -> ParserResult<'a, (Position<'a>, Token, &'a str)> {
        if let Some(current) = self.current {
            if current.1 == expected {
                return Ok(current);
            }
            return Err(ParserError::UnexpectedToken(current));
        }
        Err(ParserError::UnexpectedEndOfFile)
    }

    fn next(&mut self) -> ParserResult<'a, ()> {
        self.current = Some(self.scanner.scan()?);
        Ok(())
    }
}

fn vec_until<'a, T>(
    mut func: impl FnMut() -> ParserResult<'a, Option<T>>,
) -> ParserResult<'a, Vec<T>> {
    let mut out = vec![];

    while let Some(v) = func()? {
        out.push(v);
    }

    Ok(out)
}
