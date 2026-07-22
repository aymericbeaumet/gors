use super::{Result, ResultExt, TypeParameterParse, core::Parser};
use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};

impl<'scanner> Parser<'scanner> {
    pub(super) fn parse_source_file(&mut self) -> Result<Option<ast::File<'scanner>>> {
        log::debug!("Parser::parse_source_file()");

        let doc = self.lead_comment.take();

        let (package, package_name) = match self.parse_package_clause()? {
            Some(v) => v,
            None => return Ok(None),
        };

        self.token(Token::SEMICOLON).required()?;

        let file_start = Position {
            origin: self.original_origin,
            offset: 0,
            line: 1,
            column: 1,
        };

        let mut out = ast::File {
            doc,
            package: package.0,
            name: package_name,
            decls: vec![],
            file_start,
            file_end: file_start,
            scope: None,
            unresolved: vec![],
            comments: vec![],
            go_version: self.go_version,
        };

        while let Some(mut import_decl) = self.parse_import_decl()? {
            self.token(Token::SEMICOLON).required()?;
            if import_decl.lparen.is_none() {
                if let Some(comment) = self.line_comment.take() {
                    if let Some(ast::Spec::ImportSpec(s)) = import_decl.specs.last_mut() {
                        if s.comment.is_none() {
                            s.comment = Some(comment);
                        }
                    }
                }
            }
            out.decls.push(ast::Decl::GenDecl(import_decl));
        }

        while let Some(mut top_level_decl) = self.parse_top_level_decl()? {
            self.token(Token::SEMICOLON).required()?;
            if let Some(comment) = self.line_comment.take() {
                if let ast::Decl::GenDecl(gen_decl) = &mut top_level_decl {
                    if gen_decl.lparen.is_none() {
                        if let Some(spec) = gen_decl.specs.last_mut() {
                            let existing = match spec {
                                ast::Spec::ValueSpec(s) => &mut s.comment,
                                ast::Spec::TypeSpec(s) => &mut s.comment,
                                ast::Spec::ImportSpec(s) => &mut s.comment,
                            };
                            if existing.is_none() {
                                *existing = Some(comment);
                            }
                        }
                    }
                }
            }
            out.decls.push(top_level_decl);
        }

        let eof = self.token(Token::EOF).required()?;
        out.file_end = eof.0;

        out.comments = std::mem::take(&mut self.all_comments);

        Ok(Some(out))
    }

    // PackageClause = "package" PackageName .
    pub(super) fn parse_package_clause(
        &mut self,
    ) -> Result<Option<(scanner::Step<'scanner>, ast::Ident<'scanner>)>> {
        log::debug!("Parser::parse_package_clause()");

        let package = match self.token(Token::PACKAGE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let package_name = self.parse_package_name().required()?;

        Ok(Some((package, package_name)))
    }

    // PackageName = identifier .
    pub(super) fn parse_package_name(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::parse_package_name()");

        self.identifier()
    }

    // ImportDecl = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    pub(super) fn parse_import_decl(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::parse_import_decl()");

        if self.current_step.1 != Token::IMPORT {
            return Ok(None);
        }

        let doc = self.lead_comment.take();

        let import = self.token(Token::IMPORT).required()?;

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            loop {
                let spec_doc = self.lead_comment.take();
                if let Some(mut import_spec) = self.parse_import_spec()? {
                    import_spec.doc = spec_doc;
                    let pre_semi_comment = self.line_comment.take();
                    if self.token(Token::SEMICOLON)?.is_some() {
                        import_spec.comment = self.line_comment.take().or(pre_semi_comment);
                        specs.push(ast::Spec::ImportSpec(import_spec));
                    } else {
                        import_spec.comment = pre_semi_comment;
                        specs.push(ast::Spec::ImportSpec(import_spec));
                        break;
                    }
                } else {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc,
                tok_pos: import.0,
                tok: import.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let mut import_spec = self.parse_import_spec().required()?;
        import_spec.comment = self.line_comment.take();
        let specs = vec![ast::Spec::ImportSpec(import_spec)];
        Ok(Some(ast::GenDecl {
            doc,
            tok_pos: import.0,
            tok: import.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // ImportSpec = [ "." | PackageName ] ImportPath .
    pub(super) fn parse_import_spec(&mut self) -> Result<Option<ast::ImportSpec<'scanner>>> {
        log::debug!("Parser::parse_import_spec()");

        if let Some(name) = self.parse_period_or_package_name()? {
            let path = self.parse_import_path().required()?;
            return Ok(Some(ast::ImportSpec {
                doc: None,
                name: Some(name),
                path,
                comment: None,
            }));
        }

        let import_path = match self.parse_import_path()? {
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
    pub(super) fn parse_import_path(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::parse_import_path()");

        self.string_lit()
    }

    // TopLevelDecl = Declaration | FunctionDecl | MethodDecl .
    pub(super) fn parse_top_level_decl(&mut self) -> Result<Option<ast::Decl<'scanner>>> {
        log::debug!("Parser::parse_top_level_decl()");

        use Token::*;
        Ok(match self.current_step.1 {
            CONST | TYPE | VAR => Some(ast::Decl::GenDecl(self.parse_declaration().required()?)),
            FUNC => Some(ast::Decl::FuncDecl(
                self.parse_function_decl_or_method_decl().required()?,
            )),
            _ => None,
        })
    }

    // Declaration = ConstDecl | TypeDecl | VarDecl .
    pub(super) fn parse_declaration(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::parse_declaration()");

        let doc = self.lead_comment.take();

        Ok(match self.current_step.1 {
            Token::CONST => Some(self.parse_const_decl_with_doc(doc).required()?),
            Token::TYPE => Some(self.parse_type_decl_with_doc(doc).required()?),
            Token::VAR => Some(self.parse_var_decl_with_doc(doc).required()?),
            _ => None,
        })
    }

    // TypeDecl = "type" ( TypeSpec | "(" { TypeSpec ";" } ")" ) .
    pub(super) fn parse_type_decl_with_doc(
        &mut self,
        doc: Option<ast::CommentGroup<'scanner>>,
    ) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::parse_type_decl_with_doc()");

        let type_ = match self.token(Token::TYPE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            loop {
                let spec_doc = self.lead_comment.take();
                if let Some(mut type_spec) = self.parse_type_spec()? {
                    type_spec.doc = spec_doc;
                    let pre_semi_comment = self.line_comment.take();
                    if self.token(Token::SEMICOLON)?.is_some() {
                        type_spec.comment = self.line_comment.take().or(pre_semi_comment);
                        specs.push(ast::Spec::TypeSpec(type_spec));
                    } else {
                        type_spec.comment = pre_semi_comment;
                        specs.push(ast::Spec::TypeSpec(type_spec));
                        break;
                    }
                } else {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc,
                tok_pos: type_.0,
                tok: type_.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let mut type_spec = self.parse_type_spec().required()?;
        type_spec.comment = self.line_comment.take();
        let specs = vec![ast::Spec::TypeSpec(type_spec)];
        Ok(Some(ast::GenDecl {
            doc,
            tok_pos: type_.0,
            tok: type_.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // TypeSpec  = AliasDecl | TypeDef .
    // AliasDecl = identifier "=" Type .
    // TypeDef   = identifier [ TypeParameters ] Type .
    pub(super) fn parse_type_spec(&mut self) -> Result<Option<ast::TypeSpec<'scanner>>> {
        log::debug!("Parser::parse_type_spec()");

        let name = match self.identifier()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Parse optional type parameters (Go 1.18+ generics)
        // Only try to parse type params if [ is followed by an identifier (not ] for slice)
        let type_params = if self.current_step.1 == Token::LBRACK {
            // Need to distinguish between:
            // - type Foo[T any] ... (type parameters)
            // - type Foo []int     (slice type - [ immediately followed by ])
            // - type Foo [5]int    (array type - [ followed by expression)
            // Type parameters always have: [ identifier constraint ]
            // So we look for [ followed by identifier
            match self.parse_type_parameters()? {
                TypeParameterParse::ConsumedSlice { lbrack, .. } => {
                    // This was [] - it's a slice type, not type params.
                    let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
                    let element_type = self.parse_type().required()?;
                    return Ok(Some(ast::TypeSpec {
                        doc: None,
                        name: Some(name),
                        type_params: None,
                        assign,
                        type_: ast::Expr::ArrayType(ast::ArrayType {
                            lbrack,
                            len: None, // slice type has no length
                            elt: Box::new(element_type),
                        }),
                        comment: None,
                    }));
                }
                TypeParameterParse::ConsumedArray { lbrack, len, .. } => {
                    // This was [expr] - it's an array type, not type params.
                    let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
                    let element_type = self.parse_type().required()?;
                    return Ok(Some(ast::TypeSpec {
                        doc: None,
                        name: Some(name),
                        type_params: None,
                        assign,
                        type_: ast::Expr::ArrayType(ast::ArrayType {
                            lbrack,
                            len: Some(Box::new(len)),
                            elt: Box::new(element_type),
                        }),
                        comment: None,
                    }));
                }
                TypeParameterParse::TypeParameters(field_list)
                    if field_list.list.len() == 1
                        && field_list
                            .list
                            .first()
                            .is_some_and(|field| field.names.is_some())
                        && matches!(
                            field_list.list.first().and_then(|field| field.type_.as_ref()),
                            Some(ast::Expr::StarExpr(s)) if matches!(*s.x, ast::Expr::Ident(_))
                        )
                        && !matches!(self.current_step.1, Token::ASSIGN)
                        && self
                            .buffer
                            .get(
                                field_list.opening.map_or(0, |p| p.offset)
                                    ..field_list.closing.map_or(0, |p| p.offset),
                            )
                            .is_some_and(|text| !text.contains(',')) =>
                {
                    // [Name * Ident]Type without comma — reinterpret * as multiplication.
                    if let Some(elt) = self.parse_type()? {
                        let f = field_list.list.into_iter().next();
                        let Some(ast::Field {
                            names: Some(mut names),
                            type_: Some(ast::Expr::StarExpr(star_expr)),
                            ..
                        }) = f
                        else {
                            return Ok(Some(ast::TypeSpec {
                                doc: None,
                                name: Some(name),
                                type_params: None,
                                assign: None,
                                type_: elt,
                                comment: None,
                            }));
                        };
                        let name_ident = names.pop();
                        let len_expr = if let Some(lhs) = name_ident {
                            ast::Expr::BinaryExpr(ast::BinaryExpr {
                                x: Box::new(ast::Expr::Ident(lhs)),
                                op_pos: star_expr.star,
                                op: Token::MUL,
                                y: star_expr.x,
                            })
                        } else {
                            *star_expr.x
                        };
                        let lbrack = field_list.opening.unwrap_or_default();
                        return Ok(Some(ast::TypeSpec {
                            doc: None,
                            name: Some(name),
                            type_params: None,
                            assign: None,
                            type_: ast::Expr::ArrayType(ast::ArrayType {
                                lbrack,
                                len: Some(Box::new(len_expr)),
                                elt: Box::new(elt),
                            }),
                            comment: None,
                        }));
                    }
                    Some(field_list)
                }
                TypeParameterParse::TypeParameters(field_list) => Some(field_list),
                TypeParameterParse::None => None,
            }
        } else {
            None
        };

        let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);

        let type_ = self.parse_type().required()?;

        Ok(Some(ast::TypeSpec {
            doc: None,
            name: Some(name),
            type_params,
            assign,
            type_,
            comment: None,
        }))
    }

    // ConstDecl = "const" ( ConstSpec | "(" { ConstSpec ";" } ")" ) .
    pub(super) fn parse_const_decl_with_doc(
        &mut self,
        doc: Option<ast::CommentGroup<'scanner>>,
    ) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::parse_const_decl_with_doc()");

        let const_ = match self.token(Token::CONST)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            loop {
                let spec_doc = self.lead_comment.take();
                if let Some(mut const_spec) = self.parse_const_spec()? {
                    const_spec.doc = spec_doc;
                    let pre_semi_comment = self.line_comment.take();
                    if self.token(Token::SEMICOLON)?.is_some() {
                        const_spec.comment = self.line_comment.take().or(pre_semi_comment);
                        specs.push(ast::Spec::ValueSpec(const_spec));
                    } else {
                        const_spec.comment = pre_semi_comment;
                        specs.push(ast::Spec::ValueSpec(const_spec));
                        break;
                    }
                } else {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc,
                tok_pos: const_.0,
                tok: const_.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let mut const_spec = self.parse_const_spec().required()?;
        const_spec.comment = self.line_comment.take();
        let specs = vec![ast::Spec::ValueSpec(const_spec)];
        Ok(Some(ast::GenDecl {
            doc,
            tok_pos: const_.0,
            tok: const_.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // ConstSpec = IdentifierList [ [ Type ] "=" ExpressionList ] .
    pub(super) fn parse_const_spec(&mut self) -> Result<Option<ast::ValueSpec<'scanner>>> {
        log::debug!("Parser::parse_const_spec()");

        let (names, _, _) = match self.parse_identifier_list()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, Some(self.parse_expression_list().required()?))
        } else if let Some(type_) = self.parse_type()? {
            if self.token(Token::ASSIGN)?.is_some() {
                (Some(type_), Some(self.parse_expression_list().required()?))
            } else {
                (Some(type_), None)
            }
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
    pub(super) fn parse_var_decl_with_doc(
        &mut self,
        doc: Option<ast::CommentGroup<'scanner>>,
    ) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::parse_var_decl_with_doc()");

        let var = match self.token(Token::VAR)? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let mut specs = vec![];
            loop {
                let spec_doc = self.lead_comment.take();
                if let Some(mut var_spec) = self.parse_var_spec()? {
                    var_spec.doc = spec_doc;
                    let pre_semi_comment = self.line_comment.take();
                    if self.token(Token::SEMICOLON)?.is_some() {
                        var_spec.comment = self.line_comment.take().or(pre_semi_comment);
                        specs.push(ast::Spec::ValueSpec(var_spec));
                    } else {
                        var_spec.comment = pre_semi_comment;
                        specs.push(ast::Spec::ValueSpec(var_spec));
                        break;
                    }
                } else {
                    break;
                }
            }

            let rparen = self.token(Token::RPAREN).required()?;

            return Ok(Some(ast::GenDecl {
                doc,
                tok_pos: var.0,
                tok: var.1,
                lparen: Some(lparen.0),
                specs,
                rparen: Some(rparen.0),
            }));
        }

        let mut var_spec = self.parse_var_spec().required()?;
        var_spec.comment = self.line_comment.take();
        let specs = vec![ast::Spec::ValueSpec(var_spec)];
        Ok(Some(ast::GenDecl {
            doc,
            tok_pos: var.0,
            tok: var.1,
            lparen: None,
            specs,
            rparen: None,
        }))
    }

    // VarSpec = IdentifierList ( Type [ "=" ExpressionList ] | "=" ExpressionList ) .
    pub(super) fn parse_var_spec(&mut self) -> Result<Option<ast::ValueSpec<'scanner>>> {
        log::debug!("Parser::parse_var_spec()");

        let (names, _, _) = match self.parse_identifier_list()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, Some(self.parse_expression_list().required()?))
        } else {
            (
                Some(self.parse_type().required()?),
                if self.token(Token::ASSIGN)?.is_some() {
                    Some(self.parse_expression_list().required()?)
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
    // Returns (identifiers, has_trailing_comma, last_is_qualified) where:
    // - has_trailing_comma is true if a comma was consumed but no identifier followed (e.g., "int," in "(int, map[...])")
    // - last_is_qualified is true if the last identifier is followed by "." (making it a qualified type)
    pub(super) fn parse_identifier_list(
        &mut self,
    ) -> Result<Option<(Vec<ast::Ident<'scanner>>, bool, bool)>> {
        log::debug!("Parser::parse_identifier_list()");

        let first_ident = match self.identifier()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // If the first identifier is followed by a period, it's a qualified type name
        // (like pkg.Type), not a simple identifier. Return it as a single item
        // and let the caller handle it as a type.
        if self.current_step.1 == Token::PERIOD {
            return Ok(Some((vec![first_ident], false, true)));
        }

        let mut out = vec![first_ident];

        while self.token(Token::COMMA)?.is_some() {
            // After consuming comma, try to parse an identifier
            // If it fails, we've consumed too much - this is a type list like (int, map[...])
            // In this case, ParameterList will handle the remaining types
            if let Some(ident) = self.identifier()? {
                // Check if this identifier is followed by a period (qualified type)
                // If so, return with last_is_qualified=true so caller can handle it
                if self.current_step.1 == Token::PERIOD {
                    out.push(ident);
                    return Ok(Some((out, true, true)));
                }
                out.push(ident);
            } else {
                // Not an identifier after comma. The comma is already consumed,
                // so we return what we have with trailing_comma=true
                return Ok(Some((out, true, false)));
            }
        }

        Ok(Some((out, false, false)))
    }
}
