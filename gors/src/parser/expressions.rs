use super::{RawParserError, Result, ResultExt, core::Parser};
use crate::ast;
use crate::token::Token;

impl<'scanner> Parser<'scanner> {
    // ExpressionList = Expression { "," Expression } .
    pub(super) fn parse_expression_list(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
        log::debug!("Parser::parse_expression_list()");

        let mut out = match self.parse_expression()? {
            Some(v) => vec![v],
            None => return Ok(None),
        };

        while self.token(Token::COMMA)?.is_some() {
            // Allow trailing comma - if next token is ), ], or }, don't require another expression
            if matches!(
                self.current_step.1,
                Token::RPAREN | Token::RBRACK | Token::RBRACE
            ) {
                break;
            }
            out.push(self.parse_expression().required()?);
        }

        Ok(Some(out))
    }

    // Expression = UnaryExpr | Expression binary_op Expression .
    pub(super) fn parse_expression(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_expression()");

        let unary_expr = match self.parse_unary_expr()? {
            Some(v) => v,
            None => return Ok(None),
        };

        self.expression(unary_expr, Token::lowest_precedence())
    }

    // https://en.wikipedia.org/wiki/Operator-precedence_parser
    pub(super) fn expression(
        &mut self,
        mut lhs: ast::Expr<'scanner>,
        min_precedence: u8,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        while let Some(op) = self.get_binary_op(min_precedence)? {
            self.next()?;

            let mut rhs = self.parse_unary_expr().required()?;
            while self.get_binary_op(op.1.precedence() + 1)?.is_some() {
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
    pub(super) fn parse_unary_expr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_unary_expr()");

        // Special case: <- followed by chan is a receive-only channel type, not receive expression
        // This happens in contexts like make(<-chan T)
        // We need to check if <- is followed by chan WITHOUT consuming the <-
        if self.current_step.1 == Token::ARROW {
            // Look ahead to see if next token after <- is chan
            // Save current position and try to parse channel type
            // Since we can't easily peek 2 tokens, we use the scanner iterator state
            // For now, manually check: consume <- and check if chan follows
            // If chan follows, parse as channel type; otherwise put it back (return as unary)

            // Actually, we need a different approach. Let's check if we're in a type context.
            // Better approach: just consume <- and check immediately
            let arrow_step = self.current_step;
            self.next()?; // consume <-

            if self.current_step.1 == Token::CHAN {
                // It's <-chan, parse the rest as channel type
                self.next()?; // consume chan
                let value = Box::new(self.parse_element_type().required()?);
                return Ok(Some(ast::Expr::ChanType(ast::ChanType {
                    begin: arrow_step.0,
                    arrow: Some(arrow_step.0),
                    dir: ast::ChanDir::RECV as u8,
                    value,
                })));
            }

            // Not followed by chan - it's a receive expression
            // The <- was already consumed, so parse the operand
            let x = Box::new(self.parse_unary_expr().required()?);
            return Ok(Some(ast::Expr::UnaryExpr(ast::UnaryExpr {
                op: Token::ARROW,
                op_pos: arrow_step.0,
                x,
            })));
        }

        if let Some(op) = self.unary_op()? {
            let x = Box::new(self.parse_unary_expr().required()?);
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

        self.parse_primary_expr()
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
    pub(super) fn parse_primary_expr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_primary_expr()");

        let primary_expr = match self.parse_operand()? {
            Some(v) => v,
            None => return Ok(None),
        };

        Ok(Some(self.continue_primary_expr(primary_expr)?))
    }

    pub(super) fn continue_primary_expr(
        &mut self,
        mut primary_expr: ast::Expr<'scanner>,
    ) -> Result<ast::Expr<'scanner>> {
        loop {
            match self.current_step.1 {
                Token::PERIOD => {
                    primary_expr = self
                        .parse_selector_or_type_assertion(primary_expr)
                        .required()?;
                }
                Token::LBRACK => {
                    primary_expr = self.parse_index_or_slice(primary_expr).required()?;
                }
                Token::LPAREN => {
                    primary_expr = self.parse_arguments(primary_expr).required()?;
                }
                Token::LBRACE if self.expr_level >= 0 => {
                    primary_expr = self.parse_literal_value(primary_expr).required()?;
                }
                _ => break,
            }
        }

        Ok(primary_expr)
    }

    // LiteralValue = "{" [ ElementList [ "," ] ] "}" .
    // ElementList  = KeyedElement { "," KeyedElement } .
    // Used when type is already known from PrimaryExpr
    pub(super) fn parse_literal_value(
        &mut self,
        type_: ast::Expr<'scanner>,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_literal_value()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Inside composite literal, allow nested composite literals with elided types
        // Use max(1, ...) to ensure expr_level is positive even if it was -1
        let prev_expr_level = self.expr_level;
        self.expr_level = std::cmp::max(1, self.expr_level + 1);

        let mut elts = self.parse_keyed_element()?.map(|elt| vec![elt]);
        if let Some(elts) = elts.as_mut() {
            while self.token(Token::COMMA)?.is_some() {
                if let Some(k) = self.parse_keyed_element()? {
                    elts.push(k);
                } else {
                    break;
                }
            }
        }

        let rbrace = self.token(Token::RBRACE).required()?;
        self.expr_level = prev_expr_level;

        Ok(Some(ast::Expr::CompositeLit(ast::CompositeLit {
            type_: Some(Box::new(type_)),
            lbrace: lbrace.0,
            elts,
            rbrace: rbrace.0,
            incomplete: false,
        })))
    }

    // Selector      = "." identifier .
    // TypeAssertion = "." "(" Type ")" .
    // TypeSwitchGuard = "." "(" "type" ")" .
    pub(super) fn parse_selector_or_type_assertion(
        &mut self,
        x: ast::Expr<'scanner>,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_selector_or_type_assertion()");

        if self.token(Token::PERIOD)?.is_none() {
            return Ok(None);
        }

        if let Some(lparen) = self.token(Token::LPAREN)? {
            // Check for type switch guard: x.(type)
            if self.token(Token::TYPE)?.is_some() {
                let rparen = self.token(Token::RPAREN).required()?;
                return Ok(Some(ast::Expr::TypeAssertExpr(ast::TypeAssertExpr {
                    x: Box::new(x),
                    lparen: lparen.0,
                    type_: None, // nil in Go's AST for type switch guards
                    rparen: rparen.0,
                })));
            }
            let type_ = self.parse_type().required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            return Ok(Some(ast::Expr::TypeAssertExpr(ast::TypeAssertExpr {
                x: Box::new(x),
                lparen: lparen.0,
                type_: Some(Box::new(type_)),
                rparen: rparen.0,
            })));
        }

        Ok(Some(ast::Expr::SelectorExpr(ast::SelectorExpr {
            x: Box::new(x),
            sel: self.identifier().required()?,
        })))
    }

    // Index = "[" Expression "]" .
    // Slice = "[" [ Expression ] ":" [ Expression ] "]" |
    //         "[" [ Expression ] ":" Expression ":" Expression "]" .
    // IndexListExpr (Go 1.18+ generics) = "[" Expression { "," Expression } "]" .
    pub(super) fn parse_index_or_slice(
        &mut self,
        x: ast::Expr<'scanner>,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_index_or_slice()");

        let lbrack = match self.token(Token::LBRACK)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Inside brackets, composite literals are always allowed
        self.expr_level += 1;

        let low = if let Some(low) = self.parse_expression()? {
            // Check for comma (generic instantiation with multiple type args)
            if self.token(Token::COMMA)?.is_some() {
                let mut indices = vec![low];
                // Allow trailing comma
                if self.current_step.1 != Token::RBRACK {
                    indices.push(self.parse_expression().required()?);
                    while self.token(Token::COMMA)?.is_some() {
                        if self.current_step.1 == Token::RBRACK {
                            break;
                        }
                        indices.push(self.parse_expression().required()?);
                    }
                }
                let rbrack = self.token(Token::RBRACK).required()?;
                self.expr_level -= 1;
                return Ok(Some(ast::Expr::IndexListExpr(ast::IndexListExpr {
                    x: Box::new(x),
                    lbrack: lbrack.0,
                    indices,
                    rbrack: rbrack.0,
                })));
            }

            if let Some(rbrack) = self.token(Token::RBRACK)? {
                self.expr_level -= 1;
                return Ok(Some(ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(x),
                    lbrack: lbrack.0,
                    index: Box::new(low),
                    rbrack: rbrack.0,
                })));
            }
            Some(low)
        } else {
            None
        };

        self.token(Token::COLON).required()?;

        let high = if let Some(high) = self.parse_expression()? {
            if self.token(Token::COLON)?.is_some() {
                let max = self.parse_expression().required()?;
                let rbrack = self.token(Token::RBRACK).required()?;
                self.expr_level -= 1;
                return Ok(Some(ast::Expr::SliceExpr(ast::SliceExpr {
                    x: Box::new(x),
                    lbrack: lbrack.0,
                    low: low.map(Box::new),
                    high: Some(Box::new(high)),
                    max: Some(Box::new(max)),
                    slice3: true,
                    rbrack: rbrack.0,
                })));
            }
            Some(high)
        } else {
            None
        };
        let rbrack = self.token(Token::RBRACK).required()?;
        self.expr_level -= 1;

        Ok(Some(ast::Expr::SliceExpr(ast::SliceExpr {
            x: Box::new(x),
            lbrack: lbrack.0,
            low: low.map(Box::new),
            high: high.map(Box::new),
            max: None,
            slice3: false,
            rbrack: rbrack.0,
        })))
    }

    // Arguments = "(" [ ( ExpressionList | Type [ "," ExpressionList ] ) [ "..." ] [ "," ] ] ")" .
    pub(super) fn parse_arguments(
        &mut self,
        x: ast::Expr<'scanner>,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_arguments()");

        let lparen = match self.token(Token::LPAREN)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Inside parentheses, composite literals are always allowed
        self.expr_level += 1;

        let mut args = if let Some(exprs) = self.parse_expression_list()? {
            exprs
        } else if let Some(type_) = self.parse_type()? {
            vec![type_]
        } else {
            vec![]
        };

        if self.token(Token::COMMA)?.is_some() {
            let mut exprs = self.parse_expression_list().required()?;
            args.append(&mut exprs);
        }

        let ellipsis = if !args.is_empty() {
            let ellipsis = self.token(Token::ELLIPSIS)?;
            self.token(Token::COMMA)?;
            ellipsis
        } else {
            None
        };

        let rparen = self.token(Token::RPAREN).required()?;
        self.expr_level -= 1;

        Ok(Some(ast::Expr::CallExpr(ast::CallExpr {
            fun: Box::new(x),
            lparen: lparen.0,
            args: Some(args),
            ellipsis: ellipsis.map(|(pos, _, _)| pos),
            rparen: rparen.0,
        })))
    }

    // Operand = Literal | OperandName | "(" Expression ")" .
    // Literal = BasicLit | CompositeLit | FunctionLit .
    // OperandName = identifier | QualifiedIdent .
    pub(super) fn parse_operand(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_operand()");

        use Token::*;
        Ok(match self.current_step.1 {
            IDENT => Some(ast::Expr::Ident(self.identifier().required()?)),
            INT | FLOAT | IMAG | CHAR | STRING => {
                Some(ast::Expr::BasicLit(self.parse_basic_lit().required()?))
            }
            LPAREN => {
                let lparen = self.token(Token::LPAREN).required()?;
                // Inside parentheses, composite literals are always allowed
                self.expr_level += 1;

                // First, try to parse as expression
                if let Some(expr) = self.parse_expression()? {
                    let rparen = self.token(Token::RPAREN).required()?;
                    self.expr_level -= 1;
                    return Ok(Some(ast::Expr::ParenExpr(ast::ParenExpr {
                        lparen: lparen.0,
                        x: Box::new(expr),
                        rparen: rparen.0,
                    })));
                }

                // If expression parsing failed, try to parse as type
                // This handles cases like (func(string))(nil)
                if let Some(type_) = self.parse_type()? {
                    let rparen = self.token(Token::RPAREN).required()?;
                    self.expr_level -= 1;
                    // Return the type wrapped in parens - can be used for type conversion
                    return Ok(Some(ast::Expr::ParenExpr(ast::ParenExpr {
                        lparen: lparen.0,
                        x: Box::new(type_),
                        rparen: rparen.0,
                    })));
                }

                // Neither expression nor type could be parsed
                self.expr_level -= 1;
                return Err(RawParserError::UnexpectedToken);
            }
            FUNC => {
                // Try function literal first; if no body, fall back to function type
                // (for use in type conversions like func(string)(nil))
                let func = self.token(Token::FUNC).required()?;
                let signature = self.parse_signature(Some(func.0)).required()?;
                // Reset expr_level when parsing function body to allow composite literals
                // inside the function, even if we're in a context (like if condition) that
                // normally disables them
                let saved_expr_level = self.expr_level;
                self.expr_level = 0;
                let body = self.parse_function_body()?;
                self.expr_level = saved_expr_level;
                if let Some(body) = body {
                    // It's a function literal
                    Some(ast::Expr::FuncLit(ast::FuncLit {
                        type_: signature,
                        body,
                    }))
                } else {
                    // It's a function type (no body)
                    Some(ast::Expr::FuncType(signature))
                }
            }
            // Interface type for type conversions like interface{}(x)
            INTERFACE => Some(ast::Expr::InterfaceType(
                self.parse_interface_type().required()?,
            )),
            // Handle nested composite literals without explicit type
            // Go allows eliding the type for nested composite literals
            LBRACE if self.expr_level > 0 => {
                let lbrace = self.token(Token::LBRACE).required()?;
                // Inside composite literal, allow nested composite literals
                self.expr_level += 1;
                let mut elts = self.parse_keyed_element()?.map(|elt| vec![elt]);
                if let Some(elts) = elts.as_mut() {
                    while self.token(Token::COMMA)?.is_some() {
                        if let Some(k) = self.parse_keyed_element()? {
                            elts.push(k);
                        } else {
                            break;
                        }
                    }
                }
                let rbrace = self.token(Token::RBRACE).required()?;
                self.expr_level -= 1;
                // CompositeLit with nil type (type is elided in nested literals)
                Some(ast::Expr::CompositeLit(ast::CompositeLit {
                    type_: None,
                    lbrace: lbrace.0,
                    elts,
                    rbrace: rbrace.0,
                    incomplete: false,
                }))
            }
            _ => {
                // Try to parse a composite literal, or just a type if no { follows
                if let Some(type_) = self.parse_literal_type()? {
                    if self.current_step.1 == Token::LBRACE {
                        // After a LiteralType, a { is always a composite literal
                        // (the ambiguity with blocks only exists at statement level)
                        let lbrace = self.token(Token::LBRACE).required()?;
                        // Inside composite literal, allow nested composite literals
                        // Use max(1, ...) to ensure expr_level is positive even if it was -1
                        let prev_expr_level = self.expr_level;
                        self.expr_level = std::cmp::max(1, self.expr_level + 1);
                        let mut elts = self.parse_keyed_element()?.map(|elt| vec![elt]);
                        if let Some(elts) = elts.as_mut() {
                            while self.token(Token::COMMA)?.is_some() {
                                if let Some(k) = self.parse_keyed_element()? {
                                    elts.push(k);
                                } else {
                                    break;
                                }
                            }
                        }
                        let rbrace = self.token(Token::RBRACE).required()?;
                        self.expr_level = prev_expr_level;
                        Some(ast::Expr::CompositeLit(ast::CompositeLit {
                            type_: Some(Box::new(type_)),
                            lbrace: lbrace.0,
                            elts,
                            rbrace: rbrace.0,
                            incomplete: false,
                        }))
                    } else {
                        // Just the type (used as an expression, e.g., in make([]byte, 10))
                        Some(type_)
                    }
                } else {
                    None
                }
            }
        })
    }

    // LiteralType = StructType | ArrayType | "[" "..." "]" ElementType |
    //               SliceType | MapType | TypeName .
    pub(super) fn parse_literal_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_literal_type()");

        Ok(match self.current_step.1 {
            Token::STRUCT => Some(ast::Expr::StructType(self.parse_struct_type().required()?)),
            Token::LBRACK => Some(ast::Expr::ArrayType(
                self.parse_array_type_or_slice_type::<true>().required()?,
            )),
            Token::MAP => Some(ast::Expr::MapType(self.parse_map_type().required()?)),
            Token::CHAN => Some(ast::Expr::ChanType(self.parse_channel_type().required()?)),
            Token::IDENT => Some(self.parse_type_name().required()?),
            _ => None,
        })
    }

    // KeyedElement = [ Key ":" ] Element .
    // Key          = FieldName | Expression | LiteralValue .
    // FieldName    = identifier .
    // Element      = Expression | LiteralValue .
    pub(super) fn parse_keyed_element(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_keyed_element()");

        let key = match self.parse_expression()? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(colon) = self.token(Token::COLON)? {
            let value = self.parse_expression().required()?;
            return Ok(Some(ast::Expr::KeyValueExpr(ast::KeyValueExpr {
                key: Box::new(key),
                colon: colon.0,
                value: Box::new(value),
            })));
        }

        Ok(Some(key))
    }

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    pub(super) fn parse_basic_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::parse_basic_lit()");

        Ok(match self.current_step.1 {
            Token::INT => Some(self.int_lit().required()?),
            Token::FLOAT => Some(self.float_lit().required()?),
            Token::IMAG => Some(self.imaginary_lit().required()?),
            Token::CHAR => Some(self.rune_lit().required()?),
            Token::STRING => Some(self.string_lit().required()?),
            _ => None,
        })
    }
}
