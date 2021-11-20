use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use scanner::{Scanner, ScannerError};
use std::cell::Cell;
use std::collections::BTreeMap;
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
        write!(f, "parser error: {:?}", self)
    }
}

pub struct Arena(bumpalo::Bump);

impl Arena {
    pub fn new() -> Self {
        Self(bumpalo::Bump::new())
    }
}

pub fn parse_file<'a>(
    arena: &'a Arena,
    filepath: &'a str,
    buffer: &'a str,
) -> ParserResult<'a, &'a ast::File<'a>> {
    let s = Scanner::new(filepath, buffer);
    let mut p = Parser::new(arena, s);
    p.source_file()
}

struct Parser<'a> {
    arena: &'a Arena,
    scanner: Scanner<'a>,
    current: Option<(Position<'a>, Token, &'a str)>,
}

impl<'a> Parser<'a> {
    fn new(arena: &'a Arena, scanner: Scanner<'a>) -> Self {
        let mut p = Self {
            arena,
            scanner,
            current: None,
        };
        p.next().unwrap();
        p
    }

    #[inline(always)]
    fn alloc<T>(&self, val: T) -> &'a T {
        self.arena.0.alloc_with(|| val)
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

    /* Productions */

    // SourceFile    = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    // PackageClause = "package" PackageName .
    fn source_file(&mut self) -> ParserResult<'a, &'a ast::File<'a>> {
        let package = self.expect(Token::PACKAGE)?;
        self.next()?;

        let package_name = self.package_name()?;

        self.expect(Token::SEMICOLON)?;
        self.next()?;

        let mut import_decls = until(|| match self.import_decl() {
            Ok(Some(out)) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(ast::Decl::GenDecl(out)))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        })?;

        let mut top_level_decls = until(|| match self.top_level_decl() {
            Ok(Some(out)) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(out))
            }
            out => out,
        })?;

        self.expect(Token::EOF)?;

        let mut objects = BTreeMap::default();
        let mut idents = vec![];
        for decl in top_level_decls.iter() {
            match decl {
                ast::Decl::FuncDecl(func_decl) => {
                    if let Some(o) = func_decl.name.obj.get() {
                        objects.insert(func_decl.name.name, o);
                    }
                }
                ast::Decl::GenDecl(gen_decl) => {
                    for spec in gen_decl.specs.iter() {
                        if let ast::Spec::ValueSpec(value_spec) = spec {
                            for name in value_spec.names.iter() {
                                if let Some(o) = name.obj.get() {
                                    objects.insert(name.name, o);
                                }
                            }
                            if let Some(ast::Expr::Ident(ident)) = value_spec.type_ {
                                idents.push(ident);
                            }
                        }
                    }
                }
            }
        }

        let imports = import_decls
            .iter()
            .filter_map(|decl| {
                if let ast::Decl::GenDecl(decl) = decl {
                    if decl.tok == Token::IMPORT {
                        return Some(decl.specs.iter());
                    }
                }
                None
            })
            .flatten()
            .filter_map(|spec| {
                if let ast::Spec::ImportSpec(spec) = spec {
                    return Some(spec);
                }
                None
            })
            .copied()
            .collect();

        let mut decls = vec![];
        decls.append(&mut import_decls);
        decls.append(&mut top_level_decls);

        let unresolved = idents
            .into_iter()
            .filter(|ident| !objects.contains_key(ident.name))
            .collect();

        Ok(self.alloc(ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls,
            scope: Some(self.alloc(ast::Scope {
                outer: None,
                objects,
            })),
            imports,
            unresolved,
            comments: vec![],
        }))
    }

    // PackageName = identifier .
    fn package_name(&mut self) -> ParserResult<'a, &'a ast::Ident<'a>> {
        self.identifier()
    }

    fn identifier(&mut self) -> ParserResult<'a, &'a ast::Ident<'a>> {
        let ident = self.expect(Token::IDENT)?;
        self.next()?;

        Ok(self.alloc(ast::Ident {
            name_pos: ident.0,
            name: ident.2,
            obj: Cell::new(None),
        }))
    }

    // ImportDecl = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    fn import_decl(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        let import = self.expect(Token::IMPORT);
        if import.is_err() {
            return Ok(None);
        }
        let import = import.unwrap();
        self.next()?;

        let lparen = self.expect(Token::LPAREN);
        if lparen.is_err() {
            let specs = vec![ast::Spec::ImportSpec(self.import_spec()?)];
            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: import.0,
                tok: import.1,
                lparen: None,
                specs,
                rparen: None,
            })));
        }
        let lparen = lparen.unwrap();
        self.next()?;

        let specs = until(|| match self.import_spec() {
            Ok(out) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(ast::Spec::ImportSpec(out)))
            }
            _ => Ok(None),
        })?;

        let rparen = self.expect(Token::RPAREN)?;
        self.next()?;

        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: import.0,
            tok: import.1,
            lparen: Some(lparen.0),
            specs,
            rparen: Some(rparen.0),
        })))
    }

    // ImportSpec = [ "." | PackageName ] ImportPath .
    fn import_spec(&mut self) -> ParserResult<'a, &'a ast::ImportSpec<'a>> {
        let name = if let Ok(package_name) = self.package_name() {
            Some(package_name)
        } else if let Ok(period) = self.expect(Token::PERIOD) {
            self.next()?;
            Some(self.alloc(ast::Ident {
                name_pos: period.0,
                name: ".",
                obj: Cell::new(None),
            }))
        } else {
            None
        };

        let import_path = self.import_path()?;

        Ok(self.alloc(ast::ImportSpec {
            doc: None,
            name,
            path: import_path,
            comment: None,
        }))
    }

    // ImportPath = string_lit .
    fn import_path(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        self.string_lit()
    }

    fn string_lit(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        let out = self.expect(Token::STRING)?;
        self.next()?;
        Ok(self.alloc(ast::BasicLit {
            value_pos: out.0,
            kind: out.1,
            value: out.2,
        }))
    }

    // TopLevelDecl = Declaration | FunctionDecl | MethodDecl .
    fn top_level_decl(&mut self) -> ParserResult<'a, Option<ast::Decl<'a>>> {
        if let Some(declaration) = self.declaration()? {
            return Ok(Some(ast::Decl::GenDecl(declaration)));
        }
        if let Some(function_decl) = self.function_decl()? {
            return Ok(Some(ast::Decl::FuncDecl(function_decl)));
        }
        Ok(None)
    }

    // Declaration = ConstDecl | TypeDecl | VarDecl .
    fn declaration(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        if let Some(var_decl) = self.var_decl()? {
            return Ok(Some(var_decl));
        }
        Ok(None)
    }

    // VarDecl = "var" ( VarSpec | "(" { VarSpec ";" } ")" ) .
    fn var_decl(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        let var = self.expect(Token::VAR);
        if var.is_err() {
            return Ok(None);
        }
        let var = var.unwrap();
        self.next()?;

        let lparen = self.expect(Token::LPAREN);
        if lparen.is_err() {
            let specs = vec![ast::Spec::ValueSpec(self.var_spec()?)];
            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: var.0,
                tok: var.1,
                lparen: None,
                specs,
                rparen: None,
            })));
        }
        let lparen = lparen.unwrap();
        self.next()?;

        let specs = until(|| match self.var_spec() {
            Ok(out) => {
                self.expect(Token::SEMICOLON)?;
                self.next()?;
                Ok(Some(ast::Spec::ValueSpec(out)))
            }
            _ => Ok(None),
        })?;

        let rparen = self.expect(Token::RPAREN)?;
        self.next()?;

        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: var.0,
            tok: var.1,
            lparen: Some(lparen.0),
            specs,
            rparen: Some(rparen.0),
        })))
    }

    // VarSpec = IdentifierList ( Type [ "=" ExpressionList ] | "=" ExpressionList ) .
    fn var_spec(&mut self) -> ParserResult<'a, &'a ast::ValueSpec<'a>> {
        let names = self.identifier_list()?;

        let (type_, values) = if self.expect(Token::ASSIGN).is_ok() {
            self.next()?;
            (None, self.expression_list()?)
        } else {
            (
                Some(self.type_()?),
                if self.expect(Token::ASSIGN).is_ok() {
                    self.next()?;
                    self.expression_list()?
                } else {
                    vec![]
                },
            )
        };

        let out = self.alloc(ast::ValueSpec {
            doc: None,
            names,
            type_,
            values,
            comment: None,
        });

        for name in out.names.iter() {
            name.obj.set(Some(self.alloc(ast::Object {
                kind: ast::ObjKind::Var,
                name: name.name,
                decl: Some(ast::ObjDecl::ValueSpec(out)),
                data: Some(0),
                type_: None,
            })));
        }

        Ok(out)
    }

    // IdentifierList = identifier { "," identifier } .
    fn identifier_list(&mut self) -> ParserResult<'a, Vec<&'a ast::Ident<'a>>> {
        let mut out = vec![self.identifier()?];

        while self.expect(Token::COMMA).is_ok() {
            self.next()?;
            out.push(self.identifier()?);
        }

        Ok(out)
    }

    // ExpressionList = Expression { "," Expression } .
    fn expression_list(&mut self) -> ParserResult<'a, Vec<ast::Expr<'a>>> {
        let mut out = vec![self.expression()?];

        while self.expect(Token::COMMA).is_ok() {
            self.next()?;
            out.push(self.expression()?);
        }

        Ok(out)
    }

    fn expression(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        Ok(ast::Expr::BasicLit(self.basic_lit()?))
    }

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    fn basic_lit(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        let int_lit = self.expect(Token::INT)?;
        self.next()?;

        Ok(self.alloc(ast::BasicLit {
            kind: int_lit.1,
            value: int_lit.2,
            value_pos: int_lit.0,
        }))
    }

    // Type      = TypeName | TypeLit | "(" Type ")" .
    // TypeName  = identifier | QualifiedIdent .
    // TypeLit   = ArrayType | StructType | PointerType | FunctionType | InterfaceType |
    // SliceType | MapType | ChannelType .
    fn type_(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        Ok(ast::Expr::Ident(self.identifier()?))
    }

    // FunctionDecl = "func" FunctionName Signature [ FunctionBody ] .
    // FunctionName = identifier .
    // Signature    = Parameters [ Result ] .
    // Result       = Parameters | Type .
    fn function_decl(&mut self) -> ParserResult<'a, Option<&'a ast::FuncDecl<'a>>> {
        let func = self.expect(Token::FUNC);
        if func.is_err() {
            return Ok(None);
        }
        let func = func.unwrap();
        self.next()?;

        let function_name = self.identifier()?;

        let params = self.parameters()?;
        let signature = self.alloc(ast::FuncType {
            func: func.0,
            params,
        });

        let function_body = self.function_body()?;

        let out = self.alloc(ast::FuncDecl {
            doc: None,
            recv: None,
            name: function_name,
            type_: signature,
            body: function_body,
        });

        out.name.obj.set(Some(self.alloc(ast::Object {
            kind: ast::ObjKind::Fun,
            name: out.name.name,
            decl: Some(ast::ObjDecl::FuncDecl(out)),
            data: None,
            type_: None,
        })));

        Ok(Some(out))
    }

    // Parameters     = "(" [ ParameterList [ "," ] ] ")" .
    // ParameterList  = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl  = [ IdentifierList ] [ "..." ] Type .
    fn parameters(&mut self) -> ParserResult<'a, &'a ast::FieldList<'a>> {
        let lparen = self.expect(Token::LPAREN)?;
        self.next()?;

        let rparen = self.expect(Token::RPAREN)?;
        self.next()?;

        Ok(self.alloc(ast::FieldList {
            opening: lparen.0,
            list: vec![],
            closing: rparen.0,
        }))
    }

    // FunctionBody = Block .
    fn function_body(&mut self) -> ParserResult<'a, Option<&'a ast::BlockStmt<'a>>> {
        let lbrace = self.expect(Token::LBRACE)?;
        self.next()?;

        let rbrace = self.expect(Token::RBRACE)?;
        self.next()?;

        Ok(Some(self.alloc(ast::BlockStmt {
            lbrace: lbrace.0,
            list: vec![],
            rbrace: rbrace.0,
        })))
    }
}

fn until<'a, T>(mut func: impl FnMut() -> ParserResult<'a, Option<T>>) -> ParserResult<'a, Vec<T>> {
    let mut out = vec![];
    while let Some(v) = func()? {
        out.push(v);
    }
    Ok(out)
}
