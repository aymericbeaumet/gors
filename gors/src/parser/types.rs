use super::{RawParserError, Result, ResultExt, core::Parser};
use crate::ast;
use crate::token::Token;

impl<'scanner> Parser<'scanner> {
    // Type = TypeName | TypeLit | "(" Type ")" .
    pub(super) fn parse_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_type()");

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let type_ = self.parse_type().required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            // Preserve the parentheses by wrapping in ParenExpr
            return Ok(Some(ast::Expr::ParenExpr(ast::ParenExpr {
                lparen: lparen.0,
                x: Box::new(type_),
                rparen: rparen.0,
            })));
        }

        if let Some(type_name) = self.parse_type_name()? {
            return Ok(Some(type_name));
        }

        if let Some(type_lit) = self.parse_type_lit()? {
            return Ok(Some(type_lit));
        }

        Ok(None)
    }

    // TypeList = Type { "," Type } .
    pub(super) fn parse_type_list(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
        log::debug!("Parser::parse_type_list()");

        let first_type = match self.parse_type()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut types = vec![first_type];

        while self.token(Token::COMMA)?.is_some() {
            // Allow trailing comma
            if matches!(
                self.current_step.1,
                Token::COLON | Token::RBRACK | Token::RPAREN
            ) {
                break;
            }
            types.push(self.parse_type().required()?);
        }

        Ok(Some(types))
    }

    // TypeName = identifier [ TypeArgs ] | QualifiedIdent [ TypeArgs ] .
    // TypeArgs = "[" TypeList [ "," ] "]" .
    pub(super) fn parse_type_name(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_type_name()");

        let type_name = match self.parse_identifier_or_qualified_ident()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Check for generic type instantiation [T] or [T1, T2]
        if self.current_step.1 == Token::LBRACK {
            let lbrack = self.token(Token::LBRACK).required()?;

            // Check if this is an empty [], which would be invalid for type args
            if self.current_step.1 == Token::RBRACK {
                // This shouldn't happen in a valid program, but handle gracefully
                let rbrack = self.token(Token::RBRACK).required()?;
                return Ok(Some(ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(type_name),
                    lbrack: lbrack.0,
                    index: Box::new(ast::Expr::Ident(ast::Ident {
                        name_pos: rbrack.0,
                        name: "",
                        obj: None,
                    })),
                    rbrack: rbrack.0,
                })));
            }

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
                    x: Box::new(type_name),
                    lbrack: lbrack.0,
                    index: Box::new(index),
                    rbrack: rbrack.0,
                })));
            } else {
                return Ok(Some(ast::Expr::IndexListExpr(ast::IndexListExpr {
                    x: Box::new(type_name),
                    lbrack: lbrack.0,
                    indices,
                    rbrack: rbrack.0,
                })));
            }
        }

        Ok(Some(type_name))
    }

    // TypeLit = ArrayType | StructType | PointerType | FunctionType | InterfaceType |
    //           SliceType | MapType | ChannelType .
    pub(super) fn parse_type_lit(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_type_lit()");

        Ok(match self.current_step.1 {
            Token::LBRACK => Some(ast::Expr::ArrayType(
                self.parse_array_type_or_slice_type::<false>().required()?,
            )),
            Token::STRUCT => Some(ast::Expr::StructType(self.parse_struct_type().required()?)),
            Token::MUL => Some(ast::Expr::StarExpr(self.parse_pointer_type().required()?)),
            Token::FUNC => Some(ast::Expr::FuncType(self.parse_function_type().required()?)),
            Token::INTERFACE => Some(ast::Expr::InterfaceType(
                self.parse_interface_type().required()?,
            )),
            Token::MAP => Some(ast::Expr::MapType(self.parse_map_type().required()?)),
            Token::CHAN => Some(ast::Expr::ChanType(self.parse_channel_type().required()?)),
            Token::ARROW => Some(ast::Expr::ChanType(self.parse_channel_type().required()?)), // <-chan (receive-only)
            _ => None,
        })
    }

    // ArrayType   = "[" ArrayLength "]" ElementType .
    // ArrayLength = Expression .
    // SliceType   = "[" "]" ElementType .
    pub(super) fn parse_array_type_or_slice_type<const ELLIPSIS: bool>(
        &mut self,
    ) -> Result<Option<ast::ArrayType<'scanner>>> {
        log::debug!(
            "Parser::parse_array_type_or_slice_type::<ELLIPSIS={}>()",
            ELLIPSIS
        );

        let lbrack = match self.token(Token::LBRACK)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Always permit ellipsis for fault-tolerant parsing (matches Go's parser)
        let len = if let Some(ellipsis) = self.token(Token::ELLIPSIS)? {
            Some(ast::Expr::Ellipsis(ast::Ellipsis {
                ellipsis: ellipsis.0,
                elt: None,
            }))
        } else {
            self.parse_expression()?
        };

        self.token(Token::RBRACK).required()?;

        let element_type = self.parse_element_type().required()?;

        Ok(Some(ast::ArrayType {
            lbrack: lbrack.0,
            len: len.map(Box::new),
            elt: Box::new(element_type),
        }))
    }

    // MapType = "map" "[" KeyType "]" ElementType .
    pub(super) fn parse_map_type(&mut self) -> Result<Option<ast::MapType<'scanner>>> {
        log::debug!("Parser::parse_map_type()");

        let map = match self.token(Token::MAP)? {
            Some(v) => v,
            None => return Ok(None),
        };
        self.token(Token::LBRACK).required()?;
        let key_type = self.parse_key_type().required()?;
        self.token(Token::RBRACK).required()?;
        let element_type = self.parse_element_type().required()?;

        Ok(Some(ast::MapType {
            map: map.0,
            key: Box::new(key_type),
            value: Box::new(element_type),
        }))
    }

    // KeyType = Type .
    pub(super) fn parse_key_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_key_type()");

        self.parse_type()
    }

    // ChannelType = ( "chan" | "chan" "<-" | "<-" "chan" ) ElementType .
    pub(super) fn parse_channel_type(&mut self) -> Result<Option<ast::ChanType<'scanner>>> {
        log::debug!("Parser::parse_channel_type()");

        if let Some(chan) = self.token(Token::CHAN)? {
            if let Some(arrow) = self.token(Token::ARROW)? {
                let value = Box::new(self.parse_element_type().required()?);
                return Ok(Some(ast::ChanType {
                    begin: chan.0,
                    arrow: Some(arrow.0),
                    dir: ast::ChanDir::SEND as u8,
                    value,
                }));
            }

            let value = Box::new(self.parse_element_type().required()?);
            return Ok(Some(ast::ChanType {
                begin: chan.0,
                arrow: None,
                dir: ast::ChanDir::SEND as u8 | ast::ChanDir::RECV as u8,
                value,
            }));
        }

        if let Some(arrow) = self.token(Token::ARROW)? {
            self.token(Token::CHAN).required()?;
            let value = Box::new(self.parse_element_type().required()?);
            return Ok(Some(ast::ChanType {
                begin: arrow.0,
                arrow: Some(arrow.0), // <-chan has arrow at the start
                dir: ast::ChanDir::RECV as u8,
                value,
            }));
        }

        Ok(None)
    }

    // FunctionType = "func" Signature .
    pub(super) fn parse_function_type(&mut self) -> Result<Option<ast::FuncType<'scanner>>> {
        log::debug!("Parser::parse_function_type()");

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut signature = self.parse_signature(None).required()?;
        signature.func = Some(func.0);
        Ok(Some(signature))
    }

    // ElementType = Type .
    pub(super) fn parse_element_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_element_type()");

        self.parse_type()
    }

    // PointerType = "*" BaseType .
    pub(super) fn parse_pointer_type(&mut self) -> Result<Option<ast::StarExpr<'scanner>>> {
        log::debug!("Parser::parse_pointer_type()");

        let star = match self.token(Token::MUL)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let x = Box::new(self.parse_base_type().required()?);
        Ok(Some(ast::StarExpr { star: star.0, x }))
    }

    // BaseType = Type .
    pub(super) fn parse_base_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_base_type()");

        self.parse_type()
    }

    // InterfaceType = "interface" "{" { InterfaceElem ";" } "}" .
    // InterfaceElem = MethodElem | TypeElem .
    // MethodElem    = MethodName Signature .
    // TypeElem      = TypeTerm { "|" TypeTerm } .
    // TypeTerm      = Type | UnderlyingType .
    // UnderlyingType = "~" Type .
    pub(super) fn parse_interface_type(&mut self) -> Result<Option<ast::InterfaceType<'scanner>>> {
        log::debug!("Parser::parse_interface_type()");

        let interface = match self.token(Token::INTERFACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut fields = vec![];
        loop {
            let doc = self.lead_comment.take();

            // Check for underlying type constraint (~Type)
            if let Some(tilde) = self.token(Token::TILDE)? {
                let type_ = self.parse_type().required()?;
                let mut type_elem = ast::Expr::UnaryExpr(ast::UnaryExpr {
                    op_pos: tilde.0,
                    op: Token::TILDE,
                    x: Box::new(type_),
                });

                // Check for union types
                while let Some(or_tok) = self.token(Token::OR)? {
                    let next_term = self.parse_type_term().required()?;
                    type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                        x: Box::new(type_elem),
                        op_pos: or_tok.0,
                        op: Token::OR,
                        y: Box::new(next_term),
                    });
                }

                fields.push(ast::Field {
                    doc,
                    names: None,
                    type_: Some(type_elem),
                    tag: None,
                    comment: self.line_comment.take(),
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            }

            if matches!(
                self.current_step.1,
                Token::STRUCT
                    | Token::INTERFACE
                    | Token::LPAREN
                    | Token::LBRACK
                    | Token::MAP
                    | Token::FUNC
                    | Token::CHAN
                    | Token::ARROW
                    | Token::MUL
            ) {
                let type_ = self.parse_type().required()?;

                let mut type_elem = type_;
                while let Some(or_tok) = self.token(Token::OR)? {
                    let next_term = self.parse_type_term().required()?;
                    type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                        x: Box::new(type_elem),
                        op_pos: or_tok.0,
                        op: Token::OR,
                        y: Box::new(next_term),
                    });
                }

                fields.push(ast::Field {
                    doc,
                    names: None,
                    type_: Some(type_elem),
                    tag: None,
                    comment: self.line_comment.take(),
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            }

            if let Some(method_spec) = self.parse_method_name()? {
                if self.current_step.1 == Token::PERIOD {
                    // Qualified type: pkg.Type, possibly followed by [T] and/or |
                    self.token(Token::PERIOD)?;
                    let sel = self.identifier().required()?;
                    let mut type_expr: ast::Expr<'scanner> =
                        ast::Expr::SelectorExpr(ast::SelectorExpr {
                            x: Box::new(ast::Expr::Ident(method_spec)),
                            sel,
                        });

                    // Check for generic instantiation: pkg.Type[T]
                    type_expr = self.parse_optional_type_instance(type_expr)?;

                    // Check for union: pkg.Type | OtherType
                    type_expr = self.parse_embedded_elem(type_expr)?;

                    fields.push(ast::Field {
                        doc,
                        names: None,
                        type_: Some(type_expr),
                        tag: None,
                        comment: self.line_comment.take(),
                    });
                    if self.token(Token::SEMICOLON)?.is_none() {
                        break;
                    }
                    continue;
                }

                // Check for type parameters on the embedded type (e.g., Comparable[T])
                if self.current_step.1 == Token::LBRACK {
                    let type_expr = ast::Expr::Ident(method_spec);
                    let type_expr = self.parse_optional_type_instance(type_expr)?;
                    let type_expr = self.parse_embedded_elem(type_expr)?;

                    fields.push(ast::Field {
                        doc,
                        names: None,
                        type_: Some(type_expr),
                        tag: None,
                        comment: self.line_comment.take(),
                    });
                    if self.token(Token::SEMICOLON)?.is_none() {
                        break;
                    }
                    continue;
                }

                if let Some(signature) = self.parse_signature(None)? {
                    let pre_semi_comment = self.line_comment.take();
                    let mut field = ast::Field {
                        doc,
                        names: Some(vec![method_spec]),
                        type_: Some(ast::Expr::FuncType(signature)),
                        tag: None,
                        comment: None,
                    };
                    if self.token(Token::SEMICOLON)?.is_some() {
                        field.comment = self.line_comment.take().or(pre_semi_comment);
                        fields.push(field);
                    } else {
                        field.comment = pre_semi_comment;
                        fields.push(field);
                        break;
                    }
                    continue;
                }

                let mut type_elem = ast::Expr::Ident(method_spec);

                while let Some(or_tok) = self.token(Token::OR)? {
                    let next_term = self.parse_type_term().required()?;
                    type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                        x: Box::new(type_elem),
                        op_pos: or_tok.0,
                        op: Token::OR,
                        y: Box::new(next_term),
                    });
                }

                fields.push(ast::Field {
                    doc,
                    names: None,
                    type_: Some(type_elem),
                    tag: None,
                    comment: self.line_comment.take(),
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            };

            if let Some(interface_type_name) = self.parse_interface_type_name()? {
                fields.push(ast::Field {
                    doc,
                    names: None,
                    type_: Some(interface_type_name),
                    tag: None,
                    comment: self.line_comment.take(),
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
    pub(super) fn parse_method_name(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::parse_method_name()");

        self.identifier()
    }

    // InterfaceTypeName = TypeName .
    pub(super) fn parse_interface_type_name(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_interface_type_name()");

        self.parse_type_name()
    }

    // StructType = "struct" "{" { FieldDecl ";" } "}" .
    pub(super) fn parse_struct_type(&mut self) -> Result<Option<ast::StructType<'scanner>>> {
        log::debug!("Parser::parse_struct_type()");

        let struct_ = match self.token(Token::STRUCT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut fields = vec![];
        loop {
            let doc = self.lead_comment.take();
            if let Some(mut field_decl) = self.parse_field_decl()? {
                field_decl.doc = doc;
                let pre_semi_comment = self.line_comment.take();
                if self.token(Token::SEMICOLON)?.is_some() {
                    field_decl.comment = self.line_comment.take().or(pre_semi_comment);
                    fields.push(field_decl);
                } else {
                    field_decl.comment = pre_semi_comment;
                    fields.push(field_decl);
                    break;
                }
            } else {
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
    pub(super) fn parse_field_decl(&mut self) -> Result<Option<ast::Field<'scanner>>> {
        log::debug!("Parser::parse_field_decl()");

        if let Some(star) = self.token(Token::MUL)? {
            let type_name = Box::new(self.parse_type_name().required()?);
            let tag = self.parse_tag()?;
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

        if let Some((mut names, _, last_is_qualified)) = self.parse_identifier_list()? {
            // Check if this is a qualified identifier for an embedded field (e.g., sync.RWMutex)
            // or a qualified generic type (e.g., listers.ResourceIndexer[*Deployment])
            if let Some(name) = (names.len() == 1
                && (self.current_step.1 == Token::PERIOD || last_is_qualified))
                .then(|| names.pop())
                .flatten()
            {
                self.token(Token::PERIOD)?;
                let sel = self.identifier().required()?;

                // Check for generic type arguments [T] or [T1, T2]
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
                        x: Box::new(ast::Expr::Ident(name)),
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
                    ast::Expr::SelectorExpr(ast::SelectorExpr {
                        x: Box::new(ast::Expr::Ident(name)),
                        sel,
                    })
                };

                let tag = self.parse_tag()?;
                return Ok(Some(ast::Field {
                    doc: None,
                    type_: Some(type_expr),
                    names: None,
                    tag,
                    comment: None,
                }));
            }

            // Handle the complex case of single identifier followed by [
            // This could be:
            // - `a [20]int`     -> field 'a' with array type [20]int
            // - `a [size]int`  -> field 'a' with array type [size]int (size is constant)
            // - `B[int]`        -> embedded generic type B[int]
            // - `a []int`       -> field 'a' with slice type []int
            //
            // The disambiguation rule is: if a type follows ] (outside the brackets),
            // then [...] is the array size, not a generic type argument.
            if let Some(name) = (names.len() == 1 && self.current_step.1 == Token::LBRACK)
                .then(|| names.pop())
                .flatten()
            {
                let lbrack = self.token(Token::LBRACK).required()?;

                // Handle slice type []T first
                if self.current_step.1 == Token::RBRACK {
                    let _rbrack = self.token(Token::RBRACK).required()?;
                    let elt = Box::new(self.parse_type().required()?);
                    let array_type = ast::Expr::ArrayType(ast::ArrayType {
                        lbrack: lbrack.0,
                        len: None,
                        elt,
                    });
                    let tag = self.parse_tag()?;
                    return Ok(Some(ast::Field {
                        doc: None,
                        names: Some(vec![name]),
                        type_: Some(array_type),
                        tag,
                        comment: None,
                    }));
                }

                // Parse what's inside [...] - could be:
                // - Single expression (array size or single type arg)
                // - Multiple types separated by commas (multiple type args)
                let first_inner = self.parse_expression().required()?;

                // Check for comma (multiple type arguments)
                if self.current_step.1 == Token::COMMA {
                    // This is a generic type with multiple type args: name[T, V, ...]
                    let mut indices = vec![first_inner];
                    while self.token(Token::COMMA)?.is_some() {
                        if self.current_step.1 == Token::RBRACK {
                            break;
                        }
                        indices.push(self.parse_type().required()?);
                    }
                    let rbrack = self.token(Token::RBRACK).required()?;

                    let type_expr = ast::Expr::IndexListExpr(ast::IndexListExpr {
                        x: Box::new(ast::Expr::Ident(name)),
                        lbrack: lbrack.0,
                        indices,
                        rbrack: rbrack.0,
                    });
                    let tag = self.parse_tag()?;
                    return Ok(Some(ast::Field {
                        doc: None,
                        type_: Some(type_expr),
                        names: None,
                        tag,
                        comment: None,
                    }));
                }

                let rbrack = self.token(Token::RBRACK).required()?;

                // Check what follows ]
                // If a type follows, this is field 'name' with array type [inner]element
                // Otherwise, it's an embedded generic field name[inner]
                if let Some(elt) = self.parse_type()? {
                    // Array type: 'name' is field name, [inner] is array size
                    let array_type = ast::Expr::ArrayType(ast::ArrayType {
                        lbrack: lbrack.0,
                        len: Some(Box::new(first_inner)),
                        elt: Box::new(elt),
                    });
                    let tag = self.parse_tag()?;
                    return Ok(Some(ast::Field {
                        doc: None,
                        names: Some(vec![name]),
                        type_: Some(array_type),
                        tag,
                        comment: None,
                    }));
                } else {
                    // Generic type: 'name' is type name, [inner] is type argument
                    let type_expr = ast::Expr::IndexExpr(ast::IndexExpr {
                        x: Box::new(ast::Expr::Ident(name)),
                        lbrack: lbrack.0,
                        index: Box::new(first_inner),
                        rbrack: rbrack.0,
                    });
                    let tag = self.parse_tag()?;
                    return Ok(Some(ast::Field {
                        doc: None,
                        type_: Some(type_expr),
                        names: None,
                        tag,
                        comment: None,
                    }));
                }
            }

            if let Some(type_) = self.parse_type()? {
                let tag = self.parse_tag()?;
                return Ok(Some(ast::Field {
                    doc: None,
                    names: Some(names),
                    type_: Some(type_),
                    tag,
                    comment: None,
                }));
            }

            if let Some(name) = (names.len() == 1).then(|| names.pop()).flatten() {
                let tag = self.parse_tag()?;
                return Ok(Some(ast::Field {
                    doc: None,
                    type_: Some(ast::Expr::Ident(name)),
                    names: None,
                    tag,
                    comment: None,
                }));
            }

            return Err(RawParserError::UnexpectedToken);
        }

        if let Some(type_) = self.parse_type_name()? {
            let tag = self.parse_tag()?;
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
    pub(super) fn parse_tag(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::parse_tag()");

        self.string_lit()
    }
}
