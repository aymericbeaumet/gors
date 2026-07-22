use super::{RawParserError, Result};
use crate::ast;
use crate::scanner;
use crate::token::{Position, SourceOrigin, Token};

fn make_basic_lit<'a>((value_pos, kind, value): scanner::Step<'a>) -> ast::BasicLit<'a> {
    let hide_column = value_pos.column == 0;
    let mut end = value_pos;
    for c in value.chars() {
        let byte_len = c.len_utf8();
        if c == '\n' {
            end.line += 1;
            end.column = if hide_column { 0 } else { 1 };
        } else if !hide_column {
            end.column += byte_len;
        }
        end.offset += byte_len;
    }
    ast::BasicLit {
        value_pos,
        value_end: end,
        kind,
        value,
    }
}

pub(super) struct Parser<'scanner> {
    pub(super) steps: scanner::IntoIter<'scanner>,
    pub(super) current_step: scanner::Step<'scanner>,
    pub(super) expr_level: isize,
    pub(super) go_version: &'scanner str,
    pub(super) buffer: &'scanner str,
    pub(super) original_origin: SourceOrigin<'scanner>,
    pub(super) lead_comment: Option<ast::CommentGroup<'scanner>>,
    pub(super) line_comment: Option<ast::CommentGroup<'scanner>>,
    pub(super) all_comments: Vec<ast::CommentGroup<'scanner>>,
}

impl<'scanner> Parser<'scanner> {
    pub(super) fn new(
        scanner: scanner::Scanner<'scanner>,
        go_version: &'scanner str,
        buffer: &'scanner str,
        filename: &'scanner str,
    ) -> Self {
        Self {
            steps: scanner.into_iter(),
            current_step: (Position::default(), Token::EOF, ""),
            expr_level: 0,
            go_version,
            buffer,
            original_origin: SourceOrigin::initial(filename),
            lead_comment: None,
            line_comment: None,
            all_comments: Vec::new(),
        }
    }

    pub(super) fn comment_end_line(comment: &ast::Comment) -> usize {
        if comment.text.starts_with("//") {
            comment.slash.line
        } else {
            comment.slash.line + comment.text.matches('\n').count()
        }
    }

    pub(super) fn comment_end_offset(comment: &ast::Comment) -> usize {
        comment.slash.offset + comment.text.len()
    }

    pub(super) fn newlines_between(&self, start: usize, end: usize) -> usize {
        self.buffer
            .get(start..end)
            .unwrap_or_default()
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
    }

    pub(super) fn consume_comment_group(
        comments: &[ast::Comment<'scanner>],
        n: usize,
        buffer: &str,
    ) -> (ast::CommentGroup<'scanner>, usize, usize) {
        let Some(first_comment) = comments.first() else {
            return (ast::CommentGroup { list: Vec::new() }, 0, 0);
        };
        let mut list = vec![first_comment.clone()];
        let mut end_offset = Self::comment_end_offset(first_comment);
        let mut endline = Self::comment_end_line(first_comment);
        let mut consumed = 1;

        for comment in comments.iter().skip(1) {
            let gap_newlines = buffer
                .get(end_offset..comment.slash.offset)
                .unwrap_or_default()
                .bytes()
                .filter(|&b| b == b'\n')
                .count();
            if gap_newlines > n {
                break;
            }
            end_offset = Self::comment_end_offset(comment);
            endline = Self::comment_end_line(comment);
            list.push(comment.clone());
            consumed += 1;
        }

        (ast::CommentGroup { list }, endline, consumed)
    }

    /// Check if a statement already consumed its terminating semicolon.
    /// This is true for EmptyStmt and for LabeledStmt whose inner statement consumed its semicolon.
    pub(super) fn stmt_consumed_semicolon(stmt: &ast::Stmt) -> bool {
        match stmt {
            ast::Stmt::EmptyStmt(_) => true,
            ast::Stmt::LabeledStmt(ls) => Self::stmt_consumed_semicolon(&ls.stmt),
            _ => false,
        }
    }

    pub(super) fn apply_line_comment_to_decl(
        stmt: &mut ast::Stmt<'scanner>,
        comment: Option<ast::CommentGroup<'scanner>>,
    ) {
        if comment.is_none() {
            return;
        }
        if let ast::Stmt::DeclStmt(decl_stmt) = stmt {
            let gen_decl = &mut decl_stmt.decl;
            if gen_decl.lparen.is_none() {
                if let Some(spec) = gen_decl.specs.last_mut() {
                    let existing = match spec {
                        ast::Spec::ValueSpec(s) => &mut s.comment,
                        ast::Spec::TypeSpec(s) => &mut s.comment,
                        ast::Spec::ImportSpec(s) => &mut s.comment,
                    };
                    if existing.is_none() {
                        *existing = comment;
                    }
                }
            }
        }
    }

    // assign_op = [ add_op | mul_op ] "=" .
    // add_op    = "+" | "-" | "|" | "^" .
    // mul_op    = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    pub(super) fn assign_op(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::assign_op()");

        use Token::*;
        Ok(match self.current_step {
            step @ (_,
                /* "=" */
                ASSIGN |
                /* add_op "=" */
                ADD_ASSIGN | SUB_ASSIGN | OR_ASSIGN | XOR_ASSIGN |
                /* mul_op "=" */
                MUL_ASSIGN | QUO_ASSIGN | REM_ASSIGN | SHL_ASSIGN | SHR_ASSIGN | AND_ASSIGN | AND_NOT_ASSIGN
            , _) => {
                self.next()?;
                Some(step)
            }
            _ => None,
        })
    }

    // unary_op = "+" | "-" | "!" | "^" | "*" | "&" | "<-" | "~" .
    pub(super) fn unary_op(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::unary_op()");

        use Token::*;
        Ok(match self.current_step {
            step @ (_, ADD | SUB | NOT | MUL | XOR | AND | ARROW | TILDE, _) => {
                self.next()?;
                Some(step)
            }
            _ => None,
        })
    }

    // binary_op = "||" | "&&" | rel_op | add_op | mul_op .
    // rel_op    = "==" | "!=" | "<" | "<=" | ">" | ">=" .
    // add_op    = "+" | "-" | "|" | "^" .
    // mul_op    = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    pub(super) fn get_binary_op(
        &mut self,
        min_precedence: u8,
    ) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::get_binary_op()");

        use Token::*;
        Ok(match self.current_step {
            step @ (_,
                /* binary_op */
                LOR | LAND |
                /* rel_op */
                EQL | NEQ | LSS | LEQ | GTR | GEQ |
                /* add_op */
                ADD | SUB | OR | XOR |
                /* mul_op */
                MUL | QUO | REM | SHL | SHR | AND | AND_NOT
            , _) if step.1.precedence() >= min_precedence => {
                Some(step)
            }
            _ => None,
        })
    }

    pub(super) fn identifier(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::identifier()");

        self.token(Token::IDENT)?
            .map_or(Ok(None), |(name_pos, _, name)| {
                Ok(Some(ast::Ident {
                    name_pos,
                    name,
                    obj: None,
                }))
            })
    }

    pub(super) fn int_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::int_lit()");
        Ok(self.token(Token::INT)?.map(make_basic_lit))
    }

    pub(super) fn float_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::float_lit()");
        Ok(self.token(Token::FLOAT)?.map(make_basic_lit))
    }

    pub(super) fn imaginary_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::imaginary_lit()");
        Ok(self.token(Token::IMAG)?.map(make_basic_lit))
    }

    pub(super) fn rune_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::rune_lit()");
        Ok(self.token(Token::CHAR)?.map(make_basic_lit))
    }

    pub(super) fn string_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::string_lit()");
        Ok(self.token(Token::STRING)?.map(make_basic_lit))
    }

    /// Returns the current step and advances to the next one, but only if it matches the expected
    /// token. [`Parser::next`] is automatically called for you.
    pub(super) fn token(&mut self, expected: Token) -> Result<Option<scanner::Step<'scanner>>> {
        Ok(match self.current_step {
            step @ (_, tok, _) if tok == expected => {
                if expected != Token::EOF {
                    self.next()?;
                }
                Some(step)
            }
            _ => None,
        })
    }

    pub(super) fn next(&mut self) -> Result<()> {
        let prev = self.current_step.0;

        self.lead_comment = None;
        self.line_comment = None;

        let mut comments: Vec<ast::Comment<'scanner>> = Vec::new();
        loop {
            match self.steps.next() {
                Some(Ok((pos, Token::COMMENT, text))) => {
                    comments.push(ast::Comment { slash: pos, text });
                }
                Some(Ok(step)) => {
                    self.current_step = step;
                    break;
                }
                Some(Err(e)) => return Err(e.into()),
                None => {
                    return Err(RawParserError::UnexpectedEndOfFile {
                        offset: self.buffer.len(),
                    });
                }
            }
        }

        if comments.is_empty() {
            return Ok(());
        }

        let mut i = 0;

        if comments
            .first()
            .is_some_and(|comment| comment.slash.line == prev.line)
            && prev.line > 0
        {
            let (group, endline, consumed) =
                Self::consume_comment_group(comments.get(i..).unwrap_or_default(), 0, self.buffer);
            i += consumed;
            self.all_comments.push(group.clone());

            let next_on_different_line = if i < comments.len() {
                comments
                    .get(i)
                    .is_some_and(|comment| comment.slash.line != endline)
            } else {
                self.current_step.0.line != endline
                    || (self.current_step.1 == Token::SEMICOLON && self.current_step.2 == "\n")
                    || self.current_step.1 == Token::EOF
            };

            if next_on_different_line {
                self.line_comment = Some(group);
            }
        }

        let mut last_group: Option<ast::CommentGroup<'scanner>> = None;
        while i < comments.len() {
            let (group, _, consumed) =
                Self::consume_comment_group(comments.get(i..).unwrap_or_default(), 1, self.buffer);
            i += consumed;
            self.all_comments.push(group.clone());
            last_group = Some(group);
        }

        if let Some(ref group) = last_group {
            if let Some(last_comment) = group.list.last() {
                let group_end_offset = Self::comment_end_offset(last_comment);
                if self.newlines_between(group_end_offset, self.current_step.0.offset) == 1 {
                    self.lead_comment = last_group;
                }
            }
        }

        Ok(())
    }
}

/// Check if an expression is a type switch guard: x.(type)
pub(super) fn is_type_switch_guard(expr: &ast::Expr) -> bool {
    if let ast::Expr::TypeAssertExpr(type_assert) = expr {
        // Type switch guard has type_ = None (nil in Go's AST)
        return type_assert.type_.is_none();
    }
    false
}
