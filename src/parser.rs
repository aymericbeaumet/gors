#![allow(non_snake_case)]

use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use scanner::{Scanner, ScannerError};
use std::cell::Cell;
use std::fmt;

#[derive(Debug)]
pub enum ParserError {
    ScannerError(ScannerError),
    UnexpectedEndOfFile,
    UnexpectedToken,
    UnexpectedTokenAt {
        at: String,
        token: Token,
        literal: String,
    },
}

impl<'a> std::error::Error for ParserError {}

impl<'a> From<ScannerError> for ParserError {
    fn from(e: ScannerError) -> Self {
        Self::ScannerError(e)
    }
}

impl<'a> fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parser error: {:?}", self)
    }
}

pub type ParserResult<T> = Result<T, ParserError>;

trait ParserResultExt<T> {
    fn required(self) -> ParserResult<T>;
}

impl<T> ParserResultExt<T> for ParserResult<Option<T>> {
    fn required(self) -> ParserResult<T> {
        match self {
            Ok(Some(node)) => Ok(node),
            Ok(None) => Err(ParserError::UnexpectedToken),
            Err(err) => Err(err),
        }
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
) -> ParserResult<&'a ast::File<'a>> {
    let s = Scanner::new(filepath, buffer);
    let mut p = Parser::new(arena, s);
    p.SourceFile()
        .required()
        .map_err(|err| match (err, p.current) {
            (ParserError::UnexpectedToken, Some(current)) => ParserError::UnexpectedTokenAt {
                at: current.0.to_string(),
                token: current.1,
                literal: current.2.to_owned(),
            },
            (err, _) => err,
        })
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

    fn next(&mut self) -> ParserResult<()> {
        self.current = Some(self.scanner.scan()?);
        log::debug!("self.current = {:?}", self.current);
        Ok(())
    }

    /*
     * Non-terminal productions
     */

    // SourceFile = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    fn SourceFile(&mut self) -> ParserResult<Option<&'a ast::File<'a>>> {
        log::debug!("Parser::SourceFile()");

        let (package, package_name) = match self.PackageClause()? {
            Some(v) => v,
            None => return Ok(None),
        };
        self.token(Token::SEMICOLON)?;

        let mut import_decls = zero_or_more(|| match self.ImportDecl() {
            Ok(Some(import_decl)) => {
                self.token(Token::SEMICOLON)?;
                Ok(Some(ast::Decl::GenDecl(import_decl)))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        })
        .required()?;

        let mut top_level_decls = zero_or_more(|| match self.TopLevelDecl() {
            Ok(Some(out)) => {
                self.token(Token::SEMICOLON)?;
                Ok(Some(out))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        })
        .required()?;

        self.token(Token::EOF)?;

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

        Ok(Some(self.alloc(ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls,
            scope: None,
            imports,
            unresolved: vec![],
            comments: vec![],
        })))
    }

    // PackageClause = "package" PackageName .
    fn PackageClause(
        &mut self,
    ) -> ParserResult<Option<((Position<'a>, Token, &'a str), &'a ast::Ident<'a>)>> {
        log::debug!("Parser::PackageClause()");

        let package = match self.token(Token::PACKAGE)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let package_name = self.PackageName().required()?;
        Ok(Some((package, package_name)))
    }

    // PackageName = identifier .
    fn PackageName(&mut self) -> ParserResult<Option<&'a ast::Ident<'a>>> {
        log::debug!("Parser::PackageName()");

        self.identifier()
    }

    // ImportDecl = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    fn ImportDecl(&mut self) -> ParserResult<Option<&'a ast::GenDecl<'a>>> {
        log::debug!("Parser::ImportDecl()");

        let import = match self.token(Token::IMPORT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let specs = zero_or_more(|| match self.ImportSpec() {
                Ok(Some(out)) => {
                    self.token(Token::SEMICOLON)?;
                    Ok(Some(ast::Spec::ImportSpec(out)))
                }
                _ => Ok(None),
            })
            .required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: import.0,
                tok: import.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            })));
        }

        let specs = vec![ast::Spec::ImportSpec(self.ImportSpec().required()?)];
        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: import.0,
            tok: import.1,
            lparen: None,
            specs,
            rparen: None,
        })))
    }

    // ImportSpec = [ "." | PackageName ] ImportPath .
    fn ImportSpec(&mut self) -> ParserResult<Option<&'a ast::ImportSpec<'a>>> {
        log::debug!("Parser::ImportSpec()");

        let name = if let Some(period) = self.token(Token::PERIOD)? {
            Some(self.alloc(ast::Ident {
                name_pos: period.0,
                name: ".",
                obj: Cell::new(None),
            }))
        } else {
            self.PackageName()?
        };

        let import_path = self.ImportPath().required()?;

        Ok(Some(self.alloc(ast::ImportSpec {
            doc: None,
            name,
            path: import_path,
            comment: None,
        })))
    }

    // ImportPath = string_lit .
    fn ImportPath(&mut self) -> ParserResult<Option<&'a ast::BasicLit<'a>>> {
        log::debug!("Parser::ImportPath()");

        self.string_lit()
    }

    // TopLevelDecl = Declaration | FunctionDecl | MethodDecl .
    fn TopLevelDecl(&mut self) -> ParserResult<Option<ast::Decl<'a>>> {
        log::debug!("Parser::TopLevelDecl()");

        if let Some(declaration) = self.Declaration()? {
            return Ok(Some(ast::Decl::GenDecl(declaration)));
        }
        if let Some(function_decl) = self.FunctionDecl()? {
            return Ok(Some(ast::Decl::FuncDecl(function_decl)));
        }
        Ok(None)
    }

    // Declaration = ConstDecl | TypeDecl | VarDecl .
    fn Declaration(&mut self) -> ParserResult<Option<&'a ast::GenDecl<'a>>> {
        log::debug!("Parser::Declaration()");

        if let Some(declaration) = self.ConstDecl()? {
            return Ok(Some(declaration));
        }
        if let Some(declaration) = self.TypeDecl()? {
            return Ok(Some(declaration));
        }
        if let Some(declaration) = self.VarDecl()? {
            return Ok(Some(declaration));
        }
        Ok(None)
    }

    // TypeDecl = "type" ( TypeSpec | "(" { TypeSpec ";" } ")" ) .
    fn TypeDecl(&mut self) -> ParserResult<Option<&'a ast::GenDecl<'a>>> {
        log::debug!("Parser::TypeDecl()");

        let type_ = match self.token(Token::TYPE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let specs = zero_or_more(|| match self.TypeSpec() {
                Ok(Some(out)) => {
                    self.token(Token::SEMICOLON)?;
                    Ok(Some(ast::Spec::TypeSpec(out)))
                }
                _ => Ok(None),
            })
            .required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: type_.0,
                tok: type_.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            })));
        }

        let specs = vec![ast::Spec::TypeSpec(self.TypeSpec().required()?)];
        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: type_.0,
            tok: type_.1,
            lparen: None,
            specs,
            rparen: None,
        })))
    }

    // TypeSpec  = AliasDecl | TypeDef .
    // AliasDecl = identifier "=" Type .
    // TypeDef   = identifier Type .
    fn TypeSpec(&mut self) -> ParserResult<Option<&'a ast::TypeSpec<'a>>> {
        log::debug!("Parser::TypeSpec()");

        let name = match self.identifier()? {
            Some(v) => v,
            None => return Ok(None),
        };
        let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
        let type_ = self.Type().required()?;

        Ok(Some(self.alloc(ast::TypeSpec {
            doc: None,
            name: Some(name),
            assign,
            type_,
            comment: None,
        })))
    }

    // ConstDecl = "const" ( ConstSpec | "(" { ConstSpec ";" } ")" ) .
    fn ConstDecl(&mut self) -> ParserResult<Option<&'a ast::GenDecl<'a>>> {
        log::debug!("Parser::ConstDecl()");

        let const_ = match self.token(Token::CONST)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let specs = zero_or_more(|| match self.ConstSpec() {
                Ok(Some(out)) => {
                    self.token(Token::SEMICOLON)?;
                    Ok(Some(ast::Spec::ValueSpec(out)))
                }
                _ => Ok(None),
            })
            .required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: const_.0,
                tok: const_.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            })));
        }

        let specs = vec![ast::Spec::ValueSpec(self.ConstSpec().required()?)];
        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: const_.0,
            tok: const_.1,
            lparen: None,
            specs,
            rparen: None,
        })))
    }

    // ConstSpec = IdentifierList [ [ Type ] "=" ExpressionList ] .
    fn ConstSpec(&mut self) -> ParserResult<Option<&'a ast::ValueSpec<'a>>> {
        log::debug!("Parser::ConstSpec()");

        let names = match self.IdentifierList()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, self.ExpressionList().required()?)
        } else {
            (
                self.Type()?,
                if self.token(Token::ASSIGN)?.is_some() {
                    self.ExpressionList().required()?
                } else {
                    vec![]
                },
            )
        };

        Ok(Some(self.alloc(ast::ValueSpec {
            doc: None,
            names,
            type_,
            values,
            comment: None,
        })))
    }

    // VarDecl = "var" ( VarSpec | "(" { VarSpec ";" } ")" ) .
    fn VarDecl(&mut self) -> ParserResult<Option<&'a ast::GenDecl<'a>>> {
        log::debug!("Parser::VarDecl()");

        let var = match self.token(Token::VAR)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let specs = zero_or_more(|| match self.VarSpec() {
                Ok(Some(out)) => {
                    self.token(Token::SEMICOLON)?;
                    Ok(Some(ast::Spec::ValueSpec(out)))
                }
                _ => Ok(None),
            })
            .required()?;

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(self.alloc(ast::GenDecl {
                doc: None,
                tok_pos: var.0,
                tok: var.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            })));
        }

        let specs = vec![ast::Spec::ValueSpec(self.VarSpec().required()?)];
        Ok(Some(self.alloc(ast::GenDecl {
            doc: None,
            tok_pos: var.0,
            tok: var.1,
            lparen: None,
            specs,
            rparen: None,
        })))
    }

    // VarSpec = IdentifierList ( Type [ "=" ExpressionList ] | "=" ExpressionList ) .
    fn VarSpec(&mut self) -> ParserResult<Option<&'a ast::ValueSpec<'a>>> {
        log::debug!("Parser::VarSpec()");

        let names = match self.IdentifierList()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, self.ExpressionList().required()?)
        } else {
            (
                Some(self.Type().required()?),
                if self.token(Token::ASSIGN)?.is_some() {
                    self.ExpressionList().required()?
                } else {
                    vec![]
                },
            )
        };

        Ok(Some(self.alloc(ast::ValueSpec {
            doc: None,
            names,
            type_,
            values,
            comment: None,
        })))
    }

    // IdentifierList = identifier { "," identifier } .
    fn IdentifierList(&mut self) -> ParserResult<Option<Vec<&'a ast::Ident<'a>>>> {
        log::debug!("Parser::IdentifierList()");

        let mut out = vec![];

        if let Some(ident) = self.identifier()? {
            out.push(ident);
        } else {
            return Ok(None);
        }

        while self.token(Token::COMMA)?.is_some() {
            out.push(self.identifier().required()?);
        }

        Ok(Some(out))
    }

    // ExpressionList = Expression { "," Expression } .
    fn ExpressionList(&mut self) -> ParserResult<Option<Vec<ast::Expr<'a>>>> {
        log::debug!("Parser::ExpressionList()");

        let mut out = vec![];

        if let Some(expression) = self.Expression()? {
            out.push(expression);
        } else {
            return Ok(None);
        }

        while self.token(Token::COMMA)?.is_some() {
            out.push(self.Expression().required()?);
        }

        Ok(Some(out))
    }

    // Expression = UnaryExpr | Expression binary_op Expression .
    fn Expression(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::Expression()");

        let x = match self.UnaryExpr()? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(op) = self.binary_op()? {
            let y = self.Expression().required()?;
            return Ok(Some(ast::Expr::BinaryExpr(self.alloc(ast::BinaryExpr {
                x,
                op_pos: op.0,
                op: op.1,
                y,
            }))));
        }

        Ok(Some(x))
    }

    // UnaryExpr = PrimaryExpr | unary_op UnaryExpr .
    fn UnaryExpr(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::UnaryExpr()");

        if let Some(op) = self.unary_op()? {
            let x = self.UnaryExpr().required()?;
            let out = if op.1 == Token::MUL {
                ast::Expr::StarExpr(self.alloc(ast::StarExpr { star: op.0, x }))
            } else {
                ast::Expr::UnaryExpr(self.alloc(ast::UnaryExpr {
                    op: op.1,
                    op_pos: op.0,
                    x,
                }))
            };
            return Ok(Some(out));
        }

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
    fn PrimaryExpr(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::PrimaryExpr()");

        self.Operand()
    }

    // Operand = Literal | OperandName | "(" Expression ")" .
    fn Operand(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::Operand()");

        if let Some(lit) = self.Literal()? {
            return Ok(Some(lit));
        }

        self.OperandName()
    }

    // Literal = BasicLit | CompositeLit | FunctionLit .
    fn Literal(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::Literal()");

        if let Some(lit) = self.BasicLit()? {
            return Ok(Some(ast::Expr::BasicLit(lit)));
        }

        Ok(None)
    }

    // OperandName = identifier | QualifiedIdent .
    fn OperandName(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::OperandName()");

        if let Some(ident) = self.identifier()? {
            return Ok(Some(ast::Expr::Ident(ident)));
        }

        Ok(None)
    }

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    fn BasicLit(&mut self) -> ParserResult<Option<&'a ast::BasicLit<'a>>> {
        log::debug!("Parser::BasicLit()");

        self.int_lit()
    }

    // Type = TypeName | TypeLit | "(" Type ")" .
    fn Type(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::Type()");

        if self.token(Token::LPAREN)?.is_some() {
            let type_ = self.Type().required()?;
            self.token(Token::RPAREN).required()?;
            return Ok(Some(type_));
        }

        if let Some(type_name) = self.TypeName()? {
            return Ok(Some(type_name));
        }

        if let Some(type_lit) = self.TypeLit()? {
            return Ok(Some(type_lit));
        }

        Ok(None)
    }

    // TypeName = identifier | QualifiedIdent .
    fn TypeName(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::TypeName()");

        if let Some(ident) = self.identifier()? {
            return Ok(Some(ast::Expr::Ident(ident)));
        }

        Ok(None)
    }

    // TypeLit = ArrayType | StructType | PointerType | FunctionType | InterfaceType |
    //           SliceType | MapType | ChannelType .
    fn TypeLit(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::TypeLit()");

        if let Some(t) = self.StructType()? {
            return Ok(Some(ast::Expr::StructType(t)));
        }

        if let Some(t) = self.PointerType()? {
            return Ok(Some(ast::Expr::StarExpr(t)));
        }

        if let Some(t) = self.InterfaceType()? {
            return Ok(Some(ast::Expr::InterfaceType(t)));
        }

        Ok(None)
    }

    // PointerType = "*" BaseType .
    fn PointerType(&mut self) -> ParserResult<Option<&'a ast::StarExpr<'a>>> {
        let star = match self.token(Token::MUL)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let x = self.BaseType().required()?;
        Ok(Some(self.alloc(ast::StarExpr { star: star.0, x })))
    }

    // BaseType = Type .
    fn BaseType(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        self.Type()
    }

    // InterfaceType = "interface" "{" { ( MethodSpec | InterfaceTypeName ) ";" } "}" .
    // MethodSpec    = MethodName Signature .
    fn InterfaceType(&mut self) -> ParserResult<Option<&'a ast::InterfaceType<'a>>> {
        log::debug!("Parser::InterfaceType()");

        let interface = match self.token(Token::INTERFACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let fields = zero_or_more(|| {
            if let Some(method_spec) = self.MethodName()? {
                if let Some(signature) = self.Signature(None)? {
                    self.token(Token::SEMICOLON).required()?;
                    return Ok(Some(self.alloc(ast::Field {
                        doc: None,
                        names: Some(vec![method_spec]),
                        type_: Some(ast::Expr::FuncType(signature)),
                        tag: None,
                        comment: None,
                    })));
                }

                self.token(Token::SEMICOLON).required()?;
                return Ok(Some(self.alloc(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(ast::Expr::Ident(method_spec)),
                    tag: None,
                    comment: None,
                })));
            };

            if let Some(interface_type_name) = self.InterfaceTypeName()? {
                self.token(Token::SEMICOLON).required()?;
                return Ok(Some(self.alloc(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(interface_type_name),
                    tag: None,
                    comment: None,
                })));
            }

            Ok(None)
        })
        .required()?;

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(self.alloc(ast::InterfaceType {
            interface: interface.0,
            methods: Some(self.alloc(ast::FieldList {
                opening: Some(lbrace.0),
                list: fields,
                closing: Some(rbrace.0),
            })),
            incomplete: false,
        })))
    }

    // MethodName = identifier .
    fn MethodName(&mut self) -> ParserResult<Option<&'a ast::Ident<'a>>> {
        log::debug!("Parser::MethodName()");

        self.identifier()
    }

    // InterfaceTypeName = TypeName .
    fn InterfaceTypeName(&mut self) -> ParserResult<Option<ast::Expr<'a>>> {
        log::debug!("Parser::InterfaceTypeName()");

        self.TypeName()
    }

    // StructType = "struct" "{" { FieldDecl ";" } "}" .
    fn StructType(&mut self) -> ParserResult<Option<&'a ast::StructType<'a>>> {
        log::debug!("Parser::StructType()");

        let struct_ = match self.token(Token::STRUCT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let fields = zero_or_more(|| match self.FieldDecl() {
            Ok(Some(out)) => {
                self.token(Token::SEMICOLON)?;
                Ok(Some(out))
            }
            _ => Ok(None),
        })
        .required()?;

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(self.alloc(ast::StructType {
            struct_: struct_.0,
            fields: Some(self.alloc(ast::FieldList {
                opening: Some(lbrace.0),
                list: fields,
                closing: Some(rbrace.0),
            })),
            incomplete: false,
        })))
    }

    // FieldDecl     = (IdentifierList Type | EmbeddedField) [ Tag ] .
    // EmbeddedField = [ "*" ] TypeName .
    fn FieldDecl(&mut self) -> ParserResult<Option<&'a ast::Field<'a>>> {
        if let Some(star) = self.token(Token::MUL)? {
            let type_name = self.TypeName().required()?;
            let tag = self.Tag()?;
            return Ok(Some(self.alloc(ast::Field {
                doc: None,
                type_: Some(ast::Expr::StarExpr(self.alloc(ast::StarExpr {
                    star: star.0,
                    x: type_name,
                }))),
                names: None,
                tag,
                comment: None,
            })));
        };

        if let Some(names) = self.IdentifierList()? {
            if let Some(type_) = self.Type()? {
                let tag = self.Tag()?;
                return Ok(Some(self.alloc(ast::Field {
                    doc: None,
                    names: Some(names),
                    type_: Some(type_),
                    tag,
                    comment: None,
                })));
            }

            if names.len() == 1 {
                let tag = self.Tag()?;
                return Ok(Some(self.alloc(ast::Field {
                    doc: None,
                    type_: Some(ast::Expr::Ident(names[0])),
                    names: None,
                    tag,
                    comment: None,
                })));
            }

            return Err(ParserError::UnexpectedToken);
        }

        if let Some(type_) = self.TypeName()? {
            let tag = self.Tag()?;
            return Ok(Some(self.alloc(ast::Field {
                doc: None,
                type_: Some(type_),
                names: None,
                tag,
                comment: None,
            })));
        }

        Ok(None)
    }

    // Tag = string_lit .
    fn Tag(&mut self) -> ParserResult<Option<&'a ast::BasicLit<'a>>> {
        self.string_lit()
    }

    // FunctionDecl = "func" FunctionName Signature [ FunctionBody ] .
    fn FunctionDecl(&mut self) -> ParserResult<Option<&'a ast::FuncDecl<'a>>> {
        log::debug!("Parser::FunctionDecl()");

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let function_name = self.FunctionName().required()?;

        let signature = self.Signature(Some(func.0)).required()?;

        let function_body = self.FunctionBody()?;

        Ok(Some(self.alloc(ast::FuncDecl {
            doc: None,
            recv: None,
            name: function_name,
            type_: signature,
            body: function_body,
        })))
    }

    // Signature = Parameters [ Result ] .
    fn Signature(
        &mut self,
        func: Option<Position<'a>>,
    ) -> ParserResult<Option<&'a ast::FuncType<'a>>> {
        log::debug!("Parser::Signature()");

        let params = match self.Parameters()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let results = self.Result()?;

        Ok(Some(self.alloc(ast::FuncType {
            func,
            params,
            results,
        })))
    }

    // Result = Parameters | Type .
    fn Result(&mut self) -> ParserResult<Option<&'a ast::FieldList<'a>>> {
        log::debug!("Parser::Result()");

        if let Some(parameters) = self.Parameters()? {
            Ok(Some(parameters))
        } else if let Some(type_) = self.Type()? {
            Ok(Some(self.alloc(ast::FieldList {
                opening: None,
                list: vec![self.alloc(ast::Field {
                    doc: None,
                    names: None,
                    tag: None,
                    type_: Some(type_),
                    comment: None,
                })],
                closing: None,
            })))
        } else {
            Ok(None)
        }
    }

    // FunctionName = identifier .
    fn FunctionName(&mut self) -> ParserResult<Option<&'a ast::Ident<'a>>> {
        log::debug!("Parser::FunctionName()");

        self.identifier()
    }

    // Parameters = "(" [ ParameterList [ "," ] ] ")" .
    fn Parameters(&mut self) -> ParserResult<Option<&'a ast::FieldList<'a>>> {
        log::debug!("Parser::Parameters()");

        let lparen = match self.token(Token::LPAREN)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let list = self
            .ParameterList()?
            .map(|list| {
                let _ = self.token(Token::COMMA);
                list
            })
            .unwrap_or_default();

        let rparen = self.token(Token::RPAREN).required()?;
        Ok(Some(self.alloc(ast::FieldList {
            opening: Some(lparen.0),
            list,
            closing: Some(rparen.0),
        })))
    }

    // ParameterList  = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl  = [ IdentifierList ] [ "..." ] Type .
    // IdentifierList = identifier { "," identifier } .
    fn ParameterList(&mut self) -> ParserResult<Option<Vec<&'a ast::Field<'a>>>> {
        log::debug!("Parser::ParameterList()");

        let idents = match self.IdentifierList()? {
            Some(v) => v,
            None => return Ok(None),
        };
        let type_ = self.Type()?;

        // If no type can be found, then the idents are types, e.g.: (bool, bool)
        if type_.is_none() {
            return Ok(Some(
                idents
                    .iter()
                    .map(|ident| {
                        self.alloc(ast::Field {
                            doc: None,
                            names: None,
                            type_: Some(ast::Expr::Ident(ident)),
                            tag: None,
                            comment: None,
                        })
                    })
                    .collect(),
            ));
        }

        // If a type can be found, then we expect idents + types: (a, b bool, c bool, d bool)

        let mut fields = vec![self.alloc(ast::Field {
            comment: None,
            type_,
            tag: None,
            names: Some(idents),
            doc: None,
        })];

        while self.token(Token::COMMA)?.is_some() {
            let idents = self.IdentifierList().required()?;
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.Type().required()?;

            if let Some(ellipsis) = ellipsis {
                fields.push(self.alloc(ast::Field {
                    comment: None,
                    type_: Some(ast::Expr::Ellipsis(self.alloc(ast::Ellipsis {
                        ellipsis: ellipsis.0,
                        elt: type_,
                    }))),
                    tag: None,
                    names: Some(idents),
                    doc: None,
                }));
                return Ok(Some(fields));
            }

            fields.push(self.alloc(ast::Field {
                comment: None,
                type_: Some(type_),
                tag: None,
                names: Some(idents),
                doc: None,
            }));
        }

        Ok(Some(fields))
    }

    // FunctionBody = Block .
    fn FunctionBody(&mut self) -> ParserResult<Option<&'a ast::BlockStmt<'a>>> {
        log::debug!("Parser::FunctionBody()");

        self.Block()
    }

    // Block = "{" StatementList "}" .
    fn Block(&mut self) -> ParserResult<Option<&'a ast::BlockStmt<'a>>> {
        log::debug!("Parser::Block()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let list = self.StatementList().required()?;
        let rbrace = self.token(Token::RBRACE).required()?;
        Ok(Some(self.alloc(ast::BlockStmt {
            lbrace: lbrace.0,
            list,
            rbrace: rbrace.0,
        })))
    }

    // StatementList = { Statement ";" } .
    fn StatementList(&mut self) -> ParserResult<Option<Vec<ast::Stmt<'a>>>> {
        log::debug!("Parser::StatementList()");

        zero_or_more(|| match self.Statement() {
            Ok(Some(out)) => {
                self.token(Token::SEMICOLON)?;
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
    fn Statement(&mut self) -> ParserResult<Option<ast::Stmt<'a>>> {
        log::debug!("Parser::Statement()");

        if let Some(decl) = self.Declaration()? {
            return Ok(Some(ast::Stmt::DeclStmt(
                self.alloc(ast::DeclStmt { decl }),
            )));
        }

        if let Some(s) = self.ReturnStmt()? {
            return Ok(Some(ast::Stmt::ReturnStmt(s)));
        }

        self.SimpleStmt()
    }

    // ReturnStmt = "return" [ ExpressionList ] .
    fn ReturnStmt(&mut self) -> ParserResult<Option<&'a ast::ReturnStmt<'a>>> {
        log::debug!("Parser::ReturnStmt()");

        if let Some(return_) = self.token(Token::RETURN)? {
            let results = self.ExpressionList()?.unwrap_or_default();
            Ok(Some(self.alloc(ast::ReturnStmt {
                return_: return_.0,
                results,
            })))
        } else {
            Ok(None)
        }
    }

    // SimpleStmt   = EmptyStmt | ExpressionStmt | SendStmt | IncDecStmt | Assignment | ShortVarDecl .
    // Assignment   = ExpressionList assign_op ExpressionList .
    // ShortVarDecl = IdentifierList ":=" ExpressionList .
    fn SimpleStmt(&mut self) -> ParserResult<Option<ast::Stmt<'a>>> {
        log::debug!("Parser::SimpleStmt()");

        let lhs = self.ExpressionList().required()?;

        // ShortVarDecl
        if lhs.iter().all(|node| matches!(node, ast::Expr::Ident(_))) {
            if let Some(define_op) = self.token(Token::DEFINE)? {
                let rhs = self.ExpressionList().required()?;
                return Ok(Some(ast::Stmt::AssignStmt(self.alloc(ast::AssignStmt {
                    lhs,
                    tok_pos: define_op.0,
                    tok: define_op.1,
                    rhs,
                }))));
            }
        }

        // Assignment
        let assign_op = self.assign_op().required()?;
        let rhs = self.ExpressionList().required()?;
        Ok(Some(ast::Stmt::AssignStmt(self.alloc(ast::AssignStmt {
            lhs,
            tok_pos: assign_op.0,
            tok: assign_op.1,
            rhs,
        }))))
    }

    /*
     * Lexical tokens (terminal productions)
     */

    // assign_op = [ add_op | mul_op ] "=" .
    // add_op     = "+" | "-" | "|" | "^" .
    // mul_op     = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    fn assign_op(&mut self) -> ParserResult<Option<(Position<'a>, Token, &'a str)>> {
        log::debug!("Parser::assign_op()");

        use Token::*;
        if let Some(current) = self.current {
            if matches!(
                current.1,
                /* "=" */
                ASSIGN |
                /* add_op "=" */
                ADD_ASSIGN | SUB_ASSIGN | OR_ASSIGN | XOR_ASSIGN |
                /* mul_op "=" */
                MUL_ASSIGN | QUO_ASSIGN | REM_ASSIGN | SHL_ASSIGN | SHR_ASSIGN | AND_ASSIGN | AND_NOT_ASSIGN
            ) {
                self.next()?;
                return Ok(Some(current));
            }
        }

        Ok(None)
    }

    // binary_op  = "||" | "&&" | rel_op | add_op | mul_op .
    // rel_op     = "==" | "!=" | "<" | "<=" | ">" | ">=" .
    // add_op     = "+" | "-" | "|" | "^" .
    // mul_op     = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    fn binary_op(&mut self) -> ParserResult<Option<(Position<'a>, Token, &'a str)>> {
        log::debug!("Parser::binary_op()");

        use Token::*;
        if let Some(current) = self.current {
            if matches!(
                current.1,
                /* binary_op */
                LOR | LAND |
                /* rel_op */
                EQL | NEQ | LSS | LEQ | GTR | GEQ |
                /* add_op */
                ADD | SUB | OR | XOR |
                /* mul_op */
                MUL | QUO | REM | SHL | SHR | AND | AND_NOT
            ) {
                self.next()?;
                return Ok(Some(current));
            }
        }

        Ok(None)
    }

    // unary_op   = "+" | "-" | "!" | "^" | "*" | "&" | "<-" .
    fn unary_op(&mut self) -> ParserResult<Option<(Position<'a>, Token, &'a str)>> {
        log::debug!("Parser::unary_op()");

        use Token::*;
        if let Some(current) = self.current {
            if matches!(current.1, ADD | SUB | NOT | MUL | XOR | AND | ARROW) {
                self.next()?;
                return Ok(Some(current));
            }
        }

        Ok(None)
    }

    fn identifier(&mut self) -> ParserResult<Option<&'a ast::Ident<'a>>> {
        log::debug!("Parser::identifier()");

        if let Some(ident) = self.token(Token::IDENT)? {
            Ok(Some(self.alloc(ast::Ident {
                name_pos: ident.0,
                name: ident.2,
                obj: Cell::new(None),
            })))
        } else {
            Ok(None)
        }
    }

    fn int_lit(&mut self) -> ParserResult<Option<&'a ast::BasicLit<'a>>> {
        log::debug!("Parser::int_lit()");

        if let Some(int_lit) = self.token(Token::INT)? {
            Ok(Some(self.alloc(ast::BasicLit {
                kind: int_lit.1,
                value: int_lit.2,
                value_pos: int_lit.0,
            })))
        } else {
            Ok(None)
        }
    }

    fn string_lit(&mut self) -> ParserResult<Option<&'a ast::BasicLit<'a>>> {
        log::debug!("Parser::string_lit()");

        if let Some(string_lit) = self.token(Token::STRING)? {
            Ok(Some(self.alloc(ast::BasicLit {
                value_pos: string_lit.0,
                kind: string_lit.1,
                value: string_lit.2,
            })))
        } else {
            Ok(None)
        }
    }

    fn token(&mut self, expected: Token) -> ParserResult<Option<(Position<'a>, Token, &'a str)>> {
        if let Some(current) = self.current {
            if current.1 == expected {
                self.next()?;
                Ok(Some(current))
            } else {
                Ok(None)
            }
        } else {
            Err(ParserError::UnexpectedEndOfFile)
        }
    }
}

fn zero_or_more<T>(
    mut op: impl FnMut() -> ParserResult<Option<T>>,
) -> ParserResult<Option<Vec<T>>> {
    let mut out = vec![];
    while let Some(v) = op()? {
        out.push(v);
    }
    Ok(Some(out))
}
