#![allow(non_snake_case)]

use crate::ast::{self};
use crate::ast::{Visitable, Visitor};
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
    p.SourceFile()
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

    const fn get(&self) -> ParserResult<'a, (Position<'a>, Token, &'a str)> {
        if let Some(current) = self.current {
            return Ok(current);
        }
        Err(ParserError::UnexpectedEndOfFile)
    }

    fn expect(&self, expected: Token) -> ParserResult<'a, (Position<'a>, Token, &'a str)> {
        self.get().and_then(|current| {
            if current.1 == expected {
                Ok(current)
            } else {
                Err(ParserError::UnexpectedToken(current))
            }
        })
    }

    fn consume(&mut self, expected: Token) -> ParserResult<'a, (Position<'a>, Token, &'a str)> {
        self.expect(expected).and_then(|out| {
            self.next()?;
            Ok(out)
        })
    }

    fn next(&mut self) -> ParserResult<'a, ()> {
        self.current = Some(self.scanner.scan()?);
        Ok(())
    }

    /*
     * Non-terminal productions
     */

    // SourceFile    = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    fn SourceFile(&mut self) -> ParserResult<'a, &'a ast::File<'a>> {
        let (package, package_name) = self.PackageClause()?;
        self.consume(Token::SEMICOLON)?;

        let mut import_decls = repetition(|| match self.ImportDecl() {
            Ok(Some(out)) => {
                self.consume(Token::SEMICOLON)?;
                Ok(Some(ast::Decl::GenDecl(out)))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        })?;

        let mut top_level_decls = repetition(|| match self.TopLevelDecl() {
            Ok(Some(out)) => {
                self.consume(Token::SEMICOLON)?;
                Ok(Some(out))
            }
            out => out,
        })?;

        self.consume(Token::EOF)?;

        let mut ioc = IdentObjectCollector::default();
        top_level_decls.visit(&mut ioc);

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

        let unresolved = ioc
            .idents
            .into_iter()
            .filter(|ident| !ioc.objects.contains_key(ident.name))
            .collect();

        Ok(self.alloc(ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls,
            scope: Some(self.alloc(ast::Scope {
                outer: None,
                objects: ioc.objects,
            })),
            imports,
            unresolved,
            comments: vec![],
        }))
    }

    // PackageClause = "package" PackageName .
    fn PackageClause(
        &mut self,
    ) -> ParserResult<'a, ((Position<'a>, Token, &'a str), &'a ast::Ident<'a>)> {
        let package = self.consume(Token::PACKAGE)?;
        let package_name = self.PackageName()?;
        Ok((package, package_name))
    }

    // PackageName = identifier .
    fn PackageName(&mut self) -> ParserResult<'a, &'a ast::Ident<'a>> {
        self.identifier()
    }

    // ImportDecl = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    fn ImportDecl(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        let import = self.expect(Token::IMPORT);
        if import.is_err() {
            return Ok(None);
        }
        let import = import.unwrap();
        self.next()?;

        let lparen = self.expect(Token::LPAREN);
        if lparen.is_err() {
            let specs = vec![ast::Spec::ImportSpec(self.ImportSpec()?)];
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

        let specs = repetition(|| match self.ImportSpec() {
            Ok(out) => {
                self.consume(Token::SEMICOLON)?;
                Ok(Some(ast::Spec::ImportSpec(out)))
            }
            _ => Ok(None),
        })?;

        let rparen = self.consume(Token::RPAREN)?;

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
    fn ImportSpec(&mut self) -> ParserResult<'a, &'a ast::ImportSpec<'a>> {
        let name = if let Ok(package_name) = self.PackageName() {
            Some(package_name)
        } else if let Ok(period) = self.consume(Token::PERIOD) {
            Some(self.alloc(ast::Ident {
                name_pos: period.0,
                name: ".",
                obj: Cell::new(None),
            }))
        } else {
            None
        };

        let import_path = self.ImportPath()?;

        Ok(self.alloc(ast::ImportSpec {
            doc: None,
            name,
            path: import_path,
            comment: None,
        }))
    }

    // ImportPath = string_lit .
    fn ImportPath(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        self.string_lit()
    }

    // TopLevelDecl = Declaration | FunctionDecl | MethodDecl .
    fn TopLevelDecl(&mut self) -> ParserResult<'a, Option<ast::Decl<'a>>> {
        if let Some(declaration) = self.Declaration()? {
            return Ok(Some(ast::Decl::GenDecl(declaration)));
        }
        if let Some(function_decl) = self.FunctionDecl()? {
            return Ok(Some(ast::Decl::FuncDecl(function_decl)));
        }
        Ok(None)
    }

    // Declaration = ConstDecl | TypeDecl | VarDecl .
    fn Declaration(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        if let Some(decl) = self.ConstDecl()? {
            return Ok(Some(decl));
        }
        if let Some(decl) = self.VarDecl()? {
            return Ok(Some(decl));
        }
        Ok(None)
    }

    // ConstDecl = "const" ( ConstSpec | "(" { ConstSpec ";" } ")" ) .
    fn ConstDecl(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        let const_ = self.expect(Token::CONST);
        if const_.is_err() {
            return Ok(None);
        }
        let const_ = const_.unwrap();
        self.next()?;

        let lparen = self.expect(Token::LPAREN);
        if lparen.is_err() {
            let specs = vec![ast::Spec::ValueSpec(self.ConstSpec()?)];
            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: const_.0,
                tok: const_.1,
                lparen: None,
                specs,
                rparen: None,
            })));
        }
        let lparen = lparen.unwrap();
        self.next()?;

        let specs = repetition(|| match self.ConstSpec() {
            Ok(out) => {
                self.consume(Token::SEMICOLON)?;
                Ok(Some(ast::Spec::ValueSpec(out)))
            }
            _ => Ok(None),
        })?;

        let rparen = self.consume(Token::RPAREN)?;

        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: const_.0,
            tok: const_.1,
            lparen: Some(lparen.0),
            specs,
            rparen: Some(rparen.0),
        })))
    }

    // ConstSpec = IdentifierList [ [ Type ] "=" ExpressionList ] .
    fn ConstSpec(&mut self) -> ParserResult<'a, &'a ast::ValueSpec<'a>> {
        let names = self.IdentifierList()?;

        let (type_, values) = if self.consume(Token::ASSIGN).is_ok() {
            (None, self.ExpressionList()?)
        } else {
            (
                Some(self.Type()?),
                if self.consume(Token::ASSIGN).is_ok() {
                    self.ExpressionList()?
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
                kind: ast::ObjKind::Con,
                name: name.name,
                decl: Some(ast::ObjDecl::ValueSpec(out)),
                data: Some(0),
                type_: None,
            })));
        }

        Ok(out)
    }

    // VarDecl = "var" ( VarSpec | "(" { VarSpec ";" } ")" ) .
    fn VarDecl(&mut self) -> ParserResult<'a, Option<&'a ast::GenDecl<'a>>> {
        let var = self.expect(Token::VAR);
        if var.is_err() {
            return Ok(None);
        }
        let var = var.unwrap();
        self.next()?;

        let lparen = self.expect(Token::LPAREN);
        if lparen.is_err() {
            let specs = vec![ast::Spec::ValueSpec(self.VarSpec()?)];
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

        let specs = repetition(|| match self.VarSpec() {
            Ok(out) => {
                self.consume(Token::SEMICOLON)?;
                Ok(Some(ast::Spec::ValueSpec(out)))
            }
            _ => Ok(None),
        })?;

        let rparen = self.consume(Token::RPAREN)?;

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
    fn VarSpec(&mut self) -> ParserResult<'a, &'a ast::ValueSpec<'a>> {
        let names = self.IdentifierList()?;

        let (type_, values) = if self.consume(Token::ASSIGN).is_ok() {
            (None, self.ExpressionList()?)
        } else {
            (
                Some(self.Type()?),
                if self.consume(Token::ASSIGN).is_ok() {
                    self.ExpressionList()?
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
    fn IdentifierList(&mut self) -> ParserResult<'a, Vec<&'a ast::Ident<'a>>> {
        let mut out = vec![self.identifier()?];
        while self.consume(Token::COMMA).is_ok() {
            out.push(self.identifier()?);
        }
        Ok(out)
    }

    // ExpressionList = Expression { "," Expression } .
    fn ExpressionList(&mut self) -> ParserResult<'a, Vec<ast::Expr<'a>>> {
        let mut out = vec![self.Expression()?];
        while self.consume(Token::COMMA).is_ok() {
            out.push(self.Expression()?);
        }
        Ok(out)
    }

    // Expression = UnaryExpr | Expression binary_op Expression .
    fn Expression(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        self.UnaryExpr()
    }

    // UnaryExpr = PrimaryExpr | unary_op UnaryExpr .
    fn UnaryExpr(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        self.PrimaryExpr()
    }

    // PrimaryExpr =
    //         Operand |
    //         Conversion |
    //         MethodExpr |
    //         PrimaryExpr Selector |
    //         PrimaryExpr Index |
    //         PrimaryExpr Slice |
    //         PrimaryExpr TypeAssertion |
    //         PrimaryExpr Arguments .
    fn PrimaryExpr(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        self.Operand()
    }

    // Operand = Literal | OperandName | "(" Expression ")" .
    fn Operand(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        let lit = self.Literal();
        if lit.is_ok() {
            return lit;
        }
        self.OperandName()
    }

    // Literal = BasicLit | CompositeLit | FunctionLit .
    fn Literal(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        Ok(ast::Expr::BasicLit(self.BasicLit()?))
    }

    // OperandName = identifier | QualifiedIdent .
    fn OperandName(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        Ok(ast::Expr::Ident(self.identifier()?))
    }

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    fn BasicLit(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        self.int_lit()
    }

    // Type      = TypeName | TypeLit | "(" Type ")" .
    // TypeName  = identifier | QualifiedIdent .
    // TypeLit   = ArrayType | StructType | PointerType | FunctionType | InterfaceType |
    // SliceType | MapType | ChannelType .
    fn Type(&mut self) -> ParserResult<'a, ast::Expr<'a>> {
        Ok(ast::Expr::Ident(self.identifier()?))
    }

    // FunctionDecl = "func" FunctionName Signature [ FunctionBody ] .
    // Signature    = Parameters [ Result ] .
    // Result       = Parameters | Type .
    fn FunctionDecl(&mut self) -> ParserResult<'a, Option<&'a ast::FuncDecl<'a>>> {
        let func = self.expect(Token::FUNC);
        if func.is_err() {
            return Ok(None);
        }
        let func = func.unwrap();
        self.next()?;

        let function_name = self.FunctionName()?;

        let params = self.Parameters()?;
        let signature = self.alloc(ast::FuncType {
            func: func.0,
            params,
        });

        let function_body = self.FunctionBody()?;

        let out = self.alloc(ast::FuncDecl {
            doc: None,
            recv: None,
            name: function_name,
            type_: signature,
            body: Some(function_body),
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

    // FunctionName = identifier .
    fn FunctionName(&mut self) -> ParserResult<'a, &'a ast::Ident<'a>> {
        self.identifier()
    }

    // Parameters     = "(" [ ParameterList [ "," ] ] ")" .
    // ParameterList  = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl  = [ IdentifierList ] [ "..." ] Type .
    fn Parameters(&mut self) -> ParserResult<'a, &'a ast::FieldList<'a>> {
        let lparen = self.consume(Token::LPAREN)?;
        let rparen = self.consume(Token::RPAREN)?;
        Ok(self.alloc(ast::FieldList {
            opening: lparen.0,
            list: vec![],
            closing: rparen.0,
        }))
    }

    // FunctionBody = Block .
    fn FunctionBody(&mut self) -> ParserResult<'a, &'a ast::BlockStmt<'a>> {
        self.Block()
    }

    // Block = "{" StatementList "}" .
    fn Block(&mut self) -> ParserResult<'a, &'a ast::BlockStmt<'a>> {
        let lbrace = self.consume(Token::LBRACE)?;
        let list = self.StatementList()?;
        let rbrace = self.consume(Token::RBRACE)?;
        Ok(self.alloc(ast::BlockStmt {
            lbrace: lbrace.0,
            list,
            rbrace: rbrace.0,
        }))
    }

    // StatementList = { Statement ";" } .
    fn StatementList(&mut self) -> ParserResult<'a, Vec<ast::Stmt<'a>>> {
        repetition(|| match self.Statement() {
            Ok(out) => {
                self.consume(Token::SEMICOLON)?;
                Ok(Some(out))
            }
            _ => Ok(None),
        })
    }

    // Statement =
    //         Declaration | LabeledStmt | SimpleStmt |
    //         GoStmt | ReturnStmt | BreakStmt | ContinueStmt | GotoStmt |
    //         FallthroughStmt | Block | IfStmt | SwitchStmt | SelectStmt | ForStmt |
    //         DeferStmt .
    fn Statement(&mut self) -> ParserResult<'a, ast::Stmt<'a>> {
        self.SimpleStmt()
    }

    // SimpleStmt = EmptyStmt | ExpressionStmt | SendStmt | IncDecStmt | Assignment | ShortVarDecl .
    fn SimpleStmt(&mut self) -> ParserResult<'a, ast::Stmt<'a>> {
        Ok(ast::Stmt::AssignStmt(self.Assignment()?))
    }

    // Assignment = ExpressionList assign_op ExpressionList .
    fn Assignment(&mut self) -> ParserResult<'a, &'a ast::AssignStmt<'a>> {
        let lhs = self.ExpressionList()?;
        let assign_op = self.assign_op()?;
        let rhs = self.ExpressionList()?;
        Ok(self.alloc(ast::AssignStmt {
            lhs,
            tok_pos: assign_op.0,
            tok: assign_op.1,
            rhs,
        }))
    }

    /*
     * Lexical tokens
     */

    // assign_op = [ add_op | mul_op ] "=" .
    // add_op     = "+" | "-" | "|" | "^" .
    // mul_op     = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    fn assign_op(&mut self) -> ParserResult<'a, (Position<'a>, Token, &'a str)> {
        use Token::*;

        let out = self.get()?;
        match out.1 {
            /* add_op "=" */ ADD_ASSIGN | SUB_ASSIGN | OR_ASSIGN | XOR_ASSIGN |
            /* mul_op "=" */ MUL_ASSIGN | QUO_ASSIGN | REM_ASSIGN | SHL_ASSIGN | SHR_ASSIGN | AND_ASSIGN | AND_NOT_ASSIGN |
            /* "=" */ EQL => {
                self.next()?;
                Ok(out)
            }
            _ => Err(ParserError::UnexpectedToken(out)),
        }
    }

    fn identifier(&mut self) -> ParserResult<'a, &'a ast::Ident<'a>> {
        let ident = self.consume(Token::IDENT)?;
        Ok(self.alloc(ast::Ident {
            name_pos: ident.0,
            name: ident.2,
            obj: Cell::new(None),
        }))
    }

    fn int_lit(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        let int_lit = self.consume(Token::INT)?;
        Ok(self.alloc(ast::BasicLit {
            kind: int_lit.1,
            value: int_lit.2,
            value_pos: int_lit.0,
        }))
    }

    fn string_lit(&mut self) -> ParserResult<'a, &'a ast::BasicLit<'a>> {
        let out = self.consume(Token::STRING)?;
        Ok(self.alloc(ast::BasicLit {
            value_pos: out.0,
            kind: out.1,
            value: out.2,
        }))
    }
}

fn repetition<'a, T>(
    mut func: impl FnMut() -> ParserResult<'a, Option<T>>,
) -> ParserResult<'a, Vec<T>> {
    let mut out = vec![];
    while let Some(v) = func()? {
        out.push(v);
    }
    Ok(out)
}

#[derive(Default)]
struct IdentObjectCollector<'a> {
    idents: Vec<&'a ast::Ident<'a>>,
    objects: BTreeMap<&'a str, &'a ast::Object<'a>>,
}

impl<'a> Visitor<'a> for IdentObjectCollector<'a> {
    fn FuncDecl(&mut self, func_decl: &'a ast::FuncDecl<'a>) {
        if let Some(o) = func_decl.name.obj.get() {
            self.objects.insert(func_decl.name.name, o);
        }
    }

    fn Ident(&mut self, ident: &'a ast::Ident<'a>) {
        self.idents.push(ident);
    }

    fn ValueSpec(&mut self, value_spec: &'a ast::ValueSpec<'a>) {
        for name in value_spec.names.iter() {
            if let Some(o) = name.obj.get() {
                self.objects.insert(name.name, o);
            }
        }
    }
}
