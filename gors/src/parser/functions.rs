use super::{RawParserError, Result, ResultExt, TypeParameterParse, core::Parser};
use crate::ast;
use crate::token::Token;

impl<'scanner> Parser<'scanner> {
    // Receiver = Parameters .
    pub(super) fn parse_receiver(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_receiver()");

        self.parse_parameters()
    }

    // identifier | QualifiedIdent
    // QualifiedIdent = PackageName "." identifier .
    // PackageName    = identifier .
    pub(super) fn parse_identifier_or_qualified_ident(
        &mut self,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_identifier_or_qualified_ident()");

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
    pub(super) fn parse_period_or_package_name(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::parse_period_or_package_name()");

        if let Some(period) = self.token(Token::PERIOD)? {
            return Ok(Some(ast::Ident {
                name_pos: period.0,
                name: ".",
                obj: None,
            }));
        }

        if let Some(package_name) = self.parse_package_name()? {
            return Ok(Some(package_name));
        }

        Ok(None)
    }

    // FunctionDecl | MethodDecl
    // FunctionDecl = "func" FunctionName [ TypeParameters ] Signature [ FunctionBody ] .
    // MethodDecl   = "func" Receiver MethodName Signature [ FunctionBody ] .
    // FunctionName = identifier .
    // MethodName   = identifier .
    pub(super) fn parse_function_decl_or_method_decl(
        &mut self,
    ) -> Result<Option<ast::FuncDecl<'scanner>>> {
        log::debug!("Parser::parse_function_decl_or_method_decl()");

        let doc = self.lead_comment.take();

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let recv = self.parse_receiver()?;
        let name = self.identifier().required()?;

        // Parse optional type parameters (Go 1.18+ generics)
        let type_params = match self.parse_type_parameters()? {
            TypeParameterParse::None => None,
            TypeParameterParse::TypeParameters(type_params) => Some(type_params),
            TypeParameterParse::ConsumedSlice { lbrack, rbrack } => Some(ast::FieldList {
                opening: Some(lbrack),
                list: Vec::new(),
                closing: Some(rbrack),
            }),
            TypeParameterParse::ConsumedArray {
                lbrack,
                rbrack,
                len,
            } => Some(ast::FieldList {
                opening: Some(lbrack),
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(len),
                    tag: None,
                    comment: None,
                }],
                closing: Some(rbrack),
            }),
        };

        let mut type_ = self.parse_signature(Some(func.0)).required()?;
        type_.type_params = type_params;

        let body = self.parse_function_body()?;

        Ok(Some(ast::FuncDecl {
            doc,
            recv,
            name,
            type_,
            body,
        }))
    }

    // TypeParameters = "[" TypeParamList [ "," ] "]" .
    // TypeParamList  = TypeParamDecl { "," TypeParamDecl } .
    // TypeParamDecl  = IdentifierList TypeConstraint .
    //
    // Distinguishes between type parameters and bracketed type prefixes:
    // - [T any]      -> type parameters
    // - []int        -> consumed slice prefix
    // - [5]int       -> consumed array prefix
    pub(super) fn parse_type_parameters(&mut self) -> Result<TypeParameterParse<'scanner>> {
        log::debug!("Parser::parse_type_parameters()");

        // Must start with [
        if self.current_step.1 != Token::LBRACK {
            return Ok(TypeParameterParse::None);
        }

        // TypeParameters require at least one TypeParamDecl which starts with an identifier
        // If [ is immediately followed by ] (slice) or a non-identifier (array expression),
        // this is not type parameters.
        // We need to NOT consume [ if this isn't type parameters.
        // Since we can't peek 2 tokens ahead easily, we'll consume [ and then
        // check if the first thing we see is an identifier.

        let lbrack = self.token(Token::LBRACK).required()?;

        // If immediately followed by ], this is a slice type, not type params
        if self.current_step.1 == Token::RBRACK {
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(TypeParameterParse::ConsumedSlice {
                lbrack: lbrack.0,
                rbrack: rbrack.0,
            });
        }

        // If not followed by identifier, this is not type parameters (could be array type [5]int)
        if self.current_step.1 != Token::IDENT {
            // Handle [...]Type (auto-sized array)
            let len = if let Some(ellipsis) = self.token(Token::ELLIPSIS)? {
                ast::Expr::Ellipsis(ast::Ellipsis {
                    ellipsis: ellipsis.0,
                    elt: None,
                })
            } else {
                self.parse_expression().required()?
            };
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(TypeParameterParse::ConsumedArray {
                lbrack: lbrack.0,
                rbrack: rbrack.0,
                len,
            });
        }

        // We have [ident... - need to distinguish between:
        // - [ident] or [pkg.Const + other] for array type (expression as length)
        // - [ident constraint] for type parameters
        //
        // Following Go's parser: parse ident as start of expression, then analyze.
        let first_ident = self.identifier().required()?;

        // If followed by [, this could be a slice/array constraint [P []E]
        // which we handle separately since parsing it as expression would fail
        if self.current_step.1 == Token::LBRACK {
            // Fall through to type parameter parsing below
        } else if self.current_step.1 == Token::RBRACK {
            // [ident] — array type with single ident as length
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(TypeParameterParse::ConsumedArray {
                lbrack: lbrack.0,
                rbrack: rbrack.0,
                len: ast::Expr::Ident(first_ident),
            });
        } else {
            // Parse the rest of the expression starting from first_ident.
            // Following Go's parser: parse full expression, then determine
            // if it's a type parameter list or array length.
            //
            // If the next token can continue a primary expression (.sel, (args), etc.)
            // or is a binary operator, parse the full expression — it's an array length.
            // Otherwise it's still just an identifier, which might be a type param name.
            // These tokens unambiguously continue an expression (not a type constraint).
            // Notably MUL (*) and OR (|) are excluded: * could be pointer constraint,
            // | could be union type.
            let is_expr_continuation = matches!(
                self.current_step.1,
                Token::PERIOD
                    | Token::LPAREN
                    | Token::ADD
                    | Token::SUB
                    | Token::QUO
                    | Token::REM
                    | Token::AND
                    | Token::XOR
                    | Token::SHL
                    | Token::SHR
                    | Token::AND_NOT
                    | Token::LAND
                    | Token::LOR
                    | Token::EQL
                    | Token::LSS
                    | Token::GTR
                    | Token::NEQ
                    | Token::LEQ
                    | Token::GEQ
            );

            if is_expr_continuation {
                self.expr_level += 1;
                let lhs = self.continue_primary_expr(ast::Expr::Ident(first_ident))?;
                let x = self
                    .expression(lhs, Token::lowest_precedence())
                    .required()?;
                self.expr_level -= 1;

                // Check if this could be type params: CallExpr(name, type) → name (type)
                // This handles cases like [P (*int)] which parses as call P(*int)
                let x = if let ast::Expr::CallExpr(mut call) = x {
                    let is_type_param = matches!(*call.fun, ast::Expr::Ident(_))
                        && call.args.as_ref().is_some_and(|a| a.len() == 1)
                        && call.ellipsis.is_none()
                        && self.current_step.1 != Token::RBRACK;

                    if is_type_param {
                        if let ast::Expr::Ident(name) = *call.fun {
                            let Some(arg) = call.args.take().and_then(|mut a| a.pop()) else {
                                return Err(RawParserError::UnexpectedToken);
                            };
                            let constraint = ast::Expr::ParenExpr(ast::ParenExpr {
                                lparen: call.lparen,
                                x: Box::new(arg),
                                rparen: call.rparen,
                            });
                            let field = ast::Field {
                                doc: None,
                                names: Some(vec![name]),
                                type_: Some(constraint),
                                tag: None,
                                comment: None,
                            };
                            let mut fields = vec![field];
                            while self.token(Token::COMMA)?.is_some() {
                                if self.current_step.1 == Token::RBRACK {
                                    break;
                                }
                                fields.push(self.parse_type_param_decl().required()?);
                            }
                            let rbrack = self.token(Token::RBRACK).required()?;
                            return Ok(TypeParameterParse::TypeParameters(ast::FieldList {
                                opening: Some(lbrack.0),
                                list: fields,
                                closing: Some(rbrack.0),
                            }));
                        }
                        return Err(RawParserError::UnexpectedToken);
                    } else {
                        ast::Expr::CallExpr(call)
                    }
                } else {
                    x
                };

                let rbrack = self.token(Token::RBRACK).required()?;
                return Ok(TypeParameterParse::ConsumedArray {
                    lbrack: lbrack.0,
                    rbrack: rbrack.0,
                    len: x,
                });
            }
            // Otherwise first_ident is just a name — fall through to type param parsing
        }

        // Special handling for * which could be a pointer type constraint or multiplication
        // [T *S] is a type parameter T with pointer constraint *S
        // [n * 5] is an array length expression (multiplication)
        if self.current_step.1 == Token::MUL {
            // Peek ahead to determine interpretation
            let star_pos = self.current_step.0;
            self.next()?; // consume *

            // If followed by a number literal, it's definitely multiplication
            if matches!(self.current_step.1, Token::INT | Token::FLOAT) {
                // Parse as multiplication expression
                let right = self.parse_unary_expr().required()?;
                let binary_expr = ast::Expr::BinaryExpr(ast::BinaryExpr {
                    x: Box::new(ast::Expr::Ident(first_ident)),
                    op_pos: star_pos,
                    op: Token::MUL,
                    y: Box::new(right),
                });
                // Continue parsing any remaining binary operators
                let len_expr = self
                    .expression(binary_expr, Token::lowest_precedence())
                    .required()?;
                let rbrack = self.token(Token::RBRACK).required()?;
                return Ok(TypeParameterParse::ConsumedArray {
                    lbrack: lbrack.0,
                    rbrack: rbrack.0,
                    len: len_expr,
                });
            }

            // Otherwise, this is a pointer type constraint
            // Parse the type that * points to
            let pointed_type = self.parse_type().required()?;
            let pointer_constraint = ast::Expr::StarExpr(ast::StarExpr {
                star: star_pos,
                x: Box::new(pointed_type),
            });

            // Handle union types: [T *S | *R]
            let mut constraint = pointer_constraint;
            while let Some(or_tok) = self.token(Token::OR)? {
                let next_term = self.parse_type_term().required()?;
                constraint = ast::Expr::BinaryExpr(ast::BinaryExpr {
                    x: Box::new(constraint),
                    op_pos: or_tok.0,
                    op: Token::OR,
                    y: Box::new(next_term),
                });
            }

            // Create the type parameter field
            let field = ast::Field {
                doc: None,
                names: Some(vec![first_ident]),
                type_: Some(constraint),
                tag: None,
                comment: None,
            };

            let mut fields = vec![field];

            // Parse additional type parameter declarations
            while self.token(Token::COMMA)?.is_some() {
                if self.current_step.1 == Token::RBRACK {
                    break;
                }
                fields.push(self.parse_type_param_decl().required()?);
            }

            let rbrack = self.token(Token::RBRACK).required()?;

            return Ok(TypeParameterParse::TypeParameters(ast::FieldList {
                opening: Some(lbrack.0),
                list: fields,
                closing: Some(rbrack.0),
            }));
        }

        // If followed by a binary operator (like / + -), this is an array type [ident op expr]
        // where the length is a binary expression
        // Note: MUL (*) is handled specially above to distinguish pointer types from multiplication
        if matches!(
            self.current_step.1,
            Token::ADD
                | Token::SUB
                | Token::QUO
                | Token::REM
                | Token::AND
                | Token::OR
                | Token::XOR
                | Token::SHL
                | Token::SHR
                | Token::AND_NOT
                | Token::LOR
                | Token::LAND
                | Token::EQL
                | Token::NEQ
                | Token::LSS
                | Token::GTR
                | Token::LEQ
                | Token::GEQ
        ) {
            // We need to continue parsing this as a binary expression
            // The first_ident becomes the left operand
            let left = ast::Expr::Ident(first_ident);
            // Parse the rest of the expression using binary expression parsing
            let len_expr = self
                .expression(left, Token::lowest_precedence())
                .required()?;
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(TypeParameterParse::ConsumedArray {
                lbrack: lbrack.0,
                rbrack: rbrack.0,
                len: len_expr,
            });
        }

        // If followed by comma, could be multiple idents like [T, U any]
        // If followed by type constraint, it's type parameters [T any]
        // For now, assume type parameters and parse accordingly
        let mut names = vec![first_ident];

        // Check for more identifiers (like T, U in [T, U any])
        while self.current_step.1 == Token::COMMA {
            self.token(Token::COMMA)?;
            // If immediately followed by ], this was a trailing comma
            if self.current_step.1 == Token::RBRACK {
                break;
            }
            // Check if this is another identifier (for type params) or something else
            if self.current_step.1 == Token::IDENT {
                names.push(self.identifier().required()?);
            } else {
                break;
            }
        }

        // Try to parse the constraint
        let constraint = match self.parse_type_constraint()? {
            Some(c) => c,
            None => {
                // No constraint found - this might be an array type [T] where T is a type
                // But we already handled [ident] above, so this is an error or
                // partial type parameter. For now, treat single ident without constraint
                // as type parameter with inferred 'any' constraint (Go 1.18 behavior)
                ast::Expr::Ident(ast::Ident {
                    name_pos: names.first().map(|name| name.name_pos).unwrap_or_default(),
                    name: "any",
                    obj: None,
                })
            }
        };

        let mut fields = vec![ast::Field {
            doc: None,
            names: Some(names),
            type_: Some(constraint),
            tag: None,
            comment: None,
        }];

        // Parse additional type parameter declarations
        while self.token(Token::COMMA)?.is_some() {
            // Allow trailing comma
            if self.current_step.1 == Token::RBRACK {
                break;
            }
            fields.push(self.parse_type_param_decl().required()?);

            // Parse additional type parameter declarations
            while self.token(Token::COMMA)?.is_some() {
                // Allow trailing comma
                if self.current_step.1 == Token::RBRACK {
                    break;
                }
                fields.push(self.parse_type_param_decl().required()?);
            }
        }

        let rbrack = self.token(Token::RBRACK).required()?;

        Ok(TypeParameterParse::TypeParameters(ast::FieldList {
            opening: Some(lbrack.0),
            list: fields,
            closing: Some(rbrack.0),
        }))
    }

    // TypeParamDecl = IdentifierList TypeConstraint .
    // TypeConstraint = TypeElem .
    pub(super) fn parse_type_param_decl(&mut self) -> Result<Option<ast::Field<'scanner>>> {
        log::debug!("Parser::parse_type_param_decl()");

        let (names, _, _) = match self.parse_identifier_list()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Parse the constraint (which can be a union type)
        let constraint = self.parse_type_constraint().required()?;

        Ok(Some(ast::Field {
            doc: None,
            names: Some(names),
            type_: Some(constraint),
            tag: None,
            comment: None,
        }))
    }

    // TypeConstraint = TypeElem .
    // TypeElem = TypeTerm { "|" TypeTerm } .
    pub(super) fn parse_type_constraint(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_type_constraint()");

        // Parse the first type term
        let first = match self.parse_type_term()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Check for union types (Type1 | Type2 | ...)
        let mut type_elem = first;
        while let Some(or_tok) = self.token(Token::OR)? {
            let next_term = self.parse_type_term().required()?;
            type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                x: Box::new(type_elem),
                op_pos: or_tok.0,
                op: Token::OR,
                y: Box::new(next_term),
            });
        }

        Ok(Some(type_elem))
    }

    // TypeTerm = Type | UnderlyingType .
    // UnderlyingType = "~" Type .
    pub(super) fn parse_type_term(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_type_term()");

        // Check for underlying type constraint (~Type)
        if let Some(tilde) = self.token(Token::TILDE)? {
            let type_ = self.parse_type_with_instantiation().required()?;
            return Ok(Some(ast::Expr::UnaryExpr(ast::UnaryExpr {
                op_pos: tilde.0,
                op: Token::TILDE,
                x: Box::new(type_),
            })));
        }

        self.parse_type_with_instantiation()
    }

    // TypeWithInstantiation = Type [ TypeArgs ] .
    // TypeArgs = "[" TypeList [ "," ] "]" .
    // This handles generic type instantiation like Comparable[T] or _SliceOf[E]
    pub(super) fn parse_type_with_instantiation(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_type_with_instantiation()");

        let type_ = match self.parse_type()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Check for type instantiation [T] or [T1, T2]
        if self.current_step.1 == Token::LBRACK {
            let lbrack = self.token(Token::LBRACK).required()?;
            let mut indices = vec![self.parse_type().required()?];
            while self.token(Token::COMMA)?.is_some() {
                if self.current_step.1 == Token::RBRACK {
                    break;
                }
                indices.push(self.parse_type().required()?);
            }
            let rbrack = self.token(Token::RBRACK).required()?;

            if let Some(index) = (indices.len() == 1).then(|| indices.pop()).flatten() {
                return Ok(Some(ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(type_),
                    lbrack: lbrack.0,
                    index: Box::new(index),
                    rbrack: rbrack.0,
                })));
            } else {
                return Ok(Some(ast::Expr::IndexListExpr(ast::IndexListExpr {
                    x: Box::new(type_),
                    lbrack: lbrack.0,
                    indices,
                    rbrack: rbrack.0,
                })));
            }
        }

        Ok(Some(type_))
    }

    pub(super) fn parse_optional_type_instance(
        &mut self,
        type_: ast::Expr<'scanner>,
    ) -> Result<ast::Expr<'scanner>> {
        if self.current_step.1 != Token::LBRACK {
            return Ok(type_);
        }
        let lbrack = self.token(Token::LBRACK).required()?;
        let mut indices = vec![self.parse_type().required()?];
        while self.token(Token::COMMA)?.is_some() {
            if self.current_step.1 == Token::RBRACK {
                break;
            }
            indices.push(self.parse_type().required()?);
        }
        let rbrack = self.token(Token::RBRACK).required()?;

        if let Some(index) = (indices.len() == 1).then(|| indices.pop()).flatten() {
            Ok(ast::Expr::IndexExpr(ast::IndexExpr {
                x: Box::new(type_),
                lbrack: lbrack.0,
                index: Box::new(index),
                rbrack: rbrack.0,
            }))
        } else {
            Ok(ast::Expr::IndexListExpr(ast::IndexListExpr {
                x: Box::new(type_),
                lbrack: lbrack.0,
                indices,
                rbrack: rbrack.0,
            }))
        }
    }

    pub(super) fn parse_embedded_elem(
        &mut self,
        mut type_elem: ast::Expr<'scanner>,
    ) -> Result<ast::Expr<'scanner>> {
        while let Some(or_tok) = self.token(Token::OR)? {
            let next_term = self.parse_type_term().required()?;
            type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                x: Box::new(type_elem),
                op_pos: or_tok.0,
                op: Token::OR,
                y: Box::new(next_term),
            });
        }
        Ok(type_elem)
    }
}
