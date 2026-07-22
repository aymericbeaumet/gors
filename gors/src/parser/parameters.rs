use super::{Result, ResultExt, core::Parser};
use crate::ast;
use crate::token::Token;

impl<'scanner> Parser<'scanner> {
    // ParameterList = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl = [ IdentifierList ] [ "..." ] Type .
    pub(super) fn parse_parameter_list(&mut self) -> Result<Option<Vec<ast::Field<'scanner>>>> {
        log::debug!("Parser::parse_parameter_list()");

        // First, try to parse identifiers
        let idents_result = self.parse_identifier_list()?;

        // If no identifiers, try to parse just a type (unnamed parameter like "*T" or "interface{}")
        if idents_result.is_none() {
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.parse_type()?;
            if let Some(type_) = type_ {
                let field_type = if let Some(ellipsis) = ellipsis {
                    ast::Expr::Ellipsis(ast::Ellipsis {
                        ellipsis: ellipsis.0,
                        elt: Some(Box::new(type_)),
                    })
                } else {
                    type_
                };
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(field_type),
                    tag: None,
                    comment: None,
                }];
                // Parse more unnamed parameters
                self.parse_unnamed_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }
            return Ok(None);
        }

        let Some((idents, has_trailing_comma, last_is_qualified)) = idents_result else {
            return Ok(None);
        };

        // If IdentifierList consumed a trailing comma (e.g., "int," in "(int, map[...])"),
        // then all idents are types and we should parse remaining types
        if has_trailing_comma {
            // If the last identifier is followed by ".", it's a qualified type (e.g., "schema.T")
            // We need to handle this specially by completing the qualified type
            if last_is_qualified {
                let mut fields: Vec<ast::Field<'scanner>> = Vec::new();
                let mut idents_iter = idents.into_iter().peekable();

                // Convert all but the last ident to simple types
                while let Some(ident) = idents_iter.next() {
                    if idents_iter.peek().is_none() {
                        // This is the last ident - it's followed by "." so complete the qualified type
                        self.token(Token::PERIOD)?;
                        let sel = self.identifier().required()?;
                        fields.push(ast::Field {
                            doc: None,
                            names: None,
                            type_: Some(ast::Expr::SelectorExpr(ast::SelectorExpr {
                                x: Box::new(ast::Expr::Ident(ident)),
                                sel,
                            })),
                            tag: None,
                            comment: None,
                        });
                    } else {
                        fields.push(ast::Field {
                            doc: None,
                            names: None,
                            type_: Some(ast::Expr::Ident(ident)),
                            tag: None,
                            comment: None,
                        });
                    }
                }

                // Parse remaining types after comma
                self.parse_plain_unnamed_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }

            let mut fields: Vec<ast::Field<'scanner>> = idents
                .into_iter()
                .map(|ident| ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(ast::Expr::Ident(ident)),
                    tag: None,
                    comment: None,
                })
                .collect();
            // The trailing comma was already consumed by IdentifierList.
            // Check if there's more to parse after that comma (the comma wasn't truly trailing)
            // by checking if we can parse another type
            if self.current_step.1 != Token::RPAREN {
                // Parse the type that comes after the consumed comma (may be variadic like ...string)
                let ellipsis = self.token(Token::ELLIPSIS)?;
                let type_ = self.parse_type().required()?;
                let field_type = if let Some(ellipsis) = ellipsis {
                    ast::Expr::Ellipsis(ast::Ellipsis {
                        ellipsis: ellipsis.0,
                        elt: Some(Box::new(type_)),
                    })
                } else {
                    type_
                };
                fields.push(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(field_type),
                    tag: None,
                    comment: None,
                });
                // Parse remaining types after comma
                self.parse_unnamed_parameter_tail(&mut fields)?;
            }
            return Ok(Some(fields));
        }

        // Check for ellipsis (variadic parameter like "args ...int")
        let ellipsis = self.token(Token::ELLIPSIS)?;

        // Special case: qualified type followed by generic args: `sets.Set[string]`
        // IdentifierList returns ["sets"] with last_is_qualified=true when it sees `sets.`
        if idents.len() == 1
            && ellipsis.is_none()
            && last_is_qualified
            && self.current_step.1 == Token::PERIOD
        {
            // Safe to use into_iter().next() because we verified len == 1 above
            let Some(pkg_ident) = idents.into_iter().next() else {
                return Ok(None);
            };
            self.token(Token::PERIOD)?;
            let sel = self.identifier().required()?;

            // Check if this qualified type has generic args [T]
            let type_expr = if self.current_step.1 == Token::LBRACK {
                let lbrack = self.token(Token::LBRACK).required()?;
                let mut indices = vec![self.parse_type().required()?];
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RBRACK {
                        break;
                    }
                    indices.push(self.parse_type().required()?);
                }
                let rbrack = self.token(Token::RBRACK).required()?;

                let selector = ast::Expr::SelectorExpr(ast::SelectorExpr {
                    x: Box::new(ast::Expr::Ident(pkg_ident)),
                    sel,
                });

                if let Some(index) = (indices.len() == 1).then(|| indices.pop()).flatten() {
                    ast::Expr::IndexExpr(ast::IndexExpr {
                        x: Box::new(selector),
                        lbrack: lbrack.0,
                        index: Box::new(index),
                        rbrack: rbrack.0,
                    })
                } else {
                    ast::Expr::IndexListExpr(ast::IndexListExpr {
                        x: Box::new(selector),
                        lbrack: lbrack.0,
                        indices,
                        rbrack: rbrack.0,
                    })
                }
            } else {
                // Just a qualified type without generic args
                ast::Expr::SelectorExpr(ast::SelectorExpr {
                    x: Box::new(ast::Expr::Ident(pkg_ident)),
                    sel,
                })
            };

            // This is an unnamed parameter type
            let mut fields = vec![ast::Field {
                doc: None,
                names: None,
                type_: Some(type_expr),
                tag: None,
                comment: None,
            }];

            // Parse remaining parameters after comma
            self.parse_unnamed_parameter_tail(&mut fields)?;
            return Ok(Some(fields));
        }

        // Multiple idents followed by [ could be:
        // - Named params with slice/array type: (k, v []byte) — [ followed by ]
        // - Unnamed generic type params: (T, F[T]) — [ followed by type arg
        if idents.len() > 1 && ellipsis.is_none() && self.current_step.1 == Token::LBRACK {
            let lbrack_step = self.token(Token::LBRACK).required()?;
            if self.current_step.1 == Token::RBRACK {
                // (k, v []byte) — named parameters with slice type
                let _rbrack = self.token(Token::RBRACK).required()?;
                let elt = self.parse_type().required()?;
                let type_ = ast::Expr::ArrayType(ast::ArrayType {
                    lbrack: lbrack_step.0,
                    len: None,
                    elt: Box::new(elt),
                });
                let names: Vec<ast::Ident<'scanner>> = idents.into_iter().collect();
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: Some(names),
                    type_: Some(type_),
                    tag: None,
                    comment: None,
                }];
                self.parse_named_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }
            // Parse contents of [...] and check what follows ]
            let mut inner = self.parse_expression()?;
            let rbrack = self.token(Token::RBRACK).required()?;

            // If a type follows ], this is (x, y [N]Type) — named params with array type
            if let Some(elt) = self.parse_type()? {
                let type_ = ast::Expr::ArrayType(ast::ArrayType {
                    lbrack: lbrack_step.0,
                    len: inner.map(Box::new),
                    elt: Box::new(elt),
                });
                let names: Vec<ast::Ident<'scanner>> = idents.into_iter().collect();
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: Some(names),
                    type_: Some(type_),
                    tag: None,
                    comment: None,
                }];
                self.parse_named_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }

            // Otherwise (T, F[T]) — last ident has generic args, all unnamed
            let mut fields: Vec<ast::Field<'scanner>> = Vec::new();
            let last = idents.len() - 1;
            for (i, ident) in idents.into_iter().enumerate() {
                if i == last {
                    let type_expr = if let Some(index) = inner.take() {
                        ast::Expr::IndexExpr(ast::IndexExpr {
                            x: Box::new(ast::Expr::Ident(ident)),
                            lbrack: lbrack_step.0,
                            index: Box::new(index),
                            rbrack: rbrack.0,
                        })
                    } else {
                        ast::Expr::Ident(ident)
                    };
                    fields.push(ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(type_expr),
                        tag: None,
                        comment: None,
                    });
                } else {
                    fields.push(ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(ast::Expr::Ident(ident)),
                        tag: None,
                        comment: None,
                    });
                }
            }
            self.parse_plain_unnamed_parameter_tail(&mut fields)?;
            return Ok(Some(fields));
        }

        // Special case: single identifier followed by [ could be:
        // 1. Named parameter with array/slice type: `ret []*Foo` or `n [10]int`
        // 2. Unnamed parameter with generic type: `BarType[T]`
        //
        // Disambiguation: parse the bracket contents, then check what follows ].
        // - If a type follows ] → case 1: ident is param name, [...] is array/slice size
        // - If ) or , follows ] → case 2: ident[...] is a generic type instantiation
        if idents.len() == 1 && ellipsis.is_none() && self.current_step.1 == Token::LBRACK {
            // Safe to use into_iter().next() because we verified len == 1 above
            let Some(ident) = idents.into_iter().next() else {
                return Ok(None);
            };
            let lbrack = self.token(Token::LBRACK).required()?;

            // Check for empty [] which is a slice type
            if self.current_step.1 == Token::RBRACK {
                // This is `ident []Type` - ident is param name, []Type is slice type
                let _rbrack = self.token(Token::RBRACK).required()?;
                let elt = self.parse_type().required()?;
                let type_ = ast::Expr::ArrayType(ast::ArrayType {
                    lbrack: lbrack.0,
                    len: None,
                    elt: Box::new(elt),
                });
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: Some(vec![ident]),
                    type_: Some(type_),
                    tag: None,
                    comment: None,
                }];

                // Continue parsing more named parameters after comma
                self.parse_named_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }

            // Parse what's inside the brackets as an expression/type
            // This could be: a type arg (T), array length (10), or array length expr (n*2)
            // Or multiple type args (K, V)
            let first_inner = self.parse_expression().required()?;

            // Check for comma (multiple type arguments like [K, V])
            if self.current_step.1 == Token::COMMA {
                // This is a generic type with multiple type args: ident[T, V, ...]
                let mut indices = vec![first_inner];
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RBRACK {
                        break;
                    }
                    indices.push(self.parse_type().required()?);
                }
                let rbrack = self.token(Token::RBRACK).required()?;

                let type_expr = ast::Expr::IndexListExpr(ast::IndexListExpr {
                    x: Box::new(ast::Expr::Ident(ident)),
                    lbrack: lbrack.0,
                    indices,
                    rbrack: rbrack.0,
                });

                // This generic type is an unnamed parameter type
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(type_expr),
                    tag: None,
                    comment: None,
                }];

                // Parse remaining unnamed type parameters after comma
                self.parse_unnamed_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }

            let rbrack_pos = self.token(Token::RBRACK).required()?.0;

            // Check what follows ]
            // If a type follows, this is `ident [expr]Type` (array type with ident as param name)
            // If ) or , follows, this is `ident[expr]` (generic type instantiation)
            if let Some(elt) = self.parse_type()? {
                // Case 1: Array type - ident is parameter name
                let type_ = ast::Expr::ArrayType(ast::ArrayType {
                    lbrack: lbrack.0,
                    len: Some(Box::new(first_inner)),
                    elt: Box::new(elt),
                });
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: Some(vec![ident]),
                    type_: Some(type_),
                    tag: None,
                    comment: None,
                }];

                // Continue parsing more named parameters after comma
                self.parse_named_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            } else {
                // Case 2: Generic type instantiation - ident[inner] is the type
                let type_expr = ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(ast::Expr::Ident(ident)),
                    lbrack: lbrack.0,
                    index: Box::new(first_inner),
                    rbrack: rbrack_pos,
                });

                // This generic type is an unnamed parameter type
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(type_expr),
                    tag: None,
                    comment: None,
                }];

                // Parse remaining unnamed type parameters after comma
                self.parse_unnamed_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }
        }

        let type_ = self.parse_type()?;

        // If no type can be found and no ellipsis, then the idents might be types
        // Handle qualified types like (cipher.AEAD, error) where the first ident (cipher)
        // is actually the package part of a qualified type
        if type_.is_none() && ellipsis.is_none() {
            // Check if the first (and only) ident is followed by a period - qualified type
            if idents.len() == 1 && self.current_step.1 == Token::PERIOD {
                // Safe to use into_iter().next() because we verified len == 1 above
                let Some(ident) = idents.into_iter().next() else {
                    return Ok(None);
                };
                self.token(Token::PERIOD)?;
                let sel = self.identifier().required()?;
                let type_ = ast::Expr::SelectorExpr(ast::SelectorExpr {
                    x: Box::new(ast::Expr::Ident(ident)),
                    sel,
                });
                // Continue parsing as unnamed parameter types
                let mut fields = vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(type_),
                    tag: None,
                    comment: None,
                }];
                // Parse remaining types after comma
                self.parse_unnamed_parameter_tail(&mut fields)?;
                return Ok(Some(fields));
            }
            // Simple case: all idents are types, but we need to continue parsing more types after comma
            let mut fields: Vec<ast::Field<'scanner>> = idents
                .into_iter()
                .map(|ident| ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(ast::Expr::Ident(ident)),
                    tag: None,
                    comment: None,
                })
                .collect();
            // Parse remaining types after comma
            self.parse_unnamed_parameter_tail(&mut fields)?;
            return Ok(Some(fields));
        }

        // If a type can be found, then we expect idents + types: (a, b bool, c bool, d bool)

        // Handle variadic parameter in first position
        let first_field = if let Some(ellipsis) = ellipsis {
            ast::Field {
                comment: None,
                type_: Some(ast::Expr::Ellipsis(ast::Ellipsis {
                    ellipsis: ellipsis.0,
                    elt: type_.map(Box::new),
                })),
                tag: None,
                names: Some(idents),
                doc: None,
            }
        } else {
            ast::Field {
                comment: None,
                type_,
                tag: None,
                names: Some(idents),
                doc: None,
            }
        };

        let mut fields = vec![first_field];

        while self.token(Token::COMMA)?.is_some() {
            // Handle trailing comma
            if self.current_step.1 == Token::RPAREN {
                break;
            }
            let (idents, _, _) = self.parse_identifier_list().required()?;
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.parse_type().required()?;

            if let Some(ellipsis) = ellipsis {
                fields.push(ast::Field {
                    comment: None,
                    type_: Some(ast::Expr::Ellipsis(ast::Ellipsis {
                        ellipsis: ellipsis.0,
                        elt: Some(Box::new(type_)),
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

    fn parse_unnamed_parameter_tail(
        &mut self,
        fields: &mut Vec<ast::Field<'scanner>>,
    ) -> Result<()> {
        while self.token(Token::COMMA)?.is_some() {
            if self.current_step.1 == Token::RPAREN {
                break;
            }
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.parse_type().required()?;
            let field_type = if let Some(ellipsis) = ellipsis {
                ast::Expr::Ellipsis(ast::Ellipsis {
                    ellipsis: ellipsis.0,
                    elt: Some(Box::new(type_)),
                })
            } else {
                type_
            };
            fields.push(ast::Field {
                doc: None,
                names: None,
                type_: Some(field_type),
                tag: None,
                comment: None,
            });
        }
        Ok(())
    }

    fn parse_named_parameter_tail(&mut self, fields: &mut Vec<ast::Field<'scanner>>) -> Result<()> {
        while self.token(Token::COMMA)?.is_some() {
            if self.current_step.1 == Token::RPAREN {
                break;
            }
            let (param_names, _, _) = self.parse_identifier_list().required()?;
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let param_type = self.parse_type().required()?;
            let field_type = if let Some(ellipsis) = ellipsis {
                ast::Expr::Ellipsis(ast::Ellipsis {
                    ellipsis: ellipsis.0,
                    elt: Some(Box::new(param_type)),
                })
            } else {
                param_type
            };
            fields.push(ast::Field {
                doc: None,
                names: Some(param_names),
                type_: Some(field_type),
                tag: None,
                comment: None,
            });
        }
        Ok(())
    }

    fn parse_plain_unnamed_parameter_tail(
        &mut self,
        fields: &mut Vec<ast::Field<'scanner>>,
    ) -> Result<()> {
        while self.token(Token::COMMA)?.is_some() {
            if self.current_step.1 == Token::RPAREN {
                break;
            }
            let type_ = self.parse_type().required()?;
            fields.push(ast::Field {
                doc: None,
                names: None,
                type_: Some(type_),
                tag: None,
                comment: None,
            });
        }
        Ok(())
    }
}
