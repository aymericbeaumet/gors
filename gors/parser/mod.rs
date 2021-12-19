#![allow(non_snake_case)] // TODO: switch to parse_* function naming

use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use std::fmt;

#[derive(Debug)]
pub enum ParserError {
    ScannerError(scanner::ScannerError),
    UnexpectedEndOfFile,
    UnexpectedToken,
    UnexpectedTokenAt {
        at: String,
        token: Token,
        literal: String,
    },
}

impl std::error::Error for ParserError {}

impl From<scanner::ScannerError> for ParserError {
    fn from(e: scanner::ScannerError) -> Self {
        Self::ScannerError(e)
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parser error: {:?}", self)
    }
}

pub type Result<T> = std::result::Result<T, ParserError>;

trait ResultExt<T> {
    fn required(self) -> Result<T>;
}

impl<T> ResultExt<T> for Result<Option<T>> {
    fn required(self) -> Result<T> {
        self.and_then(|node| node.map_or(Err(ParserError::UnexpectedToken), |node| Ok(node)))
    }
}

pub fn parse_file<'a>(filename: &'a str, buffer: &'a str) -> Result<ast::File<'a>> {
    let scanner = scanner::Scanner::new(filename, buffer);
    let mut parser = Parser::new(scanner);
    parser.next()?;
    parser
        .SourceFile()
        .required()
        .map_err(|err| match (err, parser.current) {
            (ParserError::UnexpectedToken, Some(current)) => ParserError::UnexpectedTokenAt {
                at: current.0.to_string(),
                token: current.1,
                literal: current.2.to_owned(),
            },
            (err, _) => err,
        })
}

struct Parser<'scanner> {
    scanner: scanner::IntoIter<'scanner>,
    current: Option<scanner::Step<'scanner>>,
}

impl<'scanner> Parser<'scanner> {
    pub fn new(scanner: scanner::Scanner<'scanner>) -> Self {
        Self {
            current: None,
            scanner: scanner.into_iter(),
        }
    }

    fn next(&mut self) -> Result<()> {
        self.current = if let Some(step) = self.scanner.next() {
            Some(step?)
        } else {
            None
        };
        log::debug!("self.current = {:?}", self.current);
        Ok(())
    }

    /*
     * Non-terminal productions
     */

    // SourceFile = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    fn SourceFile(&mut self) -> Result<Option<ast::File<'scanner>>> {
        log::debug!("Parser::SourceFile()");

        let (package, package_name) = match self.PackageClause()? {
            Some(v) => v,
            None => return Ok(None),
        };

        self.token(Token::SEMICOLON).required()?;

        let mut out = ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls: vec![],
            scope: None,
            unresolved: vec![],
            comments: vec![],
        };

        while let Some(import_decl) = self.ImportDecl()? {
            self.token(Token::SEMICOLON).required()?;
            out.decls.push(ast::Decl::GenDecl(import_decl));
        }

        while let Some(top_level_decl) = self.TopLevelDecl()? {
            self.token(Token::SEMICOLON).required()?;
            out.decls.push(top_level_decl);
        }

        self.token(Token::EOF).required()?;

        Ok(Some(out))
    }

    // PackageClause = "package" PackageName .
    fn PackageClause(&mut self) -> Result<Option<(scanner::Step<'scanner>, ast::Ident<'scanner>)>> {
        log::debug!("Parser::PackageClause()");

        let package = match self.token(Token::PACKAGE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let package_name = self.PackageName().required()?;

        Ok(Some((package, package_name)))
    }

    // PackageName = identifier .
    fn PackageName(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::PackageName()");

        self.identifier()
    }

    // ImportDecl = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    fn ImportDecl(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::ImportDecl()");

        let import = match self.token(Token::IMPORT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            while let Some(import_spec) = self.ImportSpec()? {
                specs.push(ast::Spec::ImportSpec(import_spec));
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc: None,
                tok_pos: import.0,
                tok: import.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let specs = vec![ast::Spec::ImportSpec(self.ImportSpec().required()?)];
        Ok(Some(ast::GenDecl {
            doc: None,
            tok_pos: import.0,
            tok: import.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // ImportSpec = [ "." | PackageName ] ImportPath .
    fn ImportSpec(&mut self) -> Result<Option<ast::ImportSpec<'scanner>>> {
        log::debug!("Parser::ImportSpec()");

        if let Some(name) = self.period_or_PackageName()? {
            let path = self.ImportPath().required()?;
            return Ok(Some(ast::ImportSpec {
                doc: None,
                name: Some(name),
                path,
                comment: None,
            }));
        }

        let import_path = match self.ImportPath()? {
            Some(v) => v,
            None => return Ok(None),
        };

        Ok(Some(ast::ImportSpec {
            doc: None,
            name: None,
            path: import_path,
            comment: None,
        }))
    }

    // ImportPath = string_lit .
    fn ImportPath(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::ImportPath()");

        self.string_lit()
    }

    // TopLevelDecl = Declaration | FunctionDecl | MethodDecl .
    fn TopLevelDecl(&mut self) -> Result<Option<ast::Decl<'scanner>>> {
        log::debug!("Parser::TopLevelDecl()");

        if let Some(decl) = self.Declaration()? {
            return Ok(Some(ast::Decl::GenDecl(decl)));
        }

        if let Some(decl) = self.FunctionDecl_or_MethodDecl()? {
            return Ok(Some(ast::Decl::FuncDecl(decl)));
        }

        Ok(None)
    }

    // Declaration = ConstDecl | TypeDecl | VarDecl .
    fn Declaration(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
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
    fn TypeDecl(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::TypeDecl()");

        let type_ = match self.token(Token::TYPE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            while let Some(type_spec) = self.TypeSpec()? {
                specs.push(ast::Spec::TypeSpec(type_spec));
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc: None,
                tok_pos: type_.0,
                tok: type_.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let specs = vec![ast::Spec::TypeSpec(self.TypeSpec().required()?)];
        Ok(Some(ast::GenDecl {
            doc: None,
            tok_pos: type_.0,
            tok: type_.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // TypeSpec  = AliasDecl | TypeDef .
    // AliasDecl = identifier "=" Type .
    // TypeDef   = identifier Type .
    fn TypeSpec(&mut self) -> Result<Option<ast::TypeSpec<'scanner>>> {
        log::debug!("Parser::TypeSpec()");

        let name = match self.identifier()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);

        let type_ = self.Type().required()?;

        Ok(Some(ast::TypeSpec {
            doc: None,
            name: Some(name),
            assign,
            type_,
            comment: None,
        }))
    }

    // ConstDecl = "const" ( ConstSpec | "(" { ConstSpec ";" } ")" ) .
    fn ConstDecl(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::ConstDecl()");

        let const_ = match self.token(Token::CONST)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            while let Some(const_spec) = self.ConstSpec()? {
                specs.push(ast::Spec::ValueSpec(const_spec));
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc: None,
                tok_pos: const_.0,
                tok: const_.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let specs = vec![ast::Spec::ValueSpec(self.ConstSpec().required()?)];
        Ok(Some(ast::GenDecl {
            doc: None,
            tok_pos: const_.0,
            tok: const_.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // ConstSpec = IdentifierList [ [ Type ] "=" ExpressionList ] .
    fn ConstSpec(&mut self) -> Result<Option<ast::ValueSpec<'scanner>>> {
        log::debug!("Parser::ConstSpec()");

        let names = match self.IdentifierList()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, Some(self.ExpressionList().required()?))
        } else if let Some(type_) = self.Type()? {
            self.token(Token::ASSIGN).required()?;
            (Some(type_), Some(self.ExpressionList().required()?))
        } else {
            (None, None)
        };

        Ok(Some(ast::ValueSpec {
            doc: None,
            names,
            type_,
            values,
            comment: None,
        }))
    }

    // VarDecl = "var" ( VarSpec | "(" { VarSpec ";" } ")" ) .
    fn VarDecl(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::VarDecl()");

        let var = match self.token(Token::VAR)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            while let Some(var_spec) = self.VarSpec()? {
                specs.push(ast::Spec::ValueSpec(var_spec));
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc: None,
                tok_pos: var.0,
                tok: var.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let specs = vec![ast::Spec::ValueSpec(self.VarSpec().required()?)];
        Ok(Some(ast::GenDecl {
            doc: None,
            tok_pos: var.0,
            tok: var.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // VarSpec = IdentifierList ( Type [ "=" ExpressionList ] | "=" ExpressionList ) .
    fn VarSpec(&mut self) -> Result<Option<ast::ValueSpec<'scanner>>> {
        log::debug!("Parser::VarSpec()");

        let names = match self.IdentifierList()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, Some(self.ExpressionList().required()?))
        } else {
            (
                Some(self.Type().required()?),
                if self.token(Token::ASSIGN)?.is_some() {
                    Some(self.ExpressionList().required()?)
                } else {
                    None
                },
            )
        };

        Ok(Some(ast::ValueSpec {
            doc: None,
            names,
            type_,
            values,
            comment: None,
        }))
    }

    // IdentifierList = identifier { "," identifier } .
    fn IdentifierList(&mut self) -> Result<Option<Vec<ast::Ident<'scanner>>>> {
        log::debug!("Parser::IdentifierList()");

        let mut out = match self.identifier()? {
            Some(v) => vec![v],
            None => return Ok(None),
        };

        while self.token(Token::COMMA)?.is_some() {
            out.push(self.identifier().required()?);
        }

        Ok(Some(out))
    }

    // ExpressionList = Expression { "," Expression } .
    fn ExpressionList(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
        log::debug!("Parser::ExpressionList()");

        let mut out = match self.Expression()? {
            Some(v) => vec![v],
            None => return Ok(None),
        };

        while self.token(Token::COMMA)?.is_some() {
            out.push(self.Expression().required()?);
        }

        Ok(Some(out))
    }

    // Expression = UnaryExpr | Expression binary_op Expression .
    fn Expression(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Expression()");

        let unary_expr = match self.UnaryExpr()? {
            Some(v) => v,
            None => return Ok(None),
        };

        self.expression(unary_expr, Token::lowest_precedence())
    }

    // https://en.wikipedia.org/wiki/Operator-precedence_parser
    fn expression(
        &mut self,
        mut lhs: ast::Expr<'scanner>,
        min_precedence: u8,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        while let Some(op) = self.peek_binary_op(min_precedence)? {
            self.next()?;

            let mut rhs = self.UnaryExpr().required()?;
            while self.peek_binary_op(op.1.precedence() + 1)?.is_some() {
                rhs = self.expression(rhs, op.1.precedence() + 1).required()?;
            }

            lhs = ast::Expr::BinaryExpr(ast::BinaryExpr {
                x: Box::new(lhs),
                op_pos: op.0,
                op: op.1,
                y: Box::new(rhs),
            });
        }

        Ok(Some(lhs))
    }

    // UnaryExpr = PrimaryExpr | unary_op UnaryExpr .
    fn UnaryExpr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::UnaryExpr()");

        if let Some(op) = self.unary_op()? {
            let x = Box::new(self.UnaryExpr().required()?);
            let expr = if op.1 == Token::MUL {
                ast::Expr::StarExpr(ast::StarExpr { star: op.0, x })
            } else {
                ast::Expr::UnaryExpr(ast::UnaryExpr {
                    op: op.1,
                    op_pos: op.0,
                    x,
                })
            };
            return Ok(Some(expr));
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
    fn PrimaryExpr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::PrimaryExpr()");

        let mut primary_expr = match self.Operand_or_Conversion_or_MethodExpr()? {
            Some(v) => v,
            None => return Ok(None),
        };

        loop {
            // TODO: match

            if let Some((lbrack, expr, rbrack)) = self.Index()? {
                primary_expr = ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(primary_expr),
                    rbrack,
                    index: Box::new(expr),
                    lbrack,
                });
                continue;
            }

            if let Some((lparen, args, rparen)) = self.Arguments()? {
                primary_expr = ast::Expr::CallExpr(ast::CallExpr {
                    fun: Box::new(primary_expr),
                    lparen,
                    args: Some(args),
                    ellipsis: None,
                    rparen,
                });
                continue;
            }

            break;
        }

        Ok(Some(primary_expr))
    }

    // Index = "[" Expression "]" .
    fn Index(
        &mut self,
    ) -> Result<Option<(Position<'scanner>, ast::Expr<'scanner>, Position<'scanner>)>> {
        log::debug!("Parser::Index()");

        let lbrack = match self.token(Token::LBRACK)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let expr = self.Expression().required()?;
        let rbrack = self.token(Token::RBRACK).required()?;

        Ok(Some((lbrack.0, expr, rbrack.0)))
    }

    // Operand = Literal | OperandName | "(" Expression ")" .
    fn Operand(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Operand()");

        if let Some(literal) = self.Literal()? {
            return Ok(Some(literal));
        }

        if let Some(operand_name) = self.OperandName()? {
            return Ok(Some(operand_name));
        }

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let expr = self.Expression().required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            return Ok(Some(ast::Expr::ParenExpr(ast::ParenExpr {
                lparen: lparen.0,
                x: Box::new(expr),
                rparen: rparen.0,
            })));
        }

        Ok(None)
    }

    // Literal = BasicLit | CompositeLit | FunctionLit .
    fn Literal(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Literal()");

        if let Some(l) = self.BasicLit()? {
            return Ok(Some(ast::Expr::BasicLit(l)));
        }

        if let Some(l) = self.CompositeLit()? {
            return Ok(Some(ast::Expr::CompositeLit(l)));
        }

        if let Some(l) = self.FunctionLit()? {
            return Ok(Some(ast::Expr::FuncLit(l)));
        }

        Ok(None)
    }

    // CompositeLit = LiteralType LiteralValue .
    // LiteralValue = "{" [ ElementList [ "," ] ] "}" .
    fn CompositeLit(&mut self) -> Result<Option<ast::CompositeLit<'scanner>>> {
        log::debug!("Parser::CompositeLit()");

        let type_ = match self.LiteralType()? {
            Some(v) => v,
            None => return Ok(None),
        };
        let lbrace = self.token(Token::LBRACE).required()?;
        let elts = self.ElementList()?; // TODO: add support for trailing comma from grammar
        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(ast::CompositeLit {
            type_: Box::new(type_),
            lbrace: lbrace.0,
            elts,
            rbrace: rbrace.0,
            incomplete: false,
        }))
    }

    // LiteralType = StructType | ArrayType | "[" "..." "]" ElementType |
    //               SliceType | MapType | TypeName .
    fn LiteralType(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::LiteralType()");

        if let Some(t) = self.StructType()? {
            return Ok(Some(ast::Expr::StructType(t)));
        }

        if let Some(t) = self.MapType()? {
            return Ok(Some(ast::Expr::MapType(t)));
        }

        Ok(None)
    }

    // ElementList = KeyedElement { "," KeyedElement } .
    fn ElementList(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
        log::debug!("Parser::ElementList()");

        let mut out = match self.KeyedElement()? {
            Some(v) => vec![v],
            None => return Ok(None),
        };

        // TODO: peek instead of consuming to distinguish a trailing comma from a new comma+keyed
        // pair.
        while self.token(Token::COMMA)?.is_some() {
            if let Some(k) = self.KeyedElement()? {
                out.push(k);
            } else {
                break;
            }
        }

        Ok(Some(out))
    }

    // KeyedElement = [ Key ":" ] Element .
    // Key          = FieldName | Expression | LiteralValue .
    // FieldName    = identifier .
    // Element      = Expression | LiteralValue .
    fn KeyedElement(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        let key = match self.Expression()? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(colon) = self.token(Token::COLON)? {
            return Ok(Some(ast::Expr::KeyValueExpr(ast::KeyValueExpr {
                key: Box::new(key),
                colon: colon.0,
                value: Box::new(self.Expression().required()?),
            })));
        }

        self.Expression()
    }

    // FunctionLit = "func" Signature FunctionBody .
    fn FunctionLit(&mut self) -> Result<Option<ast::FuncLit<'scanner>>> {
        log::debug!("Parser::FunctionLit()");

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let type_ = self.Signature(Some(func.0)).required()?;

        let body = self.FunctionBody().required()?;

        Ok(Some(ast::FuncLit { type_, body }))
    }

    // OperandName = identifier | QualifiedIdent .
    fn OperandName(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::OperandName()");

        self.identifier_or_QualifiedIdent()
    }

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    fn BasicLit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::BasicLit()");

        if let Some(int_lit) = self.int_lit()? {
            return Ok(Some(int_lit));
        }

        if let Some(float_lit) = self.float_lit()? {
            return Ok(Some(float_lit));
        }

        if let Some(imaginary_lit) = self.imaginary_lit()? {
            return Ok(Some(imaginary_lit));
        }

        if let Some(rune_lit) = self.rune_lit()? {
            return Ok(Some(rune_lit));
        }

        if let Some(string_lit) = self.string_lit()? {
            return Ok(Some(string_lit));
        }

        Ok(None)
    }

    // Type = TypeName | TypeLit | "(" Type ")" .
    fn Type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn TypeName(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeName()");

        self.identifier_or_QualifiedIdent()
    }

    // TypeLit = ArrayType | StructType | PointerType | FunctionType | InterfaceType |
    //           SliceType | MapType | ChannelType .
    fn TypeLit(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeLit()");

        // TODO: match

        if let Some(t) = self.StructType()? {
            return Ok(Some(ast::Expr::StructType(t)));
        }

        if let Some(t) = self.PointerType()? {
            return Ok(Some(ast::Expr::StarExpr(t)));
        }

        if let Some(t) = self.InterfaceType()? {
            return Ok(Some(ast::Expr::InterfaceType(t)));
        }

        if let Some(t) = self.MapType()? {
            return Ok(Some(ast::Expr::MapType(t)));
        }

        if let Some(t) = self.ChannelType()? {
            return Ok(Some(ast::Expr::ChanType(t)));
        }

        Ok(None)
    }

    // MapType = "map" "[" KeyType "]" ElementType .
    fn MapType(&mut self) -> Result<Option<ast::MapType<'scanner>>> {
        log::debug!("Parser::MapType()");

        let map = match self.token(Token::MAP)? {
            Some(v) => v,
            None => return Ok(None),
        };
        self.token(Token::LBRACK).required()?;
        let key_type = self.KeyType().required()?;
        self.token(Token::RBRACK).required()?;
        let element_type = self.ElementType().required()?;

        Ok(Some(ast::MapType {
            map: map.0,
            key: Box::new(key_type),
            value: Box::new(element_type),
        }))
    }

    // KeyType = Type .
    fn KeyType(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::KeyType()");

        self.Type()
    }

    // ChannelType = ( "chan" | "chan" "<-" | "<-" "chan" ) ElementType .
    fn ChannelType(&mut self) -> Result<Option<ast::ChanType<'scanner>>> {
        log::debug!("Parser::ChannelType()");

        if let Some(chan) = self.token(Token::CHAN)? {
            if let Some(arrow) = self.token(Token::ARROW)? {
                let value = Box::new(self.ElementType().required()?);
                return Ok(Some(ast::ChanType {
                    begin: chan.0,
                    arrow: Some(arrow.0),
                    dir: ast::ChanDir::SEND as u8,
                    value,
                }));
            }

            let value = Box::new(self.ElementType().required()?);
            return Ok(Some(ast::ChanType {
                begin: chan.0,
                arrow: None,
                dir: ast::ChanDir::SEND as u8 | ast::ChanDir::RECV as u8,
                value,
            }));
        }

        if let Some(arrow) = self.token(Token::ARROW)? {
            self.token(Token::CHAN).required()?;
            let value = Box::new(self.ElementType().required()?);
            return Ok(Some(ast::ChanType {
                begin: arrow.0,
                arrow: None,
                dir: ast::ChanDir::RECV as u8,
                value,
            }));
        }

        Ok(None)
    }

    // ElementType = Type .
    fn ElementType(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::ElementType()");

        self.Type()
    }

    // PointerType = "*" BaseType .
    fn PointerType(&mut self) -> Result<Option<ast::StarExpr<'scanner>>> {
        log::debug!("Parser::PointerType()");

        let star = match self.token(Token::MUL)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let x = Box::new(self.BaseType().required()?);
        Ok(Some(ast::StarExpr { star: star.0, x }))
    }

    // BaseType = Type .
    fn BaseType(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::BaseType()");

        self.Type()
    }

    // InterfaceType = "interface" "{" { ( MethodSpec | InterfaceTypeName ) ";" } "}" .
    // MethodSpec    = MethodName Signature .
    fn InterfaceType(&mut self) -> Result<Option<ast::InterfaceType<'scanner>>> {
        log::debug!("Parser::InterfaceType()");

        let interface = match self.token(Token::INTERFACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut fields = vec![];
        loop {
            if let Some(method_spec) = self.MethodName()? {
                if let Some(signature) = self.Signature(None)? {
                    self.token(Token::SEMICOLON).required()?;
                    fields.push(ast::Field {
                        doc: None,
                        names: Some(vec![method_spec]),
                        type_: Some(ast::Expr::FuncType(signature)),
                        tag: None,
                        comment: None,
                    });
                    continue;
                }

                fields.push(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(ast::Expr::Ident(method_spec)),
                    tag: None,
                    comment: None,
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            };

            if let Some(interface_type_name) = self.InterfaceTypeName()? {
                fields.push(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(interface_type_name),
                    tag: None,
                    comment: None,
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            }

            break;
        }

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(ast::InterfaceType {
            interface: interface.0,
            methods: Some(ast::FieldList {
                opening: Some(lbrace.0),
                list: fields,
                closing: Some(rbrace.0),
            }),
            incomplete: false,
        }))
    }

    // MethodName = identifier .
    fn MethodName(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::MethodName()");

        self.identifier()
    }

    // InterfaceTypeName = TypeName .
    fn InterfaceTypeName(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::InterfaceTypeName()");

        self.TypeName()
    }

    // StructType = "struct" "{" { FieldDecl ";" } "}" .
    fn StructType(&mut self) -> Result<Option<ast::StructType<'scanner>>> {
        log::debug!("Parser::StructType()");

        let struct_ = match self.token(Token::STRUCT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut fields = vec![];
        while let Some(field_decl) = self.FieldDecl()? {
            fields.push(field_decl);
            if self.token(Token::SEMICOLON)?.is_none() {
                break;
            }
        }

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(ast::StructType {
            struct_: struct_.0,
            fields: Some(ast::FieldList {
                opening: Some(lbrace.0),
                list: fields,
                closing: Some(rbrace.0),
            }),
            incomplete: false,
        }))
    }

    // FieldDecl     = (IdentifierList Type | EmbeddedField) [ Tag ] .
    // EmbeddedField = [ "*" ] TypeName .
    fn FieldDecl(&mut self) -> Result<Option<ast::Field<'scanner>>> {
        log::debug!("Parser::FieldDecl()");

        if let Some(star) = self.token(Token::MUL)? {
            let type_name = Box::new(self.TypeName().required()?);
            let tag = self.Tag()?;
            return Ok(Some(ast::Field {
                doc: None,
                type_: Some(ast::Expr::StarExpr(ast::StarExpr {
                    star: star.0,
                    x: type_name,
                })),
                names: None,
                tag,
                comment: None,
            }));
        };

        if let Some(names) = self.IdentifierList()? {
            if let Some(type_) = self.Type()? {
                let tag = self.Tag()?;
                return Ok(Some(ast::Field {
                    doc: None,
                    names: Some(names),
                    type_: Some(type_),
                    tag,
                    comment: None,
                }));
            }

            if names.len() == 1 {
                let name = names.into_iter().next().unwrap();
                let tag = self.Tag()?;
                return Ok(Some(ast::Field {
                    doc: None,
                    type_: Some(ast::Expr::Ident(name)),
                    names: None,
                    tag,
                    comment: None,
                }));
            }

            return Err(ParserError::UnexpectedToken);
        }

        if let Some(type_) = self.TypeName()? {
            let tag = self.Tag()?;
            return Ok(Some(ast::Field {
                doc: None,
                type_: Some(type_),
                names: None,
                tag,
                comment: None,
            }));
        }

        Ok(None)
    }

    // Tag = string_lit .
    fn Tag(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::Tag()");

        self.string_lit()
    }

    // Signature = Parameters [ Result ] .
    fn Signature(
        &mut self,
        func: Option<Position<'scanner>>,
    ) -> Result<Option<ast::FuncType<'scanner>>> {
        log::debug!("Parser::Signature()");

        let params = match self.Parameters()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let results = self.Result()?;

        Ok(Some(ast::FuncType {
            func,
            params,
            results,
        }))
    }

    // Result = Parameters | Type .
    fn Result(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::Result()");

        if let Some(parameters) = self.Parameters()? {
            Ok(Some(parameters))
        } else if let Some(type_) = self.Type()? {
            Ok(Some(ast::FieldList {
                opening: None,
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    tag: None,
                    type_: Some(type_),
                    comment: None,
                }],
                closing: None,
            }))
        } else {
            Ok(None)
        }
    }

    // Parameters = "(" [ ParameterList [ "," ] ] ")" .
    fn Parameters(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
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

        Ok(Some(ast::FieldList {
            opening: Some(lparen.0),
            list,
            closing: Some(rparen.0),
        }))
    }

    // ParameterList  = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl  = [ IdentifierList ] [ "..." ] Type .
    fn ParameterList(&mut self) -> Result<Option<Vec<ast::Field<'scanner>>>> {
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
                    .into_iter()
                    .map(|ident| ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(ast::Expr::Ident(ident)),
                        tag: None,
                        comment: None,
                    })
                    .collect(),
            ));
        }

        // If a type can be found, then we expect idents + types: (a, b bool, c bool, d bool)

        let mut fields = vec![ast::Field {
            comment: None,
            type_,
            tag: None,
            names: Some(idents),
            doc: None,
        }];

        while self.token(Token::COMMA)?.is_some() {
            let idents = self.IdentifierList().required()?;
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.Type().required()?;

            if let Some(ellipsis) = ellipsis {
                fields.push(ast::Field {
                    comment: None,
                    type_: Some(ast::Expr::Ellipsis(ast::Ellipsis {
                        ellipsis: ellipsis.0,
                        elt: Box::new(type_),
                    })),
                    tag: None,
                    names: Some(idents),
                    doc: None,
                });
                return Ok(Some(fields));
            }

            fields.push(ast::Field {
                comment: None,
                type_: Some(type_),
                tag: None,
                names: Some(idents),
                doc: None,
            });
        }

        Ok(Some(fields))
    }

    // FunctionBody = Block .
    fn FunctionBody(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::FunctionBody()");

        self.Block()
    }

    // Block = "{" StatementList "}" .
    fn Block(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::Block()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let list = self.StatementList().required()?;

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(ast::BlockStmt {
            lbrace: lbrace.0,
            list,
            rbrace: rbrace.0,
        }))
    }

    // StatementList = { Statement ";" } .
    fn StatementList(&mut self) -> Result<Option<Vec<ast::Stmt<'scanner>>>> {
        log::debug!("Parser::StatementList()");

        let mut out = vec![];
        while let Some(statement) = self.Statement()? {
            out.push(statement);
            if self.token(Token::SEMICOLON)?.is_none() {
                break;
            }
        }

        Ok(Some(out))
    }

    // Statement =
    //         Declaration | LabeledStmt | SimpleStmt |
    //         GoStmt | ReturnStmt | BreakStmt | ContinueStmt | GotoStmt |
    //         FallthroughStmt | Block | IfStmt | SwitchStmt | SelectStmt | ForStmt |
    //         DeferStmt .
    fn Statement(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::Statement()");

        if let Some((_, tok, lit)) = self.peek_token()? {
            use Token::*;
            return match tok {
                CONST | TYPE | VAR => Ok(Some(ast::Stmt::DeclStmt(ast::DeclStmt {
                    decl: self.Declaration().required()?,
                }))),
                IDENT | INT | FLOAT | IMAG | CHAR | STRING | FUNC | LPAREN | // operands
                LBRACK | STRUCT | MAP | CHAN | INTERFACE | // composite types
                ADD | SUB | MUL | AND | XOR | ARROW | NOT // unary operators
                  => Ok(Some(self.SimpleStmt().required()?)),
                GO => Ok(Some(ast::Stmt::GoStmt(self.GoStmt().required()?))),
                // case token.DEFER:
                RETURN => Ok(Some(ast::Stmt::ReturnStmt(self.ReturnStmt().required()?))),
                //case token.BREAK, token.CONTINUE, token.GOTO, token.FALLTHROUGH:
                //case token.LBRACE:
                IF => Ok(Some(ast::Stmt::IfStmt(self.IfStmt().required()?))),
                //case token.SWITCH:
                //case token.SELECT:
                FOR => Ok(Some(self.ForStmt().required()?)),
                SEMICOLON => Ok(Some(ast::Stmt::EmptyStmt(ast::EmptyStmt{
                    semicolon: self.token(SEMICOLON).required()?.0,
                    implicit: lit == "\n",
                }))),
                _ => Ok(None),
            };
        }

        Ok(None)
    }

    // ForStmt = "for" [ Condition | ForClause | RangeClause ] Block .
    // ForClause = [ InitStmt ] ";" [ Condition ] ";" [ PostStmt ] .
    // RangeClause = [ ExpressionList "=" | IdentifierList ":=" ] "range" Expression .
    // InitStmt = SimpleStmt .
    // Condition = Expression .
    // PostStmt = SimpleStmt .
    fn ForStmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::ForStmt()");

        let for_ = match self.token(Token::FOR)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // for {}
        if let Some(body) = self.Block()? {
            return Ok(Some(ast::Stmt::ForStmt(ast::ForStmt {
                for_: for_.0,
                init: None,
                cond: None,
                post: None,
                body,
            })));
        }

        // for range x {}
        if self.token(Token::RANGE)?.is_some() {
            let x = self.Expression().required()?;
            let body = self.Block().required()?;
            return Ok(Some(ast::Stmt::RangeStmt(ast::RangeStmt {
                for_: for_.0,
                key: None,
                value: None,
                tok_pos: None,
                tok: None,
                x,
                body,
            })));
        }

        let init = if let Some(exprs) = self.ExpressionList()? {
            // for a < b {}
            if exprs.len() == 1 {
                if let Some(body) = self.Block()? {
                    let cond = exprs.into_iter().next().unwrap();
                    return Ok(Some(ast::Stmt::ForStmt(ast::ForStmt {
                        for_: for_.0,
                        init: None,
                        cond: Some(cond),
                        post: None,
                        body,
                    })));
                }
            }

            let mut tok: Option<scanner::Step> = None;

            // for a, b := range x {}
            if let Some(define) = self.token(Token::DEFINE)? {
                tok = Some(define);
                if self.token(Token::RANGE)?.is_some() {
                    let mut exprs = exprs.into_iter();
                    let key = exprs.next();
                    let value = exprs.next();
                    let x = self.Expression().required()?;
                    let body = self.Block().required()?;
                    return Ok(Some(ast::Stmt::RangeStmt(ast::RangeStmt {
                        for_: for_.0,
                        key,
                        value,
                        tok_pos: Some(define.0),
                        tok: Some(define.1),
                        x,
                        body,
                    })));
                }

            // for a, b = range x {}
            } else if exprs.iter().all(|expr| matches!(expr, ast::Expr::Ident(_))) {
                if let Some(assign) = self.token(Token::ASSIGN)? {
                    tok = Some(assign);
                    if self.token(Token::RANGE)?.is_some() {
                        let x = self.Expression().required()?;
                        let body = self.Block().required()?;
                        return Ok(Some(ast::Stmt::RangeStmt(ast::RangeStmt {
                            for_: for_.0,
                            key: None,
                            value: None,
                            tok_pos: Some(assign.0),
                            tok: Some(assign.1),
                            x,
                            body,
                        })));
                    }
                }
            }

            match tok {
                Some(tok) => Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                    lhs: exprs,
                    tok_pos: tok.0,
                    tok: tok.1,
                    rhs: self.ExpressionList().required()?,
                })),
                _ => return Err(ParserError::UnexpectedToken),
            }
        } else {
            self.SimpleStmt()?
        };

        // for a;b;c {}
        self.token(Token::SEMICOLON).required()?;
        let cond = self.Expression()?;
        self.token(Token::SEMICOLON).required()?;
        let post = self.SimpleStmt()?;
        let body = self.Block().required()?;
        Ok(Some(ast::Stmt::ForStmt(ast::ForStmt {
            for_: for_.0,
            init: init.map(Box::new),
            cond,
            post: post.map(Box::new),
            body,
        })))
    }

    // GoStmt = "go" Expression .
    fn GoStmt(&mut self) -> Result<Option<ast::GoStmt<'scanner>>> {
        log::debug!("Parser::GoStmt()");

        let go = match self.token(Token::GO)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let call = match self.Expression().required()? {
            ast::Expr::CallExpr(v) => v,
            _ => return Err(ParserError::UnexpectedToken),
        };

        Ok(Some(ast::GoStmt { go: go.0, call }))
    }

    // IfStmt = "if" [ SimpleStmt ";" ] Expression Block [ "else" ( IfStmt | Block ) ] .
    fn IfStmt(&mut self) -> Result<Option<ast::IfStmt<'scanner>>> {
        log::debug!("Parser::IfStmt()");

        let if_ = match self.token(Token::IF)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (init, cond) = if let Some(simple_stmt) = self.SimpleStmt()? {
            if self.token(Token::SEMICOLON)?.is_some() {
                (Some(simple_stmt), self.Expression().required()?)
            } else if let ast::Stmt::ExprStmt(expr_stmt) = simple_stmt {
                (None, expr_stmt.x)
            } else {
                return Err(ParserError::UnexpectedToken);
            }
        } else {
            (None, self.Expression().required()?)
        };

        let body = self.Block().required()?;

        let else_ = if self.token(Token::ELSE)?.is_some() {
            if let Some(if_stmt) = self.IfStmt()? {
                Some(ast::Stmt::IfStmt(if_stmt))
            } else if let Some(block_stmt) = self.Block()? {
                Some(ast::Stmt::BlockStmt(block_stmt))
            } else {
                return Err(ParserError::UnexpectedToken);
            }
        } else {
            None
        };

        Ok(Some(ast::IfStmt {
            if_: if_.0,
            init: Box::new(init),
            cond,
            body,
            else_: Box::new(else_),
        }))
    }

    // SimpleStmt     = EmptyStmt | ExpressionStmt | SendStmt | IncDecStmt | Assignment | ShortVarDecl .
    // ExpressionStmt = Expression .
    // IncDecStmt     = Expression ( "++" | "--" ) .
    // Assignment     = ExpressionList assign_op ExpressionList .
    // ShortVarDecl   = IdentifierList ":=" ExpressionList .
    // SendStmt = Channel "<-" Expression .
    // Channel  = Expression .
    fn SimpleStmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::SimpleStmt()");

        if let Some(mut exprs) = self.ExpressionList()? {
            // ShortVarDecl
            if exprs.iter().all(|expr| matches!(expr, ast::Expr::Ident(_))) {
                if let Some(define_op) = self.token(Token::DEFINE)? {
                    let rhs = self.ExpressionList().required()?;
                    return Ok(Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                        lhs: exprs,
                        tok_pos: define_op.0,
                        tok: define_op.1,
                        rhs,
                    })));
                }
            }

            // Assignment
            if let Some(assign_op) = self.assign_op()? {
                let rhs = self.ExpressionList().required()?;
                return Ok(Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                    lhs: exprs,
                    tok_pos: assign_op.0,
                    tok: assign_op.1,
                    rhs,
                })));
            }

            if exprs.len() == 1 {
                let expr = exprs.pop().unwrap();

                // IncDecStmt
                if let Some(inc) = self.token(Token::INC)? {
                    return Ok(Some(ast::Stmt::IncDecStmt(ast::IncDecStmt {
                        tok: inc.1,
                        tok_pos: inc.0,
                        x: expr,
                    })));
                }

                // IncDecStmt
                if let Some(dec) = self.token(Token::DEC)? {
                    return Ok(Some(ast::Stmt::IncDecStmt(ast::IncDecStmt {
                        tok: dec.1,
                        tok_pos: dec.0,
                        x: expr,
                    })));
                }

                // SendStmt
                if let Some(arrow) = self.token(Token::ARROW)? {
                    let value = self.Expression().required()?;
                    return Ok(Some(ast::Stmt::SendStmt(ast::SendStmt {
                        chan: expr,
                        arrow: arrow.0,
                        value,
                    })));
                }

                // ExpressionStmt
                return Ok(Some(ast::Stmt::ExprStmt(ast::ExprStmt { x: expr })));
            }

            return Err(ParserError::UnexpectedToken);
        }

        Ok(None)
    }

    // Arguments = "(" [ ( ExpressionList | Type [ "," ExpressionList ] ) [ "..." ] [ "," ] ] ")" .
    fn Arguments(
        &mut self,
    ) -> Result<
        Option<(
            Position<'scanner>,
            Vec<ast::Expr<'scanner>>,
            Position<'scanner>,
        )>,
    > {
        log::debug!("Parser::Arguments()");

        let lparen = match self.token(Token::LPAREN)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut args = if let Some(exprs) = self.ExpressionList()? {
            exprs
        } else if let Some(type_) = self.Type()? {
            vec![type_]
        } else {
            vec![]
        };

        if self.token(Token::COMMA)?.is_some() {
            let mut exprs = self.ExpressionList().required()?;
            args.append(&mut exprs);
        }

        let rparen = self.token(Token::RPAREN).required()?;

        Ok(Some((lparen.0, args, rparen.0)))
    }

    // ReturnStmt = "return" [ ExpressionList ] .
    fn ReturnStmt(&mut self) -> Result<Option<ast::ReturnStmt<'scanner>>> {
        log::debug!("Parser::ReturnStmt()");

        if let Some(return_) = self.token(Token::RETURN)? {
            let results = self.ExpressionList()?.unwrap_or_default();
            Ok(Some(ast::ReturnStmt {
                return_: return_.0,
                results,
            }))
        } else {
            Ok(None)
        }
    }

    // Receiver = Parameters .
    fn Receiver(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::Receiver()");

        self.Parameters()
    }

    /*
     * Intermediate productions (simplify/factorize code + help with ambiguous look-ahead)
     */

    // identifier | QualifiedIdent
    // QualifiedIdent = PackageName "." identifier .
    // PackageName    = identifier .
    fn identifier_or_QualifiedIdent(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::identifier_or_QualifiedIdent()");

        let ident = match self.identifier()? {
            Some(v) => v,
            None => return Ok(None),
        };

        if self.token(Token::PERIOD)?.is_some() {
            let sel = self.identifier().required()?;
            return Ok(Some(ast::Expr::SelectorExpr(ast::SelectorExpr {
                x: Box::new(ast::Expr::Ident(ident)),
                sel,
            })));
        }

        Ok(Some(ast::Expr::Ident(ident)))
    }

    // "." | PackageName
    fn period_or_PackageName(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::period_or_PackageName()");

        if let Some(period) = self.token(Token::PERIOD)? {
            return Ok(Some(ast::Ident {
                name_pos: period.0,
                name: ".",
                obj: None,
            }));
        }

        if let Some(package_name) = self.PackageName()? {
            return Ok(Some(package_name));
        }

        Ok(None)
    }

    // Operand | Conversion | MethodExpr
    fn Operand_or_Conversion_or_MethodExpr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Operand_or_Conversion_or_MethodExpr()");

        if let Some(operand) = self.Operand()? {
            return Ok(Some(operand));
        }

        Ok(None)
    }

    // FunctionDecl | MethodDecl
    // FunctionDecl = "func" FunctionName Signature [ FunctionBody ] .
    // MethodDecl   = "func" Receiver MethodName Signature [ FunctionBody ] .
    // FunctionName = identifier .
    // MethodName   = identifier .
    fn FunctionDecl_or_MethodDecl(&mut self) -> Result<Option<ast::FuncDecl<'scanner>>> {
        log::debug!("Parser::FunctionDecl_or_MethodDecl()");

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let recv = self.Receiver()?;

        let name = self.identifier().required()?;

        let type_ = self.Signature(Some(func.0)).required()?;

        let body = self.FunctionBody()?;

        Ok(Some(ast::FuncDecl {
            doc: None,
            recv,
            name,
            type_,
            body,
        }))
    }

    /*
     * Terminal productions (lexical tokens)
     */

    // assign_op = [ add_op | mul_op ] "=" .
    // add_op    = "+" | "-" | "|" | "^" .
    // mul_op    = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    fn assign_op(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::assign_op()");

        use Token::*;
        if let Some(current) = self.peek_token()? {
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

    // binary_op = "||" | "&&" | rel_op | add_op | mul_op .
    // rel_op    = "==" | "!=" | "<" | "<=" | ">" | ">=" .
    // add_op    = "+" | "-" | "|" | "^" .
    // mul_op    = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    fn peek_binary_op(&mut self, min_precedence: u8) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::binary_op()");

        if let Some(current) = self.peek_token()? {
            use Token::*;
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
            ) && current.1.precedence() >= min_precedence
            {
                return Ok(Some(current));
            }
        }

        Ok(None)
    }

    // unary_op = "+" | "-" | "!" | "^" | "*" | "&" | "<-" .
    fn unary_op(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::unary_op()");

        use Token::*;
        if let Some(current) = self.peek_token()? {
            if matches!(current.1, ADD | SUB | NOT | MUL | XOR | AND | ARROW) {
                self.next()?;
                return Ok(Some(current));
            }
        }

        Ok(None)
    }

    fn identifier(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::identifier()");

        self.token(Token::IDENT)?.map_or(Ok(None), |ident| {
            Ok(Some(ast::Ident {
                name_pos: ident.0,
                name: ident.2,
                obj: None,
            }))
        })
    }

    fn int_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::int_lit()");

        self.token(Token::INT)?.map_or(Ok(None), |int_lit| {
            Ok(Some(ast::BasicLit {
                kind: int_lit.1,
                value: int_lit.2,
                value_pos: int_lit.0,
            }))
        })
    }

    fn float_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::float_lit()");

        self.token(Token::FLOAT)?.map_or(Ok(None), |float_lit| {
            Ok(Some(ast::BasicLit {
                kind: float_lit.1,
                value: float_lit.2,
                value_pos: float_lit.0,
            }))
        })
    }

    fn imaginary_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::imaginary_lit()");

        self.token(Token::IMAG)?.map_or(Ok(None), |imag_lit| {
            Ok(Some(ast::BasicLit {
                kind: imag_lit.1,
                value: imag_lit.2,
                value_pos: imag_lit.0,
            }))
        })
    }

    fn rune_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::rune_lit()");

        self.token(Token::CHAR)?.map_or(Ok(None), |rune_lit| {
            Ok(Some(ast::BasicLit {
                kind: rune_lit.1,
                value: rune_lit.2,
                value_pos: rune_lit.0,
            }))
        })
    }

    fn string_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::string_lit()");

        self.token(Token::STRING)?.map_or(Ok(None), |string_lit| {
            Ok(Some(ast::BasicLit {
                value_pos: string_lit.0,
                kind: string_lit.1,
                value: string_lit.2,
            }))
        })
    }

    fn token(&mut self, expected: Token) -> Result<Option<scanner::Step<'scanner>>> {
        while let Some(current) = self.peek_token()? {
            if current.1 == expected {
                self.next()?;
                return Ok(Some(current));
            } else if current.1 == Token::COMMENT {
                self.next()?;
                continue;
            } else {
                return Ok(None);
            }
        }
        Err(ParserError::UnexpectedEndOfFile)
    }

    fn peek_token(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
        while let Some(current) = self.current {
            if current.1 != Token::COMMENT {
                break;
            }
            self.next()?;
        }
        Ok(self.current)
    }
}
