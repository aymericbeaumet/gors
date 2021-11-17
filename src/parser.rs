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

pub type ParserArena = bumpalo::Bump;

pub fn new_arena() -> ParserArena {
    bumpalo::Bump::new()
}

pub fn parse_file<'a>(
    arena: &'a ParserArena,
    filepath: &'a str,
    buffer: &'a str,
) -> ParserResult<'a, &'a mut ast::File<'a>> {
    let s = Scanner::new(filepath, buffer);
    let mut p = Parser::new(arena, s);
    p.source_file()
}

struct Parser<'a> {
    arena: &'a ParserArena,
    scanner: Scanner<'a>,
    current: Option<(Position<'a>, Token, &'a str)>,
}

impl<'a> Parser<'a> {
    fn new(arena: &'a ParserArena, scanner: Scanner<'a>) -> Self {
        let mut p = Self {
            arena,
            scanner,
            current: None,
        };
        p.next().unwrap();
        p
    }

    // SourceFile    = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    // PackageClause = "package" PackageName .
    // PackageName   = identifier .
    fn source_file(&mut self) -> ParserResult<'a, &'a mut ast::File<'a>> {
        let package = self.expect(Token::PACKAGE)?;
        self.next()?;

        let package_name = self.identifier()?;

        self.expect(Token::SEMICOLON)?;
        self.next()?;

        let imports = zero_or_more(|| match self.import_decl() {
            Ok(Some(out)) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(out))
            }
            out => out,
        })?;

        let decls = zero_or_more(|| match self.top_level_decl() {
            Ok(Some(out)) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(out))
            }
            out => out,
        })?;

        self.expect(Token::EOF)?;

        Ok(self.arena.alloc_with(|| ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls,
            scope: Some(self.arena.alloc_with(|| ast::Scope {
                outer: None,
                objects: HashMap::new(),
            })),
            imports,
            unresolved: vec![],
            comments: vec![],
        }))
    }

    fn identifier(&mut self) -> ParserResult<'a, &'a mut ast::Ident<'a>> {
        let ident = self.expect(Token::IDENT)?;
        self.next()?;

        let out = self.arena.alloc_with(|| ast::Ident {
            name_pos: ident.0,
            name: ident.2,
            obj: None,
        });
        Ok(out)
    }

    // ImportDecl       = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    // ImportSpec       = [ "." | PackageName ] ImportPath .
    // ImportPath       = string_lit .
    fn import_decl(&mut self) -> ParserResult<'a, Option<&'a mut ast::ImportSpec>> {
        Ok(None)
    }

    // TopLevelDecl  = Declaration | FunctionDecl | MethodDecl .
    fn top_level_decl(&mut self) -> ParserResult<'a, Option<&'a mut ast::Decl<'a>>> {
        if let Some(func_decl) = self.function_decl()? {
            return Ok(Some(
                self.arena.alloc_with(|| ast::Decl::FuncDecl(func_decl)),
            ));
        }
        Ok(None)
    }

    // FunctionDecl = "func" FunctionName Signature [ FunctionBody ] .
    // FunctionName = identifier .
    // Signature    = Parameters [ Result ] .
    // Result       = Parameters | Type .
    fn function_decl(&mut self) -> ParserResult<'a, Option<&'a mut ast::FuncDecl<'a>>> {
        let func = self.expect(Token::FUNC);
        if func.is_err() {
            return Ok(None);
        }
        let func = func.unwrap();
        self.next()?;

        let function_name = self.identifier()?;

        let params = self.parameters()?;

        let signature = self.arena.alloc_with(|| ast::FuncType {
            func: func.0,
            params,
        });

        let function_body = self.function_body()?;

        Ok(Some(self.arena.alloc_with(|| ast::FuncDecl {
            doc: None,
            recv: None,
            name: function_name,
            type_: signature,
            body: function_body,
        })))
    }

    // Parameters     = "(" [ ParameterList [ "," ] ] ")" .
    // ParameterList  = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl  = [ IdentifierList ] [ "..." ] Type .
    fn parameters(&mut self) -> ParserResult<'a, &'a mut ast::FieldList<'a>> {
        let lparen = self.expect(Token::LPAREN)?;
        self.next()?;

        let rparen = self.expect(Token::RPAREN)?;
        self.next()?;

        Ok(self.arena.alloc_with(|| ast::FieldList {
            opening: lparen.0,
            list: vec![],
            closing: rparen.0,
        }))
    }

    // FunctionBody = Block .
    fn function_body(&mut self) -> ParserResult<'a, Option<&'a mut ast::BlockStmt<'a>>> {
        let lbrace = self.expect(Token::LBRACE)?;
        self.next()?;

        let rbrace = self.expect(Token::RBRACE)?;
        self.next()?;

        Ok(Some(self.arena.alloc_with(|| ast::BlockStmt {
            lbrace: lbrace.0,
            list: vec![],
            rbrace: rbrace.0,
        })))
    }

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

fn zero_or_more<'a, T>(
    mut func: impl FnMut() -> ParserResult<'a, Option<T>>,
) -> ParserResult<'a, Vec<T>> {
    let mut out = vec![];

    while let Some(v) = func()? {
        out.push(v);
    }

    Ok(out)
}
