#![allow(non_snake_case)] // TODO: switch to parse_* function naming

use crate::ast;
use crate::scanner;
use crate::token::{Position, Token};
use std::fmt;

#[derive(Debug, Clone)]
pub enum ParserError {
    ScannerError(scanner::ScannerError),
    UnexpectedEndOfFile,
    UnexpectedToken,
    UnexpectedTokenAt {
        file: String,
        line: usize,
        column: usize,
        token: Token,
        literal: String,
    },
}

impl ParserError {
    /// Get a human-readable error message
    pub fn message(&self) -> String {
        match self {
            Self::ScannerError(e) => e.message().to_string(),
            Self::UnexpectedEndOfFile => "unexpected end of file".to_string(),
            Self::UnexpectedToken => "unexpected token".to_string(),
            Self::UnexpectedTokenAt { token, literal, .. } => {
                let token_str: &str = token.into();
                if literal.is_empty() {
                    format!("unexpected token '{}'", token_str)
                } else if token_str == literal {
                    format!("unexpected token '{}'", literal)
                } else {
                    format!("unexpected {} '{}'", token_str, literal)
                }
            }
        }
    }

    /// Get the location information if available
    pub fn location(&self) -> Option<(String, usize, usize)> {
        match self {
            Self::ScannerError(e) => Some((String::new(), e.line, e.column)),
            Self::UnexpectedTokenAt { file, line, column, .. } => {
                Some((file.clone(), *line, *column))
            }
            _ => None,
        }
    }
}

impl std::error::Error for ParserError {}

impl From<scanner::ScannerError> for ParserError {
    fn from(e: scanner::ScannerError) -> Self {
        Self::ScannerError(e)
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScannerError(e) => write!(f, "{}", e),
            Self::UnexpectedEndOfFile => write!(f, "syntax error: unexpected end of file"),
            Self::UnexpectedToken => write!(f, "syntax error: unexpected token"),
            Self::UnexpectedTokenAt { file, line, column, token, literal } => {
                let loc = if file.is_empty() {
                    format!("{}:{}", line, column)
                } else {
                    format!("{}:{}:{}", file, line, column)
                };
                let token_str: &str = token.into();
                if literal.is_empty() {
                    write!(f, "{}: syntax error: unexpected token '{}'", loc, token_str)
                } else if token_str == literal {
                    write!(f, "{}: syntax error: unexpected token '{}'", loc, literal)
                } else {
                    write!(f, "{}: syntax error: unexpected {} '{}'", loc, token_str, literal)
                }
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, ParserError>;

trait ResultExt<T> {
    fn required(self) -> Result<T>;
}

impl<T> ResultExt<T> for Result<Option<T>> {
    fn required(self) -> Result<T> {
        self.and_then(|node| node.ok_or(ParserError::UnexpectedToken))
    }
}

pub fn parse_file<'a>(filename: &'a str, buffer: &'a str) -> Result<ast::File<'a>> {
    // Extract go version from //go:build directive before parsing
    let go_version = extract_go_version(buffer);

    let scanner = scanner::Scanner::new(filename, buffer);
    let mut parser = Parser::new(scanner, go_version);
    parser.next()?;
    parser.SourceFile().required().map_err(|err| match err {
        ParserError::UnexpectedToken => ParserError::UnexpectedTokenAt {
            file: format!(
                "{}/{}",
                parser.current_step.0.directory,
                parser.current_step.0.file
            ),
            line: parser.current_step.0.line,
            column: parser.current_step.0.column,
            token: parser.current_step.1,
            literal: parser.current_step.2.to_owned(),
        },
        err => err,
    })
}

/// Extract Go version from //go:build directive in source
/// Returns the go version like "go1.9" if found, empty string otherwise
fn extract_go_version(buffer: &str) -> &str {
    // Look for //go:build directive before package declaration
    for line in buffer.lines() {
        let trimmed = line.trim();

        // Stop at package declaration
        if trimmed.starts_with("package ") {
            break;
        }

        // Look for //go:build directive
        if let Some(constraint) = trimmed.strip_prefix("//go:build ") {
            // Find go version constraint (e.g., go1.9, go1.18)
            if let Some(version) = find_go_version_in_constraint(constraint) {
                // Return a reference into the original buffer
                if let Some(pos) = buffer.find(version) {
                    return &buffer[pos..pos + version.len()];
                }
            }
        }
    }
    ""
}

/// Find go version pattern (goX.Y or goX.YZ) in a build constraint
/// Returns None if:
/// - The version is negated (e.g., !go1.17)
/// - There's an OR (||) at the top level (outside parentheses), since the
///   constraint can be satisfied without the go version in that case
fn find_go_version_in_constraint(constraint: &str) -> Option<&str> {
    let bytes = constraint.as_bytes();

    // First, check if there's an OR at the top level (not inside parentheses)
    // If so, we can't reliably extract a go version since the constraint
    // might be satisfied without it
    let mut paren_depth: i32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'|' if paren_depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                // Found || at top level
                return None;
            }
            _ => {}
        }
        i += 1;
    }

    // No top-level OR, proceed to find go version
    i = 0;
    while i < bytes.len() {
        // Track if we saw a '!' before this token (negation)
        let mut is_negated = false;

        // Skip non-alphanumeric, but track '!' for negation
        while i < bytes.len() && !bytes[i].is_ascii_alphanumeric() {
            if bytes[i] == b'!' {
                is_negated = true;
            } else if !matches!(bytes[i], b' ' | b'\t' | b'(' | b')') {
                // Reset negation on other operators like && or ||
                is_negated = false;
            }
            i += 1;
        }

        let start = i;

        // Read alphanumeric + dots token
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
            i += 1;
        }

        if i > start {
            let token = &constraint[start..i];
            // Check if it matches goX.Y or goX.YZ pattern and is not negated
            if !is_negated && token.starts_with("go") && token.len() >= 4 {
                let rest = &token[2..];
                if rest.contains('.') && rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return Some(token);
                }
            }
        }
    }

    None
}

struct Parser<'scanner> {
    steps: scanner::IntoIter<'scanner>,
    current_step: scanner::Step<'scanner>,
    expr_level: isize,
    go_version: &'scanner str,
}

impl<'scanner> Parser<'scanner> {
    pub fn new(scanner: scanner::Scanner<'scanner>, go_version: &'scanner str) -> Self {
        Self {
            steps: scanner.into_iter(),
            current_step: (Position::default(), Token::EOF, ""),
            expr_level: 0,
            go_version,
        }
    }

    /// Check if a statement already consumed its terminating semicolon.
    /// This is true for EmptyStmt and for LabeledStmt whose inner statement consumed its semicolon.
    fn stmt_consumed_semicolon(stmt: &ast::Stmt) -> bool {
        match stmt {
            ast::Stmt::EmptyStmt(_) => true,
            ast::Stmt::LabeledStmt(ls) => Self::stmt_consumed_semicolon(&ls.stmt),
            _ => false,
        }
    }

    // SourceFile = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    fn SourceFile(&mut self) -> Result<Option<ast::File<'scanner>>> {
        log::debug!("Parser::SourceFile()");

        let (package, package_name) = match self.PackageClause()? {
            Some(v) => v,
            None => return Ok(None),
        };

        self.token(Token::SEMICOLON).required()?;

        // File start is always line 1, column 1 (beginning of file)
        let file_start = Position {
            directory: package.0.directory,
            file: package.0.file,
            offset: 0,
            line: 1,
            column: 1,
        };

        let mut out = ast::File {
            doc: None,
            package: package.0,
            name: package_name,
            decls: vec![],
            file_start,
            file_end: file_start, // Will be updated at EOF
            scope: None,
            unresolved: vec![],
            comments: vec![],
            go_version: self.go_version,
        };

        while let Some(import_decl) = self.ImportDecl()? {
            self.token(Token::SEMICOLON).required()?;
            out.decls.push(ast::Decl::GenDecl(import_decl));
        }

        while let Some(top_level_decl) = self.TopLevelDecl()? {
            self.token(Token::SEMICOLON).required()?;
            out.decls.push(top_level_decl);
        }

        let eof = self.token(Token::EOF).required()?;
        out.file_end = eof.0;

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

        use Token::*;
        Ok(match self.current_step.1 {
            CONST | TYPE | VAR => Some(ast::Decl::GenDecl(self.Declaration().required()?)),
            FUNC => Some(ast::Decl::FuncDecl(
                self.FunctionDecl_or_MethodDecl().required()?,
            )),
            _ => None,
        })
    }

    // Declaration = ConstDecl | TypeDecl | VarDecl .
    fn Declaration(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
        log::debug!("Parser::Declaration()");

        Ok(match self.current_step.1 {
            Token::CONST => Some(self.ConstDecl().required()?),
            Token::TYPE => Some(self.TypeDecl().required()?),
            Token::VAR => Some(self.VarDecl().required()?),
            _ => None,
        })
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
    // TypeDef   = identifier [ TypeParameters ] Type .
    fn TypeSpec(&mut self) -> Result<Option<ast::TypeSpec<'scanner>>> {
        log::debug!("Parser::TypeSpec()");

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
            let result = self.TypeParameters()?;
            // If we got an empty list, this was [] for a slice type
            // TypeParameters already consumed [] so we need to account for that when parsing type
            match result {
                Some(field_list) if field_list.list.is_empty() => {
                    // This was [] - it's a slice type, not type params
                    // We need to construct the slice type here since [ ] was consumed
                    let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
                    let element_type = self.Type().required()?;
                    return Ok(Some(ast::TypeSpec {
                        doc: None,
                        name: Some(name),
                        type_params: None,
                        assign,
                        type_: ast::Expr::ArrayType(ast::ArrayType {
                            lbrack: field_list.opening.unwrap(),
                            len: None, // slice type has no length
                            elt: Box::new(element_type),
                        }),
                        comment: None,
                    }));
                }
                Some(mut field_list)
                    if field_list.list.len() == 1 && field_list.list[0].names.is_none() =>
                {
                    // This was [expr] - it's an array type, not type params
                    // TypeParameters stored the length expression in the type_ field
                    let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
                    let element_type = self.Type().required()?;
                    let len_expr = field_list.list.pop().and_then(|f| f.type_);
                    return Ok(Some(ast::TypeSpec {
                        doc: None,
                        name: Some(name),
                        type_params: None,
                        assign,
                        type_: ast::Expr::ArrayType(ast::ArrayType {
                            lbrack: field_list.opening.unwrap(),
                            len: len_expr.map(Box::new),
                            elt: Box::new(element_type),
                        }),
                        comment: None,
                    }));
                }
                other => other,
            }
        } else {
            None
        };

        let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);

        let type_ = self.Type().required()?;

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

        let (names, _, _) = match self.IdentifierList()? {
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

        let (names, _, _) = match self.IdentifierList()? {
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
    // Returns (identifiers, has_trailing_comma, last_is_qualified) where:
    // - has_trailing_comma is true if a comma was consumed but no identifier followed (e.g., "int," in "(int, map[...])")
    // - last_is_qualified is true if the last identifier is followed by "." (making it a qualified type)
    fn IdentifierList(&mut self) -> Result<Option<(Vec<ast::Ident<'scanner>>, bool, bool)>> {
        log::debug!("Parser::IdentifierList()");

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

    // ExpressionList = Expression { "," Expression } .
    fn ExpressionList(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
        log::debug!("Parser::ExpressionList()");

        let mut out = match self.Expression()? {
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
        while let Some(op) = self.get_binary_op(min_precedence)? {
            self.next()?;

            let mut rhs = self.UnaryExpr().required()?;
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
    fn UnaryExpr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::UnaryExpr()");

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
                let value = Box::new(self.ElementType().required()?);
                return Ok(Some(ast::Expr::ChanType(ast::ChanType {
                    begin: arrow_step.0,
                    arrow: Some(arrow_step.0),
                    dir: ast::ChanDir::RECV as u8,
                    value,
                })));
            }

            // Not followed by chan - it's a receive expression
            // The <- was already consumed, so parse the operand
            let x = Box::new(self.UnaryExpr().required()?);
            return Ok(Some(ast::Expr::UnaryExpr(ast::UnaryExpr {
                op: Token::ARROW,
                op_pos: arrow_step.0,
                x,
            })));
        }

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

        let mut primary_expr = match self.Operand()? {
            Some(v) => v,
            None => return Ok(None),
        };

        loop {
            match self.current_step.1 {
                Token::PERIOD => {
                    primary_expr = self.Selector_or_TypeAssertion(primary_expr).required()?;
                }
                Token::LBRACK => {
                    primary_expr = self.Index_or_Slice(primary_expr).required()?;
                }
                Token::LPAREN => {
                    primary_expr = self.Arguments(primary_expr).required()?;
                }
                Token::LBRACE if self.expr_level >= 0 => {
                    // Composite literal with type already parsed
                    primary_expr = self.LiteralValue(primary_expr).required()?;
                }
                _ => break,
            }
        }

        Ok(Some(primary_expr))
    }

    // LiteralValue = "{" [ ElementList [ "," ] ] "}" .
    // ElementList  = KeyedElement { "," KeyedElement } .
    // Used when type is already known from PrimaryExpr
    fn LiteralValue(&mut self, type_: ast::Expr<'scanner>) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::LiteralValue()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Inside composite literal, allow nested composite literals with elided types
        // Use max(1, ...) to ensure expr_level is positive even if it was -1
        let prev_expr_level = self.expr_level;
        self.expr_level = std::cmp::max(1, self.expr_level + 1);

        let mut elts = self.KeyedElement()?.map(|elt| vec![elt]);
        if let Some(elts) = elts.as_mut() {
            while self.token(Token::COMMA)?.is_some() {
                if let Some(k) = self.KeyedElement()? {
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
    fn Selector_or_TypeAssertion(
        &mut self,
        x: ast::Expr<'scanner>,
    ) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Selector_or_TypeAssertion()");

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
            let type_ = self.Type().required()?;
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
    fn Index_or_Slice(&mut self, x: ast::Expr<'scanner>) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Index_or_Slice()");

        let lbrack = match self.token(Token::LBRACK)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Inside brackets, composite literals are always allowed
        self.expr_level += 1;

        let low = if let Some(low) = self.Expression()? {
            // Check for comma (generic instantiation with multiple type args)
            if self.token(Token::COMMA)?.is_some() {
                let mut indices = vec![low];
                // Allow trailing comma
                if self.current_step.1 != Token::RBRACK {
                    indices.push(self.Expression().required()?);
                    while self.token(Token::COMMA)?.is_some() {
                        if self.current_step.1 == Token::RBRACK {
                            break;
                        }
                        indices.push(self.Expression().required()?);
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

        let high = if let Some(high) = self.Expression()? {
            if self.token(Token::COLON)?.is_some() {
                let max = self.Expression().required()?;
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
    fn Arguments(&mut self, x: ast::Expr<'scanner>) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Arguments()");

        let lparen = match self.token(Token::LPAREN)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Inside parentheses, composite literals are always allowed
        self.expr_level += 1;

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
    fn Operand(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Operand()");

        use Token::*;
        Ok(match self.current_step.1 {
            IDENT => Some(ast::Expr::Ident(self.identifier().required()?)),
            INT | FLOAT | IMAG | CHAR | STRING => {
                Some(ast::Expr::BasicLit(self.BasicLit().required()?))
            }
            LPAREN => {
                let lparen = self.token(Token::LPAREN).required()?;
                // Inside parentheses, composite literals are always allowed
                self.expr_level += 1;

                // First, try to parse as expression
                if let Some(expr) = self.Expression()? {
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
                if let Some(type_) = self.Type()? {
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
                return Err(ParserError::UnexpectedToken);
            }
            FUNC => {
                // Try function literal first; if no body, fall back to function type
                // (for use in type conversions like func(string)(nil))
                let func = self.token(Token::FUNC).required()?;
                let signature = self.Signature(Some(func.0)).required()?;
                // Reset expr_level when parsing function body to allow composite literals
                // inside the function, even if we're in a context (like if condition) that
                // normally disables them
                let saved_expr_level = self.expr_level;
                self.expr_level = 0;
                let body = self.FunctionBody()?;
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
            INTERFACE => Some(ast::Expr::InterfaceType(self.InterfaceType().required()?)),
            // Handle nested composite literals without explicit type
            // Go allows eliding the type for nested composite literals
            LBRACE if self.expr_level > 0 => {
                let lbrace = self.token(Token::LBRACE).required()?;
                // Inside composite literal, allow nested composite literals
                self.expr_level += 1;
                let mut elts = self.KeyedElement()?.map(|elt| vec![elt]);
                if let Some(elts) = elts.as_mut() {
                    while self.token(Token::COMMA)?.is_some() {
                        if let Some(k) = self.KeyedElement()? {
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
                if let Some(type_) = self.LiteralType()? {
                    if self.current_step.1 == Token::LBRACE {
                        // After a LiteralType, a { is always a composite literal
                        // (the ambiguity with blocks only exists at statement level)
                        let lbrace = self.token(Token::LBRACE).required()?;
                        // Inside composite literal, allow nested composite literals
                        // Use max(1, ...) to ensure expr_level is positive even if it was -1
                        let prev_expr_level = self.expr_level;
                        self.expr_level = std::cmp::max(1, self.expr_level + 1);
                        let mut elts = self.KeyedElement()?.map(|elt| vec![elt]);
                        if let Some(elts) = elts.as_mut() {
                            while self.token(Token::COMMA)?.is_some() {
                                if let Some(k) = self.KeyedElement()? {
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
    fn LiteralType(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::LiteralType()");

        Ok(match self.current_step.1 {
            Token::STRUCT => Some(ast::Expr::StructType(self.StructType().required()?)),
            Token::LBRACK => Some(ast::Expr::ArrayType(
                self.ArrayType_or_SliceType::<true>().required()?,
            )),
            Token::MAP => Some(ast::Expr::MapType(self.MapType().required()?)),
            Token::CHAN => Some(ast::Expr::ChanType(self.ChannelType().required()?)),
            Token::IDENT => Some(self.TypeName().required()?),
            _ => None,
        })
    }

    // KeyedElement = [ Key ":" ] Element .
    // Key          = FieldName | Expression | LiteralValue .
    // FieldName    = identifier .
    // Element      = Expression | LiteralValue .
    fn KeyedElement(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::KeyedElement()");

        let key = match self.Expression()? {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(colon) = self.token(Token::COLON)? {
            let value = self.Expression().required()?;
            return Ok(Some(ast::Expr::KeyValueExpr(ast::KeyValueExpr {
                key: Box::new(key),
                colon: colon.0,
                value: Box::new(value),
            })));
        }

        Ok(Some(key))
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

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    fn BasicLit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::BasicLit()");

        Ok(match self.current_step.1 {
            Token::INT => Some(self.int_lit().required()?),
            Token::FLOAT => Some(self.float_lit().required()?),
            Token::IMAG => Some(self.imaginary_lit().required()?),
            Token::CHAR => Some(self.rune_lit().required()?),
            Token::STRING => Some(self.string_lit().required()?),
            _ => None,
        })
    }

    // Type = TypeName | TypeLit | "(" Type ")" .
    fn Type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::Type()");

        if let Some(lparen) = self.token(Token::LPAREN)? {
            let type_ = self.Type().required()?;
            let rparen = self.token(Token::RPAREN).required()?;
            // Preserve the parentheses by wrapping in ParenExpr
            return Ok(Some(ast::Expr::ParenExpr(ast::ParenExpr {
                lparen: lparen.0,
                x: Box::new(type_),
                rparen: rparen.0,
            })));
        }

        if let Some(type_name) = self.TypeName()? {
            return Ok(Some(type_name));
        }

        if let Some(type_lit) = self.TypeLit()? {
            return Ok(Some(type_lit));
        }

        Ok(None)
    }

    // TypeList = Type { "," Type } .
    fn TypeList(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
        log::debug!("Parser::TypeList()");

        let first_type = match self.Type()? {
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
            types.push(self.Type().required()?);
        }

        Ok(Some(types))
    }

    // TypeName = identifier [ TypeArgs ] | QualifiedIdent [ TypeArgs ] .
    // TypeArgs = "[" TypeList [ "," ] "]" .
    fn TypeName(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeName()");

        let type_name = match self.identifier_or_QualifiedIdent()? {
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

            let mut indices = vec![self.Type().required()?];
            while self.token(Token::COMMA)?.is_some() {
                if self.current_step.1 == Token::RBRACK {
                    break;
                }
                indices.push(self.Type().required()?);
            }
            let rbrack = self.token(Token::RBRACK).required()?;

            if indices.len() == 1 {
                return Ok(Some(ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(type_name),
                    lbrack: lbrack.0,
                    index: Box::new(indices.pop().unwrap()),
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
    fn TypeLit(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeLit()");

        Ok(match self.current_step.1 {
            Token::LBRACK => Some(ast::Expr::ArrayType(
                self.ArrayType_or_SliceType::<false>().required()?,
            )),
            Token::STRUCT => Some(ast::Expr::StructType(self.StructType().required()?)),
            Token::MUL => Some(ast::Expr::StarExpr(self.PointerType().required()?)),
            Token::FUNC => Some(ast::Expr::FuncType(self.FunctionType().required()?)),
            Token::INTERFACE => Some(ast::Expr::InterfaceType(self.InterfaceType().required()?)),
            Token::MAP => Some(ast::Expr::MapType(self.MapType().required()?)),
            Token::CHAN => Some(ast::Expr::ChanType(self.ChannelType().required()?)),
            Token::ARROW => Some(ast::Expr::ChanType(self.ChannelType().required()?)), // <-chan (receive-only)
            _ => None,
        })
    }

    // ArrayType   = "[" ArrayLength "]" ElementType .
    // ArrayLength = Expression .
    // SliceType   = "[" "]" ElementType .
    fn ArrayType_or_SliceType<const ELLIPSIS: bool>(
        &mut self,
    ) -> Result<Option<ast::ArrayType<'scanner>>> {
        log::debug!("Parser::ArrayType_or_SliceType::<ELLIPSIS={}>()", ELLIPSIS);

        let lbrack = match self.token(Token::LBRACK)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let len = if ELLIPSIS {
            if let Some(ellipsis) = self.token(Token::ELLIPSIS)? {
                Some(ast::Expr::Ellipsis(ast::Ellipsis {
                    ellipsis: ellipsis.0,
                    elt: None,
                }))
            } else {
                self.Expression()?
            }
        } else {
            self.Expression()?
        };

        self.token(Token::RBRACK).required()?;

        let element_type = self.ElementType().required()?;

        Ok(Some(ast::ArrayType {
            lbrack: lbrack.0,
            len: len.map(Box::new),
            elt: Box::new(element_type),
        }))
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
                arrow: Some(arrow.0), // <-chan has arrow at the start
                dir: ast::ChanDir::RECV as u8,
                value,
            }));
        }

        Ok(None)
    }

    // FunctionType = "func" Signature .
    fn FunctionType(&mut self) -> Result<Option<ast::FuncType<'scanner>>> {
        log::debug!("Parser::FunctionType()");

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut signature = self.Signature(None).required()?;
        signature.func = Some(func.0);
        Ok(Some(signature))
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

    // InterfaceType = "interface" "{" { InterfaceElem ";" } "}" .
    // InterfaceElem = MethodElem | TypeElem .
    // MethodElem    = MethodName Signature .
    // TypeElem      = TypeTerm { "|" TypeTerm } .
    // TypeTerm      = Type | UnderlyingType .
    // UnderlyingType = "~" Type .
    fn InterfaceType(&mut self) -> Result<Option<ast::InterfaceType<'scanner>>> {
        log::debug!("Parser::InterfaceType()");

        let interface = match self.token(Token::INTERFACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut fields = vec![];
        loop {
            // Check for underlying type constraint (~Type)
            if let Some(tilde) = self.token(Token::TILDE)? {
                let type_ = self.Type().required()?;
                let mut type_elem = ast::Expr::UnaryExpr(ast::UnaryExpr {
                    op_pos: tilde.0,
                    op: Token::TILDE,
                    x: Box::new(type_),
                });

                // Check for union types
                while let Some(or_tok) = self.token(Token::OR)? {
                    let next_term = self.TypeTerm().required()?;
                    type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                        x: Box::new(type_elem),
                        op_pos: or_tok.0,
                        op: Token::OR,
                        y: Box::new(next_term),
                    });
                }

                fields.push(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(type_elem),
                    tag: None,
                    comment: None,
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            }

            // Handle embedded type literals in interface (struct, slice, array, map, func, chan, pointer)
            // These are type constraints in Go 1.18+ generics
            if matches!(
                self.current_step.1,
                Token::STRUCT
                    | Token::LBRACK
                    | Token::MAP
                    | Token::FUNC
                    | Token::CHAN
                    | Token::ARROW
                    | Token::MUL
            ) {
                let type_ = self.Type().required()?;

                // Check for union types
                let mut type_elem = type_;
                while let Some(or_tok) = self.token(Token::OR)? {
                    let next_term = self.TypeTerm().required()?;
                    type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                        x: Box::new(type_elem),
                        op_pos: or_tok.0,
                        op: Token::OR,
                        y: Box::new(next_term),
                    });
                }

                fields.push(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(type_elem),
                    tag: None,
                    comment: None,
                });
                if self.token(Token::SEMICOLON)?.is_none() {
                    break;
                }
                continue;
            }

            if let Some(method_spec) = self.MethodName()? {
                // Check if this is a qualified interface name (e.g., io.Writer)
                if self.current_step.1 == Token::PERIOD {
                    self.token(Token::PERIOD)?;
                    let sel = self.identifier().required()?;
                    fields.push(ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(ast::Expr::SelectorExpr(ast::SelectorExpr {
                            x: Box::new(ast::Expr::Ident(method_spec)),
                            sel,
                        })),
                        tag: None,
                        comment: None,
                    });
                    if self.token(Token::SEMICOLON)?.is_none() {
                        break;
                    }
                    continue;
                }

                // Check for type parameters on the embedded type (e.g., Comparable[T])
                if self.current_step.1 == Token::LBRACK {
                    let lbrack = self.token(Token::LBRACK).required()?;
                    let mut indices = vec![self.Type().required()?];
                    while self.token(Token::COMMA)?.is_some() {
                        if self.current_step.1 == Token::RBRACK {
                            break;
                        }
                        indices.push(self.Type().required()?);
                    }
                    let rbrack = self.token(Token::RBRACK).required()?;

                    let type_expr = if indices.len() == 1 {
                        ast::Expr::IndexExpr(ast::IndexExpr {
                            x: Box::new(ast::Expr::Ident(method_spec)),
                            lbrack: lbrack.0,
                            index: Box::new(indices.pop().unwrap()),
                            rbrack: rbrack.0,
                        })
                    } else {
                        ast::Expr::IndexListExpr(ast::IndexListExpr {
                            x: Box::new(ast::Expr::Ident(method_spec)),
                            lbrack: lbrack.0,
                            indices,
                            rbrack: rbrack.0,
                        })
                    };

                    // Check for union with other types
                    let mut type_elem = type_expr;
                    while let Some(or_tok) = self.token(Token::OR)? {
                        let next_term = self.TypeTerm().required()?;
                        type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                            x: Box::new(type_elem),
                            op_pos: or_tok.0,
                            op: Token::OR,
                            y: Box::new(next_term),
                        });
                    }

                    fields.push(ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(type_elem),
                        tag: None,
                        comment: None,
                    });
                    if self.token(Token::SEMICOLON)?.is_none() {
                        break;
                    }
                    continue;
                }

                if let Some(signature) = self.Signature(None)? {
                    fields.push(ast::Field {
                        doc: None,
                        names: Some(vec![method_spec]),
                        type_: Some(ast::Expr::FuncType(signature)),
                        tag: None,
                        comment: None,
                    });
                    if self.token(Token::SEMICOLON)?.is_none() {
                        break;
                    }
                    continue;
                }

                // Could be a simple type name or part of a union type
                let mut type_elem = ast::Expr::Ident(method_spec);

                // Check for union types (Type1 | Type2)
                while let Some(or_tok) = self.token(Token::OR)? {
                    let next_term = self.TypeTerm().required()?;
                    type_elem = ast::Expr::BinaryExpr(ast::BinaryExpr {
                        x: Box::new(type_elem),
                        op_pos: or_tok.0,
                        op: Token::OR,
                        y: Box::new(next_term),
                    });
                }

                fields.push(ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(type_elem),
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

        if let Some((names, _, last_is_qualified)) = self.IdentifierList()? {
            // Check if this is a qualified identifier for an embedded field (e.g., sync.RWMutex)
            // or a qualified generic type (e.g., listers.ResourceIndexer[*Deployment])
            if names.len() == 1 && (self.current_step.1 == Token::PERIOD || last_is_qualified) {
                let name = names.into_iter().next().unwrap();
                self.token(Token::PERIOD)?;
                let sel = self.identifier().required()?;

                // Check for generic type arguments [T] or [T1, T2]
                let type_expr = if self.current_step.1 == Token::LBRACK {
                    let lbrack = self.token(Token::LBRACK).required()?;
                    let mut indices = vec![self.Type().required()?];
                    while self.token(Token::COMMA)?.is_some() {
                        if self.current_step.1 == Token::RBRACK {
                            break;
                        }
                        indices.push(self.Type().required()?);
                    }
                    let rbrack = self.token(Token::RBRACK).required()?;

                    let selector = ast::Expr::SelectorExpr(ast::SelectorExpr {
                        x: Box::new(ast::Expr::Ident(name)),
                        sel,
                    });

                    if indices.len() == 1 {
                        ast::Expr::IndexExpr(ast::IndexExpr {
                            x: Box::new(selector),
                            lbrack: lbrack.0,
                            index: Box::new(indices.pop().unwrap()),
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

                let tag = self.Tag()?;
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
            if names.len() == 1 && self.current_step.1 == Token::LBRACK {
                let name = names.into_iter().next().unwrap();
                let lbrack = self.token(Token::LBRACK).required()?;

                // Handle slice type []T first
                if self.current_step.1 == Token::RBRACK {
                    let _rbrack = self.token(Token::RBRACK).required()?;
                    let elt = Box::new(self.Type().required()?);
                    let array_type = ast::Expr::ArrayType(ast::ArrayType {
                        lbrack: lbrack.0,
                        len: None,
                        elt,
                    });
                    let tag = self.Tag()?;
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
                let first_inner = self.Expression().required()?;

                // Check for comma (multiple type arguments)
                if self.current_step.1 == Token::COMMA {
                    // This is a generic type with multiple type args: name[T, V, ...]
                    let mut indices = vec![first_inner];
                    while self.token(Token::COMMA)?.is_some() {
                        if self.current_step.1 == Token::RBRACK {
                            break;
                        }
                        indices.push(self.Type().required()?);
                    }
                    let rbrack = self.token(Token::RBRACK).required()?;

                    let type_expr = ast::Expr::IndexListExpr(ast::IndexListExpr {
                        x: Box::new(ast::Expr::Ident(name)),
                        lbrack: lbrack.0,
                        indices,
                        rbrack: rbrack.0,
                    });
                    let tag = self.Tag()?;
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
                if let Some(elt) = self.Type()? {
                    // Array type: 'name' is field name, [inner] is array size
                    let array_type = ast::Expr::ArrayType(ast::ArrayType {
                        lbrack: lbrack.0,
                        len: Some(Box::new(first_inner)),
                        elt: Box::new(elt),
                    });
                    let tag = self.Tag()?;
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
                    let tag = self.Tag()?;
                    return Ok(Some(ast::Field {
                        doc: None,
                        type_: Some(type_expr),
                        names: None,
                        tag,
                        comment: None,
                    }));
                }
            }

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
            type_params: None,
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
            .inspect(|_| {
                let _ = self.token(Token::COMMA);
            })
            .unwrap_or_default();
        let rparen = self.token(Token::RPAREN).required()?;

        Ok(Some(ast::FieldList {
            opening: Some(lparen.0),
            list,
            closing: Some(rparen.0),
        }))
    }

    // ParameterList = ParameterDecl { "," ParameterDecl } .
    // ParameterDecl = [ IdentifierList ] [ "..." ] Type .
    fn ParameterList(&mut self) -> Result<Option<Vec<ast::Field<'scanner>>>> {
        log::debug!("Parser::ParameterList()");

        // First, try to parse identifiers
        let idents_result = self.IdentifierList()?;

        // If no identifiers, try to parse just a type (unnamed parameter like "*T" or "interface{}")
        if idents_result.is_none() {
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.Type()?;
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let type_ = self.Type().required()?;
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
                return Ok(Some(fields));
            }
            return Ok(None);
        }

        let (idents, has_trailing_comma, last_is_qualified) = idents_result.unwrap();

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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let type_ = self.Type().required()?;
                    fields.push(ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(type_),
                        tag: None,
                        comment: None,
                    });
                }
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
                let type_ = self.Type().required()?;
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let type_ = self.Type().required()?;
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
            let pkg_ident = idents.into_iter().next().unwrap();
            self.token(Token::PERIOD)?;
            let sel = self.identifier().required()?;

            // Check if this qualified type has generic args [T]
            let type_expr = if self.current_step.1 == Token::LBRACK {
                let lbrack = self.token(Token::LBRACK).required()?;
                let mut indices = vec![self.Type().required()?];
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RBRACK {
                        break;
                    }
                    indices.push(self.Type().required()?);
                }
                let rbrack = self.token(Token::RBRACK).required()?;

                let selector = ast::Expr::SelectorExpr(ast::SelectorExpr {
                    x: Box::new(ast::Expr::Ident(pkg_ident)),
                    sel,
                });

                if indices.len() == 1 {
                    ast::Expr::IndexExpr(ast::IndexExpr {
                        x: Box::new(selector),
                        lbrack: lbrack.0,
                        index: Box::new(indices.pop().unwrap()),
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
            while self.token(Token::COMMA)?.is_some() {
                if self.current_step.1 == Token::RPAREN {
                    break;
                }
                let ellipsis = self.token(Token::ELLIPSIS)?;
                let type_ = self.Type().required()?;
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
            let ident = idents.into_iter().next().unwrap();
            let lbrack = self.token(Token::LBRACK).required()?;

            // Check for empty [] which is a slice type
            if self.current_step.1 == Token::RBRACK {
                // This is `ident []Type` - ident is param name, []Type is slice type
                let _rbrack = self.token(Token::RBRACK).required()?;
                let elt = self.Type().required()?;
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
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let (param_names, _, _) = self.IdentifierList().required()?;
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let param_type = self.Type().required()?;
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
                return Ok(Some(fields));
            }

            // Parse what's inside the brackets as an expression/type
            // This could be: a type arg (T), array length (10), or array length expr (n*2)
            // Or multiple type args (K, V)
            let first_inner = self.Expression().required()?;

            // Check for comma (multiple type arguments like [K, V])
            if self.current_step.1 == Token::COMMA {
                // This is a generic type with multiple type args: ident[T, V, ...]
                let mut indices = vec![first_inner];
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RBRACK {
                        break;
                    }
                    indices.push(self.Type().required()?);
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
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let type_ = self.Type().required()?;
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
                return Ok(Some(fields));
            }

            let rbrack_pos = self.token(Token::RBRACK).required()?.0;

            // Check what follows ]
            // If a type follows, this is `ident [expr]Type` (array type with ident as param name)
            // If ) or , follows, this is `ident[expr]` (generic type instantiation)
            if let Some(elt) = self.Type()? {
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
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let (param_names, _, _) = self.IdentifierList().required()?;
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let param_type = self.Type().required()?;
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
                while self.token(Token::COMMA)?.is_some() {
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let type_ = self.Type().required()?;
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
                return Ok(Some(fields));
            }
        }

        let type_ = self.Type()?;

        // If no type can be found and no ellipsis, then the idents might be types
        // Handle qualified types like (cipher.AEAD, error) where the first ident (cipher)
        // is actually the package part of a qualified type
        if type_.is_none() && ellipsis.is_none() {
            // Check if the first (and only) ident is followed by a period - qualified type
            if idents.len() == 1 && self.current_step.1 == Token::PERIOD {
                let ident = idents.into_iter().next().unwrap();
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
                    if self.current_step.1 == Token::RPAREN {
                        break;
                    }
                    let ellipsis = self.token(Token::ELLIPSIS)?;
                    let type_ = self.Type().required()?;
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
            while self.token(Token::COMMA)?.is_some() {
                // Handle trailing comma
                if self.current_step.1 == Token::RPAREN {
                    break;
                }
                let ellipsis = self.token(Token::ELLIPSIS)?;
                let type_ = self.Type().required()?;
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
            let (idents, _, _) = self.IdentifierList().required()?;
            let ellipsis = self.token(Token::ELLIPSIS)?;
            let type_ = self.Type().required()?;

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

    // FunctionBody = Block .
    fn FunctionBody(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::FunctionBody()");

        self.Block()
    }

    // Block         = "{" StatementList "}" .
    // StatementList = { Statement ";" } .
    fn Block(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::Block()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut list = vec![];
        while let Some(statement) = self.Statement()? {
            // Some statements (EmptyStmt, LabeledStmt with EmptyStmt) already consumed their semicolon
            let consumed_semi = Self::stmt_consumed_semicolon(&statement);
            list.push(statement);
            if !consumed_semi && self.token(Token::SEMICOLON)?.is_none() {
                break;
            }
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
    fn Statement(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::Statement()");

        use Token::*;
        Ok(match self.current_step {
            (_, CONST | TYPE | VAR, _) => Some(ast::Stmt::DeclStmt(ast::DeclStmt {
                decl: self.Declaration().required()?,
            })),
            (_,
                IDENT | INT | FLOAT | IMAG | CHAR | STRING | FUNC | LPAREN | // operands
                LBRACK | STRUCT | MAP | CHAN | INTERFACE | // composite types
                ADD | SUB | MUL | AND | XOR | ARROW | NOT // unary operators
            , _) => Some(self.LabeledStmt_or_SimpleStmt().required()?),
            (_, GO, _) => Some(ast::Stmt::GoStmt(self.GoStmt().required()?)),
            (_, DEFER, _) => Some(ast::Stmt::DeferStmt(self.DeferStmt().required()?)),
            (_, RETURN, _) => Some(ast::Stmt::ReturnStmt(self.ReturnStmt().required()?)),
            (_, BREAK, _) => Some(ast::Stmt::BranchStmt(self.BranchStmt().required()?)),
            (_, CONTINUE, _) => Some(ast::Stmt::BranchStmt(self.BranchStmt().required()?)),
            (_, GOTO, _) => Some(ast::Stmt::BranchStmt(self.BranchStmt().required()?)),
            (_, FALLTHROUGH, _) => Some(ast::Stmt::BranchStmt(self.BranchStmt().required()?)),
            (_, LBRACE, _) => Some(ast::Stmt::BlockStmt(self.Block().required()?)),
            (_, IF, _) => Some(ast::Stmt::IfStmt(self.IfStmt().required()?)),
            (_, SWITCH, _) => Some(self.SwitchStmt().required()?),
            (_, SELECT, _) => Some(ast::Stmt::SelectStmt(self.SelectStmt().required()?)),
            (_, FOR, _) => Some(self.ForStmt().required()?),
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

        // Decrement expr_level in for loop header to prevent composite literal ambiguity
        let prev_expr_level = self.expr_level;
        self.expr_level = -1;

        // for range x {}
        if let Some(range_tok) = self.token(Token::RANGE)? {
            let x = self.Expression().required()?;
            self.expr_level = prev_expr_level;
            let body = self.Block().required()?;
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

        let init = if let Some(exprs) = self.ExpressionList()? {
            // for a < b {}
            if exprs.len() == 1 {
                self.expr_level = prev_expr_level;
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
                self.expr_level = -1;
            }

            let mut tok = None;

            // for a, b := range x {}
            if let Some(define) = self.token(Token::DEFINE)? {
                tok = Some(define);
                if let Some(range_tok) = self.token(Token::RANGE)? {
                    let mut exprs = exprs.into_iter();
                    let key = exprs.next();
                    let value = exprs.next();
                    let x = self.Expression().required()?;
                    self.expr_level = prev_expr_level;
                    let body = self.Block().required()?;
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
                    let x = self.Expression().required()?;
                    self.expr_level = prev_expr_level;
                    let body = self.Block().required()?;
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
                    rhs: self.ExpressionList().required()?,
                })),
                _ => {
                    // Handle assignment statements (e.g., for s.start = s.next; ...)
                    if let Some(assign_op) = self.assign_op()? {
                        let rhs = self.ExpressionList().required()?;
                        Some(ast::Stmt::AssignStmt(ast::AssignStmt {
                            lhs: exprs,
                            tok_pos: assign_op.0,
                            tok: assign_op.1,
                            rhs,
                        }))
                    } else if exprs.len() == 1 {
                        // Handle IncDecStmt (e.g., for p.seq++; ; p.seq++ {})
                        let expr = exprs.into_iter().next().unwrap();
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
                        return Err(ParserError::UnexpectedToken);
                    }
                }
            }
        } else {
            self.SimpleStmt()?
        };

        // for a;b;c {}
        self.token(Token::SEMICOLON).required()?;
        let cond = self.Expression()?;
        self.token(Token::SEMICOLON).required()?;
        let post = self.SimpleStmt()?;
        self.expr_level = prev_expr_level;
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

        // Decrement expr_level in if condition to prevent composite literal ambiguity
        let prev_expr_level = self.expr_level;
        self.expr_level = -1;

        // Handle: if cond {}, if init; cond {}, if ; cond {} (empty init)
        let (init, cond) = if self.token(Token::SEMICOLON)?.is_some() {
            // Empty init statement: if ; cond {}
            (None, self.Expression().required()?)
        } else if let Some(simple_stmt) = self.SimpleStmt()? {
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

        self.expr_level = prev_expr_level;
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
    // SendStmt       = Channel "<-" Expression .
    // Channel        = Expression .
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

    // DeferStmt = "defer" Expression .
    fn DeferStmt(&mut self) -> Result<Option<ast::DeferStmt<'scanner>>> {
        log::debug!("Parser::DeferStmt()");

        let defer = match self.token(Token::DEFER)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let call = match self.Expression().required()? {
            ast::Expr::CallExpr(v) => v,
            _ => return Err(ParserError::UnexpectedToken),
        };

        Ok(Some(ast::DeferStmt {
            defer: defer.0,
            call,
        }))
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

    // BranchStmt = ( "break" | "continue" | "goto" | "fallthrough" ) [ Label ] .
    // Label = identifier .
    fn BranchStmt(&mut self) -> Result<Option<ast::BranchStmt<'scanner>>> {
        log::debug!("Parser::BranchStmt()");

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
    fn SwitchStmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::SwitchStmt()");

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
                    if let Some(expr_or_stmt) = self.SimpleStmt()? {
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
                            if assign.rhs.len() == 1 && is_type_switch_guard(&assign.rhs[0]) {
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
            } else if let Some(simple_stmt) = self.SimpleStmt()? {
                if self.token(Token::SEMICOLON)?.is_some() {
                    init = Some(simple_stmt);
                    // Check for type switch guard or expression
                    if self.current_step.1 != Token::LBRACE {
                        if let Some(expr_or_stmt) = self.SimpleStmt()? {
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
                                if assign.rhs.len() == 1 && is_type_switch_guard(&assign.rhs[0]) {
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
                        if assign.rhs.len() == 1 && is_type_switch_guard(&assign.rhs[0]) {
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

    fn parse_switch_body(&mut self, is_type_switch: bool) -> Result<ast::BlockStmt<'scanner>> {
        let lbrace = self.token(Token::LBRACE).required()?;

        let mut list = vec![];
        while let Some(case_clause) = self.CaseClause(is_type_switch)? {
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
    fn CaseClause(&mut self, is_type_switch: bool) -> Result<Option<ast::CaseClause<'scanner>>> {
        log::debug!("Parser::CaseClause()");

        let case = if let Some(case) = self.token(Token::CASE)? {
            case
        } else if let Some(default) = self.token(Token::DEFAULT)? {
            let colon = self.token(Token::COLON).required()?;
            let body = self.StatementList()?;
            return Ok(Some(ast::CaseClause {
                case: default.0,
                list: None,
                colon: colon.0,
                body,
            }));
        } else {
            return Ok(None);
        };

        // In type switch, parse types; otherwise parse expressions
        let list = if is_type_switch {
            self.TypeList()?
        } else {
            self.ExpressionList()?
        };
        let colon = self.token(Token::COLON).required()?;
        let body = self.StatementList()?;

        Ok(Some(ast::CaseClause {
            case: case.0,
            list,
            colon: colon.0,
            body,
        }))
    }

    // StatementList = { Statement ";" } .
    fn StatementList(&mut self) -> Result<Vec<ast::Stmt<'scanner>>> {
        log::debug!("Parser::StatementList()");

        let mut list = vec![];
        while let Some(stmt) = self.Statement()? {
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
    fn SelectStmt(&mut self) -> Result<Option<ast::SelectStmt<'scanner>>> {
        log::debug!("Parser::SelectStmt()");

        let select = match self.token(Token::SELECT)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let lbrace = self.token(Token::LBRACE).required()?;

        let mut list = vec![];
        while let Some(comm_clause) = self.CommClause()? {
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
    fn CommClause(&mut self) -> Result<Option<ast::CommClause<'scanner>>> {
        log::debug!("Parser::CommClause()");

        let case = if let Some(case) = self.token(Token::CASE)? {
            case
        } else if let Some(default) = self.token(Token::DEFAULT)? {
            let colon = self.token(Token::COLON).required()?;
            let body = self.StatementList()?;
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
        let comm = self.SimpleStmt()?;
        let colon = self.token(Token::COLON).required()?;
        let body = self.StatementList()?;

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
    fn LabeledStmt_or_SimpleStmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::LabeledStmt_or_SimpleStmt()");

        // Try to parse as SimpleStmt first
        let stmt = self.SimpleStmt()?;

        // Check if it's a labeled statement (identifier followed by colon)
        if let Some(ast::Stmt::ExprStmt(ref expr_stmt)) = stmt {
            if let ast::Expr::Ident(ref ident) = expr_stmt.x {
                if let Some(colon) = self.token(Token::COLON)? {
                    let label = ast::Ident {
                        name_pos: ident.name_pos,
                        name: ident.name,
                        obj: None,
                    };
                    let inner_stmt = self.Statement()?;
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

    // Receiver = Parameters .
    fn Receiver(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::Receiver()");

        self.Parameters()
    }

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

    // FunctionDecl | MethodDecl
    // FunctionDecl = "func" FunctionName [ TypeParameters ] Signature [ FunctionBody ] .
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

        // Parse optional type parameters (Go 1.18+ generics)
        let type_params = self.TypeParameters()?;

        let mut type_ = self.Signature(Some(func.0)).required()?;
        type_.type_params = type_params;

        let body = self.FunctionBody()?;

        Ok(Some(ast::FuncDecl {
            doc: None,
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
    // NOTE: This function will NOT consume tokens if it determines this is not type parameters.
    // It distinguishes between:
    // - [T any]      -> type parameters
    // - []int        -> slice type (not type parameters)
    // - [5]int       -> array type (not type parameters)
    fn TypeParameters(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::TypeParameters()");

        // Must start with [
        if self.current_step.1 != Token::LBRACK {
            return Ok(None);
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
            // This is [], put tokens back conceptually by returning special result
            // Since we already consumed [, we need to handle this case specially
            // Actually, we can't easily "unread" tokens. The caller needs to handle this.
            // Let's return an empty list and have caller detect this case.
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(Some(ast::FieldList {
                opening: Some(lbrack.0),
                list: vec![], // Empty signals "not really type params"
                closing: Some(rbrack.0),
            }));
        }

        // If not followed by identifier, this is not type parameters (could be array type [5]int)
        // Return None with a special marker that we've consumed [
        if self.current_step.1 != Token::IDENT {
            // This is [expr] for array type - return a special marker
            // Parse the expression and ] to get the array length
            let len = self.Expression().required()?;
            let rbrack = self.token(Token::RBRACK).required()?;
            // Return a FieldList with a special single field containing the array length expression
            // The caller will need to detect this and construct an ArrayType
            return Ok(Some(ast::FieldList {
                opening: Some(lbrack.0),
                // Store the len expression in a Field's type_ field as a marker
                // This is a bit hacky but avoids changing the function signature
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(len),
                    tag: None,
                    comment: None,
                }],
                closing: Some(rbrack.0),
            }));
        }

        // We have [ident... - need to distinguish between:
        // - [ident] or [pkg.ident] for array type (ident is length expression)
        // - [ident constraint] for type parameters
        // Parse the identifier first
        let first_ident = self.identifier().required()?;

        // If followed by period, it's a qualified identifier like pkg.Const
        // This is an array type [pkg.Const]Type
        if self.current_step.1 == Token::PERIOD {
            self.token(Token::PERIOD)?;
            let sel = self.identifier().required()?;
            let len_expr = ast::Expr::SelectorExpr(ast::SelectorExpr {
                x: Box::new(ast::Expr::Ident(first_ident)),
                sel,
            });
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(Some(ast::FieldList {
                opening: Some(lbrack.0),
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(len_expr),
                    tag: None,
                    comment: None,
                }],
                closing: Some(rbrack.0),
            }));
        }

        // If followed by ], this is [ident] for array type
        if self.current_step.1 == Token::RBRACK {
            let rbrack = self.token(Token::RBRACK).required()?;
            return Ok(Some(ast::FieldList {
                opening: Some(lbrack.0),
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(ast::Expr::Ident(first_ident)),
                    tag: None,
                    comment: None,
                }],
                closing: Some(rbrack.0),
            }));
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
                let right = self.UnaryExpr().required()?;
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
                return Ok(Some(ast::FieldList {
                    opening: Some(lbrack.0),
                    list: vec![ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(len_expr),
                        tag: None,
                        comment: None,
                    }],
                    closing: Some(rbrack.0),
                }));
            }

            // Otherwise, this is a pointer type constraint
            // Parse the type that * points to
            let pointed_type = self.Type().required()?;
            let pointer_constraint = ast::Expr::StarExpr(ast::StarExpr {
                star: star_pos,
                x: Box::new(pointed_type),
            });

            // Handle union types: [T *S | *R]
            let mut constraint = pointer_constraint;
            while let Some(or_tok) = self.token(Token::OR)? {
                let next_term = self.TypeTerm().required()?;
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
                fields.push(self.TypeParamDecl().required()?);
            }

            let rbrack = self.token(Token::RBRACK).required()?;

            return Ok(Some(ast::FieldList {
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
            return Ok(Some(ast::FieldList {
                opening: Some(lbrack.0),
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    type_: Some(len_expr),
                    tag: None,
                    comment: None,
                }],
                closing: Some(rbrack.0),
            }));
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
        let constraint = match self.TypeConstraint()? {
            Some(c) => c,
            None => {
                // No constraint found - this might be an array type [T] where T is a type
                // But we already handled [ident] above, so this is an error or
                // partial type parameter. For now, treat single ident without constraint
                // as type parameter with inferred 'any' constraint (Go 1.18 behavior)
                ast::Expr::Ident(ast::Ident {
                    name_pos: names[0].name_pos,
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
            fields.push(self.TypeParamDecl().required()?);

            // Parse additional type parameter declarations
            while self.token(Token::COMMA)?.is_some() {
                // Allow trailing comma
                if self.current_step.1 == Token::RBRACK {
                    break;
                }
                fields.push(self.TypeParamDecl().required()?);
            }
        }

        let rbrack = self.token(Token::RBRACK).required()?;

        Ok(Some(ast::FieldList {
            opening: Some(lbrack.0),
            list: fields,
            closing: Some(rbrack.0),
        }))
    }

    // TypeParamDecl = IdentifierList TypeConstraint .
    // TypeConstraint = TypeElem .
    fn TypeParamDecl(&mut self) -> Result<Option<ast::Field<'scanner>>> {
        log::debug!("Parser::TypeParamDecl()");

        let (names, _, _) = match self.IdentifierList()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Parse the constraint (which can be a union type)
        let constraint = self.TypeConstraint().required()?;

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
    fn TypeConstraint(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeConstraint()");

        // Parse the first type term
        let first = match self.TypeTerm()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Check for union types (Type1 | Type2 | ...)
        let mut type_elem = first;
        while let Some(or_tok) = self.token(Token::OR)? {
            let next_term = self.TypeTerm().required()?;
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
    fn TypeTerm(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeTerm()");

        // Check for underlying type constraint (~Type)
        if let Some(tilde) = self.token(Token::TILDE)? {
            let type_ = self.TypeWithInstantiation().required()?;
            return Ok(Some(ast::Expr::UnaryExpr(ast::UnaryExpr {
                op_pos: tilde.0,
                op: Token::TILDE,
                x: Box::new(type_),
            })));
        }

        self.TypeWithInstantiation()
    }

    // TypeWithInstantiation = Type [ TypeArgs ] .
    // TypeArgs = "[" TypeList [ "," ] "]" .
    // This handles generic type instantiation like Comparable[T] or _SliceOf[E]
    fn TypeWithInstantiation(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::TypeWithInstantiation()");

        let type_ = match self.Type()? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Check for type instantiation [T] or [T1, T2]
        if self.current_step.1 == Token::LBRACK {
            let lbrack = self.token(Token::LBRACK).required()?;
            let mut indices = vec![self.Type().required()?];
            while self.token(Token::COMMA)?.is_some() {
                if self.current_step.1 == Token::RBRACK {
                    break;
                }
                indices.push(self.Type().required()?);
            }
            let rbrack = self.token(Token::RBRACK).required()?;

            if indices.len() == 1 {
                return Ok(Some(ast::Expr::IndexExpr(ast::IndexExpr {
                    x: Box::new(type_),
                    lbrack: lbrack.0,
                    index: Box::new(indices.pop().unwrap()),
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

    // assign_op = [ add_op | mul_op ] "=" .
    // add_op    = "+" | "-" | "|" | "^" .
    // mul_op    = "*" | "/" | "%" | "<<" | ">>" | "&" | "&^" .
    fn assign_op(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
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

    // unary_op = "+" | "-" | "!" | "^" | "*" | "&" | "<-" .
    fn unary_op(&mut self) -> Result<Option<scanner::Step<'scanner>>> {
        log::debug!("Parser::unary_op()");

        use Token::*;
        Ok(match self.current_step {
            step @ (_, ADD | SUB | NOT | MUL | XOR | AND | ARROW, _) => {
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
    fn get_binary_op(&mut self, min_precedence: u8) -> Result<Option<scanner::Step<'scanner>>> {
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

    fn identifier(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
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

    fn int_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::int_lit()");

        self.token(Token::INT)?
            .map_or(Ok(None), |(value_pos, kind, value)| {
                Ok(Some(ast::BasicLit {
                    value_pos,
                    kind,
                    value,
                }))
            })
    }

    fn float_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::float_lit()");

        self.token(Token::FLOAT)?
            .map_or(Ok(None), |(value_pos, kind, value)| {
                Ok(Some(ast::BasicLit {
                    value_pos,
                    kind,
                    value,
                }))
            })
    }

    fn imaginary_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::imaginary_lit()");

        self.token(Token::IMAG)?
            .map_or(Ok(None), |(value_pos, kind, value)| {
                Ok(Some(ast::BasicLit {
                    value_pos,
                    kind,
                    value,
                }))
            })
    }

    fn rune_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::rune_lit()");

        self.token(Token::CHAR)?
            .map_or(Ok(None), |(value_pos, kind, value)| {
                Ok(Some(ast::BasicLit {
                    value_pos,
                    kind,
                    value,
                }))
            })
    }

    fn string_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::string_lit()");

        self.token(Token::STRING)?
            .map_or(Ok(None), |(value_pos, kind, value)| {
                Ok(Some(ast::BasicLit {
                    value_pos,
                    kind,
                    value,
                }))
            })
    }

    /// Returns the current step and advances to the next one, but only if it matches the expected
    /// token. [`Parser::next`] is automatically called for you.
    fn token(&mut self, expected: Token) -> Result<Option<scanner::Step<'scanner>>> {
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

    /// Advances to the next token. Skips all the comment tokens.
    fn next(&mut self) -> Result<()> {
        if let Some(step) = self
            .steps
            .find(|step| !matches!(step, Ok((_, Token::COMMENT, _))))
        {
            self.current_step = step?;
            log::debug!("self.current_step = {:?}", self.current_step);
            return Ok(());
        }
        Err(ParserError::UnexpectedEndOfFile)
    }
}

/// Check if an expression is a type switch guard: x.(type)
fn is_type_switch_guard(expr: &ast::Expr) -> bool {
    if let ast::Expr::TypeAssertExpr(type_assert) = expr {
        // Type switch guard has type_ = None (nil in Go's AST)
        return type_assert.type_.is_none();
    }
    false
}
