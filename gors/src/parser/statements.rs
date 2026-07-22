use super::{RawParserError, Result, ResultExt, core::Parser, core::is_type_switch_guard};
use crate::ast;
use crate::token::Token;

impl<'scanner> Parser<'scanner> {
    // FunctionBody = Block .
    pub(super) fn parse_function_body(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::parse_function_body()");

        self.parse_block()
    }

    // Block         = "{" StatementList "}" .
    // StatementList = { Statement ";" } .
    pub(super) fn parse_block(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::parse_block()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut list = vec![];
        while let Some(mut statement) = self.parse_statement()? {
            let consumed_semi = Self::stmt_consumed_semicolon(&statement);
            if !consumed_semi {
                if self.token(Token::SEMICOLON)?.is_some() {
                    Self::apply_line_comment_to_decl(&mut statement, self.line_comment.take());
                } else {
                    list.push(statement);
                    break;
                }
            }
            list.push(statement);
        }

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(ast::BlockStmt {
            lbrace: lbrace.0,
            list,
            rbrace: rbrace.0,
        }))
    }

    // Statement =
    //         Declaration | LabeledStmt | SimpleStmt |
    //         GoStmt | ReturnStmt | BreakStmt | ContinueStmt | GotoStmt |
    //         FallthroughStmt | Block | IfStmt | SwitchStmt | SelectStmt | ForStmt |
    //         DeferStmt .
    pub(super) fn parse_statement(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_statement()");

        use Token::*;
        Ok(match self.current_step {
            (_, CONST | TYPE | VAR, _) => Some(ast::Stmt::DeclStmt(ast::DeclStmt {
                decl: self.parse_declaration().required()?,
            })),
            (_,
                IDENT | INT | FLOAT | IMAG | CHAR | STRING | FUNC | LPAREN | // operands
                LBRACK | STRUCT | MAP | CHAN | INTERFACE | // composite types
                ADD | SUB | MUL | AND | XOR | ARROW | NOT // unary operators
            , _) => Some(self.parse_labeled_stmt_or_simple_stmt().required()?),
            (_, GO, _) => Some(ast::Stmt::GoStmt(self.parse_go_stmt().required()?)),
            (_, DEFER, _) => Some(ast::Stmt::DeferStmt(self.parse_defer_stmt().required()?)),
            (_, RETURN, _) => Some(ast::Stmt::ReturnStmt(self.parse_return_stmt().required()?)),
            (_, BREAK, _) => Some(ast::Stmt::BranchStmt(self.parse_branch_stmt().required()?)),
            (_, CONTINUE, _) => Some(ast::Stmt::BranchStmt(self.parse_branch_stmt().required()?)),
            (_, GOTO, _) => Some(ast::Stmt::BranchStmt(self.parse_branch_stmt().required()?)),
            (_, FALLTHROUGH, _) => Some(ast::Stmt::BranchStmt(self.parse_branch_stmt().required()?)),
            (_, LBRACE, _) => Some(ast::Stmt::BlockStmt(self.parse_block().required()?)),
            (_, IF, _) => Some(ast::Stmt::IfStmt(self.parse_if_stmt().required()?)),
            (_, SWITCH, _) => Some(self.parse_switch_stmt().required()?),
            (_, SELECT, _) => Some(ast::Stmt::SelectStmt(self.parse_select_stmt().required()?)),
            (_, FOR, _) => Some(self.parse_for_stmt().required()?),
            (_, SEMICOLON, lit) => Some(ast::Stmt::EmptyStmt(ast::EmptyStmt{
                semicolon: self.token(SEMICOLON).required()?.0,
                implicit: lit == "\n",
            })),
            _ => None,
        })
    }

    // ForStmt = "for" [ Condition | ForClause | RangeClause ] Block .
    // ForClause = [ InitStmt ] ";" [ Condition ] ";" [ PostStmt ] .
    // RangeClause = [ ExpressionList "=" | IdentifierList ":=" ] "range" Expression .
    // InitStmt = SimpleStmt .
    // Condition = Expression .
    // PostStmt = SimpleStmt .
    pub(super) fn parse_for_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_for_stmt()");

        let for_ = match self.token(Token::FOR)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // for {}
        if let Some(body) = self.parse_block()? {
            return Ok(Some(ast::Stmt::ForStmt(ast::ForStmt {
                for_: for_.0,
                init: None,
                cond: None,
                post: None,
                body,
            })));
        }

        // Decrement expr_level in for loop header to prevent composite literal ambiguity
        let prev_expr_level = self.expr_level;
        self.expr_level = -1;

        // for range x {}
        if let Some(range_tok) = self.token(Token::RANGE)? {
            let x = self.parse_expression().required()?;
            self.expr_level = prev_expr_level;
            let body = self.parse_block().required()?;
            return Ok(Some(ast::Stmt::RangeStmt(ast::RangeStmt {
                for_: for_.0,
                key: None,
                value: None,
                tok_pos: None,
                tok: None,
                range: range_tok.0,
                x,
                body,
            })));
        }

        let init = if let Some(mut exprs) = self.parse_expression_list()? {
            // for a < b {}
            if exprs.len() == 1 {
                self.expr_level = prev_expr_level;
                if let Some(body) = self.parse_block()? {
                    // Safe: we verified len == 1, so pop() returns Some
                    if let Some(cond) = exprs.pop() {
                        return Ok(Some(ast::Stmt::ForStmt(ast::ForStmt {
                            for_: for_.0,
                            init: None,
                            cond: Some(cond),
                            post: None,
                            body,
                        })));
                    }
                }
                self.expr_level = -1;
            }

            let mut tok = None;

            // for a, b := range x {}
            if let Some(define) = self.token(Token::DEFINE)? {
                tok = Some(define);
                if let Some(range_tok) = self.token(Token::RANGE)? {
                    let mut exprs_iter = exprs.into_iter();
                    let key = exprs_iter.next();
                    let value = exprs_iter.next();
                    let x = self.parse_expression().required()?;
                    self.expr_level = prev_expr_level;
                    let body = self.parse_block().required()?;
                    return Ok(Some(ast::Stmt::RangeStmt(ast::RangeStmt {
                        for_: for_.0,
                        key,
                        value,
                        tok_pos: Some(define.0),
                        tok: Some(define.1),
                        range: range_tok.0,
                        x,
                        body,
                    })));
                }

            // for a, b = range x {} (left side can be any expressions, not just identifiers)
            } else if let Some(assign) = self.token(Token::ASSIGN)? {
                tok = Some(assign);
                if let Some(range_tok) = self.token(Token::RANGE)? {
                    let mut exprs = exprs.into_iter();
                    let key = exprs.next();
                    let value = exprs.next();
                    let x = self.parse_expression().required()?;
                    self.expr_level = prev_expr_level;
                    let body = self.parse_block().required()?;
                    return Ok(Some(ast::Stmt::RangeStmt(ast::RangeStmt {
                        for_: for_.0,
                        key,
                        value,
                        tok_pos: Some(assign.0),
                        tok: Some(assign.1),
                        range: range_tok.0,
                        x,
                        body,
                    })));
                }
            }

            match tok {
                Some(tok) => Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                    lhs: exprs,
                    tok_pos: tok.0,
                    tok: tok.1,
                    rhs: self.parse_expression_list().required()?,
                })),
                _ => {
                    // Handle assignment statements (e.g., for s.start = s.next; ...)
                    if let Some(assign_op) = self.assign_op()? {
                        let rhs = self.parse_expression_list().required()?;
                        Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                            lhs: exprs,
                            tok_pos: assign_op.0,
                            tok: assign_op.1,
                            rhs,
                        }))
                    } else if let Some(expr) = (exprs.len() == 1)
                        .then(|| exprs.into_iter().next())
                        .flatten()
                    {
                        // Handle IncDecStmt (e.g., for p.seq++; ; p.seq++ {})
                        if let Some(inc) = self.token(Token::INC)? {
                            Some(ast::Stmt::IncDecStmt(ast::IncDecStmt {
                                tok: inc.1,
                                tok_pos: inc.0,
                                x: expr,
                            }))
                        } else if let Some(dec) = self.token(Token::DEC)? {
                            Some(ast::Stmt::IncDecStmt(ast::IncDecStmt {
                                tok: dec.1,
                                tok_pos: dec.0,
                                x: expr,
                            }))
                        } else {
                            // ExpressionStmt (e.g., for f(); cond; {})
                            Some(ast::Stmt::ExprStmt(ast::ExprStmt { x: expr }))
                        }
                    } else {
                        return Err(RawParserError::UnexpectedToken);
                    }
                }
            }
        } else {
            self.parse_simple_stmt()?
        };

        // for a;b;c {}
        self.token(Token::SEMICOLON).required()?;
        let cond = self.parse_expression()?;
        self.token(Token::SEMICOLON).required()?;
        let post = self.parse_simple_stmt()?;
        self.expr_level = prev_expr_level;
        let body = self.parse_block().required()?;
        Ok(Some(ast::Stmt::ForStmt(ast::ForStmt {
            for_: for_.0,
            init: init.map(Box::new),
            cond,
            post: post.map(Box::new),
            body,
        })))
    }

    // GoStmt = "go" Expression .
    pub(super) fn parse_go_stmt(&mut self) -> Result<Option<ast::GoStmt<'scanner>>> {
        log::debug!("Parser::parse_go_stmt()");

        let go = match self.token(Token::GO)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let call = match self.parse_expression().required()? {
            ast::Expr::CallExpr(v) => v,
            _ => return Err(RawParserError::UnexpectedToken),
        };

        Ok(Some(ast::GoStmt { go: go.0, call }))
    }

    // IfStmt = "if" [ SimpleStmt ";" ] Expression Block [ "else" ( IfStmt | Block ) ] .
    pub(super) fn parse_if_stmt(&mut self) -> Result<Option<ast::IfStmt<'scanner>>> {
        log::debug!("Parser::parse_if_stmt()");

        let if_ = match self.token(Token::IF)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Decrement expr_level in if condition to prevent composite literal ambiguity
        let prev_expr_level = self.expr_level;
        self.expr_level = -1;

        // Handle: if cond {}, if init; cond {}, if ; cond {} (empty init)
        let (init, cond) = if self.token(Token::SEMICOLON)?.is_some() {
            // Empty init statement: if ; cond {}
            (None, self.parse_expression().required()?)
        } else if let Some(simple_stmt) = self.parse_simple_stmt()? {
            if self.token(Token::SEMICOLON)?.is_some() {
                (Some(simple_stmt), self.parse_expression().required()?)
            } else if let ast::Stmt::ExprStmt(expr_stmt) = simple_stmt {
                (None, expr_stmt.x)
            } else {
                return Err(RawParserError::UnexpectedToken);
            }
        } else {
            (None, self.parse_expression().required()?)
        };

        self.expr_level = prev_expr_level;
        let body = self.parse_block().required()?;

        let else_ = if self.token(Token::ELSE)?.is_some() {
            if let Some(if_stmt) = self.parse_if_stmt()? {
                Some(ast::Stmt::IfStmt(if_stmt))
            } else if let Some(block_stmt) = self.parse_block()? {
                Some(ast::Stmt::BlockStmt(block_stmt))
            } else {
                return Err(RawParserError::UnexpectedToken);
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
    // SendStmt       = Channel "<-" Expression .
    // Channel        = Expression .
    pub(super) fn parse_simple_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_simple_stmt()");

        if let Some(mut exprs) = self.parse_expression_list()? {
            // ShortVarDecl — Go's parser accepts any expression list on LHS,
            // semantic validation is deferred to the type checker
            if let Some(define_op) = self.token(Token::DEFINE)? {
                let rhs = self.parse_expression_list().required()?;
                return Ok(Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                    lhs: exprs,
                    tok_pos: define_op.0,
                    tok: define_op.1,
                    rhs,
                })));
            }

            // Assignment
            if let Some(assign_op) = self.assign_op()? {
                let rhs = self.parse_expression_list().required()?;
                return Ok(Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                    lhs: exprs,
                    tok_pos: assign_op.0,
                    tok: assign_op.1,
                    rhs,
                })));
            }

            if let Some(expr) = (exprs.len() == 1).then(|| exprs.pop()).flatten() {
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
                    let value = self.parse_expression().required()?;
                    return Ok(Some(ast::Stmt::SendStmt(ast::SendStmt {
                        chan: expr,
                        arrow: arrow.0,
                        value,
                    })));
                }

                // ExpressionStmt
                return Ok(Some(ast::Stmt::ExprStmt(ast::ExprStmt { x: expr })));
            }

            return Err(RawParserError::UnexpectedToken);
        }

        Ok(None)
    }

    // DeferStmt = "defer" Expression .
    pub(super) fn parse_defer_stmt(&mut self) -> Result<Option<ast::DeferStmt<'scanner>>> {
        log::debug!("Parser::parse_defer_stmt()");

        let defer = match self.token(Token::DEFER)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let call = match self.parse_expression().required()? {
            ast::Expr::CallExpr(v) => v,
            _ => return Err(RawParserError::UnexpectedToken),
        };

        Ok(Some(ast::DeferStmt {
            defer: defer.0,
            call,
        }))
    }

    // ReturnStmt = "return" [ ExpressionList ] .
    pub(super) fn parse_return_stmt(&mut self) -> Result<Option<ast::ReturnStmt<'scanner>>> {
        log::debug!("Parser::parse_return_stmt()");

        if let Some(return_) = self.token(Token::RETURN)? {
            let results = self.parse_expression_list()?.unwrap_or_default();
            Ok(Some(ast::ReturnStmt {
                return_: return_.0,
                results,
            }))
        } else {
            Ok(None)
        }
    }

    // BranchStmt = ( "break" | "continue" | "goto" | "fallthrough" ) [ Label ] .
    // Label = identifier .
    pub(super) fn parse_branch_stmt(&mut self) -> Result<Option<ast::BranchStmt<'scanner>>> {
        log::debug!("Parser::parse_branch_stmt()");

        use Token::*;
        let tok_step = match self.current_step.1 {
            BREAK | CONTINUE | GOTO | FALLTHROUGH => {
                let step = self.current_step;
                self.next()?;
                step
            }
            _ => return Ok(None),
        };

        let label = if tok_step.1 != FALLTHROUGH {
            self.identifier()?
        } else {
            None
        };

        Ok(Some(ast::BranchStmt {
            tok_pos: tok_step.0,
            tok: tok_step.1,
            label,
        }))
    }

    // SwitchStmt = ExprSwitchStmt | TypeSwitchStmt .
    // ExprSwitchStmt = "switch" [ SimpleStmt ";" ] [ Expression ] "{" { ExprCaseClause } "}" .
    // TypeSwitchStmt = "switch" [ SimpleStmt ";" ] TypeSwitchGuard "{" { TypeCaseClause } "}" .
    // TypeSwitchGuard = [ identifier ":=" ] PrimaryExpr "." "(" "type" ")" .
    pub(super) fn parse_switch_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_switch_stmt()");

        let switch = match self.token(Token::SWITCH)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut init: Option<ast::Stmt<'scanner>> = None;
        let mut tag: Option<ast::Expr<'scanner>> = None;

        // Decrement expr_level in switch header to prevent composite literal ambiguity
        let prev_expr_level = self.expr_level;
        self.expr_level = -1;

        // Parse optional init and/or tag
        if self.current_step.1 != Token::LBRACE {
            // Handle empty init statement: switch ; { ... }
            if self.token(Token::SEMICOLON)?.is_some() {
                // Empty init, continue to parse tag if present
                if self.current_step.1 != Token::LBRACE {
                    if let Some(expr_or_stmt) = self.parse_simple_stmt()? {
                        if let ast::Stmt::ExprStmt(expr_stmt) = &expr_or_stmt {
                            if is_type_switch_guard(&expr_stmt.x) {
                                self.expr_level = prev_expr_level;
                                let body = self.parse_switch_body(true)?;
                                return Ok(Some(ast::Stmt::TypeSwitchStmt(ast::TypeSwitchStmt {
                                    switch: switch.0,
                                    init: None,
                                    assign: Box::new(expr_or_stmt),
                                    body,
                                })));
                            }
                        }
                        if let ast::Stmt::AssignStmt(ref assign) = expr_or_stmt {
                            if assign.rhs.len() == 1
                                && assign.rhs.first().is_some_and(is_type_switch_guard)
                            {
                                self.expr_level = prev_expr_level;
                                let body = self.parse_switch_body(true)?;
                                return Ok(Some(ast::Stmt::TypeSwitchStmt(ast::TypeSwitchStmt {
                                    switch: switch.0,
                                    init: None,
                                    assign: Box::new(expr_or_stmt),
                                    body,
                                })));
                            }
                        }
                        if let ast::Stmt::ExprStmt(expr_stmt) = expr_or_stmt {
                            tag = Some(expr_stmt.x);
                        }
                    }
                }
            } else if let Some(simple_stmt) = self.parse_simple_stmt()? {
                if self.token(Token::SEMICOLON)?.is_some() {
                    init = Some(simple_stmt);
                    // Check for type switch guard or expression
                    if self.current_step.1 != Token::LBRACE {
                        if let Some(expr_or_stmt) = self.parse_simple_stmt()? {
                            // Check if this is a type switch guard
                            if let ast::Stmt::ExprStmt(expr_stmt) = &expr_or_stmt {
                                if is_type_switch_guard(&expr_stmt.x) {
                                    self.expr_level = prev_expr_level;
                                    let body = self.parse_switch_body(true)?;
                                    return Ok(Some(ast::Stmt::TypeSwitchStmt(
                                        ast::TypeSwitchStmt {
                                            switch: switch.0,
                                            init: init.map(Box::new),
                                            assign: Box::new(expr_or_stmt),
                                            body,
                                        },
                                    )));
                                }
                            }
                            if let ast::Stmt::AssignStmt(ref assign) = expr_or_stmt {
                                if assign.rhs.len() == 1
                                    && assign.rhs.first().is_some_and(is_type_switch_guard)
                                {
                                    self.expr_level = prev_expr_level;
                                    let body = self.parse_switch_body(true)?;
                                    return Ok(Some(ast::Stmt::TypeSwitchStmt(
                                        ast::TypeSwitchStmt {
                                            switch: switch.0,
                                            init: init.map(Box::new),
                                            assign: Box::new(expr_or_stmt),
                                            body,
                                        },
                                    )));
                                }
                            }
                            // It's an expression switch
                            if let ast::Stmt::ExprStmt(expr_stmt) = expr_or_stmt {
                                tag = Some(expr_stmt.x);
                            }
                        }
                    }
                } else {
                    // Check if simple_stmt is a type switch guard
                    if let ast::Stmt::ExprStmt(expr_stmt) = &simple_stmt {
                        if is_type_switch_guard(&expr_stmt.x) {
                            self.expr_level = prev_expr_level;
                            let body = self.parse_switch_body(true)?;
                            return Ok(Some(ast::Stmt::TypeSwitchStmt(ast::TypeSwitchStmt {
                                switch: switch.0,
                                init: None,
                                assign: Box::new(simple_stmt),
                                body,
                            })));
                        }
                    }
                    if let ast::Stmt::AssignStmt(ref assign) = simple_stmt {
                        if assign.rhs.len() == 1
                            && assign.rhs.first().is_some_and(is_type_switch_guard)
                        {
                            self.expr_level = prev_expr_level;
                            let body = self.parse_switch_body(true)?;
                            return Ok(Some(ast::Stmt::TypeSwitchStmt(ast::TypeSwitchStmt {
                                switch: switch.0,
                                init: None,
                                assign: Box::new(simple_stmt),
                                body,
                            })));
                        }
                    }
                    // It's an expression switch tag
                    if let ast::Stmt::ExprStmt(expr_stmt) = simple_stmt {
                        tag = Some(expr_stmt.x);
                    }
                }
            }
        }

        self.expr_level = prev_expr_level;
        let body = self.parse_switch_body(false)?;
        Ok(Some(ast::Stmt::SwitchStmt(ast::SwitchStmt {
            switch: switch.0,
            init: init.map(Box::new),
            tag,
            body,
        })))
    }

    pub(super) fn parse_switch_body(
        &mut self,
        is_type_switch: bool,
    ) -> Result<ast::BlockStmt<'scanner>> {
        let lbrace = self.token(Token::LBRACE).required()?;

        let mut list = vec![];
        while let Some(case_clause) = self.parse_case_clause(is_type_switch)? {
            list.push(ast::Stmt::CaseClause(case_clause));
        }

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(ast::BlockStmt {
            lbrace: lbrace.0,
            list,
            rbrace: rbrace.0,
        })
    }

    // ExprCaseClause = ExprSwitchCase ":" StatementList .
    // ExprSwitchCase = "case" ExpressionList | "default" .
    // TypeCaseClause = TypeSwitchCase ":" StatementList .
    // TypeSwitchCase = "case" TypeList | "default" .
    pub(super) fn parse_case_clause(
        &mut self,
        is_type_switch: bool,
    ) -> Result<Option<ast::CaseClause<'scanner>>> {
        log::debug!("Parser::parse_case_clause()");

        let case = if let Some(case) = self.token(Token::CASE)? {
            case
        } else if let Some(default) = self.token(Token::DEFAULT)? {
            let colon = self.token(Token::COLON).required()?;
            let body = self.parse_statement_list()?;
            return Ok(Some(ast::CaseClause {
                case: default.0,
                list: None,
                colon: colon.0,
                body,
            }));
        } else {
            return Ok(None);
        };

        // In type switch, parse types; for fault tolerance, fall back to expressions
        let list = if is_type_switch {
            match self.parse_type_list()? {
                Some(list) => Some(list),
                None => self.parse_expression_list()?,
            }
        } else {
            self.parse_expression_list()?
        };
        let colon = self.token(Token::COLON).required()?;
        let body = self.parse_statement_list()?;

        Ok(Some(ast::CaseClause {
            case: case.0,
            list,
            colon: colon.0,
            body,
        }))
    }

    // StatementList = { Statement ";" } .
    pub(super) fn parse_statement_list(&mut self) -> Result<Vec<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_statement_list()");

        let mut list = vec![];
        while let Some(stmt) = self.parse_statement()? {
            // Some statements (EmptyStmt, LabeledStmt with EmptyStmt) already consumed their semicolon
            let consumed_semi = Self::stmt_consumed_semicolon(&stmt);
            list.push(stmt);
            if !consumed_semi && self.token(Token::SEMICOLON)?.is_none() {
                break;
            }
        }
        Ok(list)
    }

    // SelectStmt = "select" "{" { CommClause } "}" .
    pub(super) fn parse_select_stmt(&mut self) -> Result<Option<ast::SelectStmt<'scanner>>> {
        log::debug!("Parser::parse_select_stmt()");

        let select = match self.token(Token::SELECT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut list = vec![];
        while let Some(comm_clause) = self.parse_comm_clause()? {
            list.push(ast::Stmt::CommClause(comm_clause));
        }

        let rbrace = self.token(Token::RBRACE).required()?;

        Ok(Some(ast::SelectStmt {
            select: select.0,
            body: ast::BlockStmt {
                lbrace: lbrace.0,
                list,
                rbrace: rbrace.0,
            },
        }))
    }

    // CommClause = CommCase ":" StatementList .
    // CommCase   = "case" ( SendStmt | RecvStmt ) | "default" .
    // RecvStmt   = [ ExpressionList "=" | IdentifierList ":=" ] RecvExpr .
    // RecvExpr   = Expression .
    pub(super) fn parse_comm_clause(&mut self) -> Result<Option<ast::CommClause<'scanner>>> {
        log::debug!("Parser::parse_comm_clause()");

        let case = if let Some(case) = self.token(Token::CASE)? {
            case
        } else if let Some(default) = self.token(Token::DEFAULT)? {
            let colon = self.token(Token::COLON).required()?;
            let body = self.parse_statement_list()?;
            return Ok(Some(ast::CommClause {
                case: default.0,
                comm: None,
                colon: colon.0,
                body,
            }));
        } else {
            return Ok(None);
        };

        // Parse send/recv statement
        let comm = self.parse_simple_stmt()?;
        let colon = self.token(Token::COLON).required()?;
        let body = self.parse_statement_list()?;

        Ok(Some(ast::CommClause {
            case: case.0,
            comm: comm.map(Box::new),
            colon: colon.0,
            body,
        }))
    }

    // LabeledStmt = Label ":" Statement .
    // Label = identifier .
    // Or SimpleStmt if not a labeled statement
    pub(super) fn parse_labeled_stmt_or_simple_stmt(
        &mut self,
    ) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_labeled_stmt_or_simple_stmt()");

        // Try to parse as SimpleStmt first
        let stmt = self.parse_simple_stmt()?;

        // Check if it's a labeled statement (identifier followed by colon)
        if let Some(ast::Stmt::ExprStmt(ref expr_stmt)) = stmt {
            if let ast::Expr::Ident(ref ident) = expr_stmt.x {
                if let Some(colon) = self.token(Token::COLON)? {
                    let label = ast::Ident {
                        name_pos: ident.name_pos,
                        name: ident.name,
                        obj: None,
                    };
                    let inner_stmt = self.parse_statement()?;
                    // If no statement follows the label, create an implicit EmptyStmt
                    // with semicolon position at the current token (e.g., the closing brace)
                    let stmt = match inner_stmt {
                        Some(s) => s,
                        None => ast::Stmt::EmptyStmt(ast::EmptyStmt {
                            semicolon: self.current_step.0,
                            implicit: true,
                        }),
                    };
                    return Ok(Some(ast::Stmt::LabeledStmt(ast::LabeledStmt {
                        label,
                        colon: colon.0,
                        stmt: Box::new(stmt),
                    })));
                }
            }
        }

        Ok(stmt)
    }
}
