// Go parser implementation following the Go language specification

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
            Self::UnexpectedTokenAt {
                file, line, column, ..
            } => Some((file.clone(), *line, *column)),
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
            Self::UnexpectedTokenAt {
                file,
                line,
                column,
                token,
                literal,
            } => {
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
                    write!(
                        f,
                        "{}: syntax error: unexpected {} '{}'",
                        loc, token_str, literal
                    )
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

/// Parse a Go source file into an Abstract Syntax Tree.
///
/// This is the main entry point for parsing Go source code. It performs
/// lexical analysis and parsing to produce a complete AST.
///
/// # Arguments
///
/// * `filename` - The name of the source file (used in error messages)
/// * `buffer` - The Go source code to parse
///
/// # Returns
///
/// Returns `Ok(ast::File)` on successful parsing, or `Err(ParserError)`
/// if the source contains syntax errors.
///
/// # Example
///
/// ```
/// use gors::parser::parse_file;
///
/// let source = "package main\n\nfunc main() {}";
/// let ast = parse_file("example.go", source).unwrap();
/// assert_eq!(ast.name.name, "main");
/// ```
pub fn parse_file<'a>(filename: &'a str, buffer: &'a str) -> Result<ast::File<'a>> {
    // Extract go version from //go:build directive before parsing
    let go_version = extract_go_version(buffer);

    let scanner = scanner::Scanner::new(filename, buffer);
    let mut parser = Parser::new(scanner, go_version, buffer, filename);
    parser.next()?;
    parser
        .parse_source_file()
        .required()
        .map_err(|err| match err {
            ParserError::UnexpectedToken => ParserError::UnexpectedTokenAt {
                file: format!(
                    "{}/{}",
                    parser.current_step.0.directory, parser.current_step.0.file
                ),
                line: parser.current_step.0.line,
                column: parser.current_step.0.column,
                token: parser.current_step.1,
                literal: parser.current_step.2.to_owned(),
            },
            err => err,
        })
}

/// Parse a Go source path (file or directory) into an Abstract Syntax Tree.
///
/// This function handles both individual Go files and directories:
/// - For a file path: parses that single file
/// - For a directory path: parses all `.go` files in the directory (excluding `_test.go` files)
///   and merges them into a single AST
///
/// This matches the behavior of `go run` and `go build`.
///
/// # Arguments
///
/// * `path` - Path to a Go source file or directory containing Go files
///
/// # Returns
///
/// Returns `Ok(ast::File)` on successful parsing, or `Err(ParserError)`
/// if the source contains syntax errors or no Go files are found.
///
/// # Example
///
/// ```no_run
/// use gors::parser::parse_path;
///
/// // Parse a single file
/// let ast = parse_path("main.go").unwrap();
///
/// // Parse all Go files in a directory
/// let ast = parse_path("./mypackage/").unwrap();
/// ```
pub fn parse_path(
    path: &str,
) -> std::result::Result<(ast::File<'static>, Vec<(String, String)>), PathParseError> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| PathParseError::IoError(format!("cannot access '{}': {}", path, e)))?;

    if metadata.is_file() {
        let buffer = std::fs::read_to_string(path)
            .map_err(|e| PathParseError::IoError(format!("cannot read '{}': {}", path, e)))?;

        // We need to leak the strings to get 'static lifetime
        let path_static: &'static str = Box::leak(path.to_string().into_boxed_str());
        let buffer_static: &'static str = Box::leak(buffer.clone().into_boxed_str());

        let ast = parse_file(path_static, buffer_static).map_err(PathParseError::ParserError)?;

        Ok((ast, vec![(path.to_string(), buffer)]))
    } else if metadata.is_dir() {
        parse_dir(path)
    } else {
        Err(PathParseError::IoError(format!(
            "'{}' is not a file or directory",
            path
        )))
    }
}

/// Error type for path parsing failures.
#[derive(Debug)]
pub enum PathParseError {
    /// An I/O error occurred (file not found, permission denied, etc.)
    IoError(String),
    /// A parser error occurred while parsing a Go file
    ParserError(ParserError),
    /// No Go files found in the directory
    NoGoFiles(String),
    /// Package name mismatch between files
    PackageMismatch {
        expected: String,
        found: String,
        file: String,
    },
}

impl std::fmt::Display for PathParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "{}", msg),
            Self::ParserError(e) => write!(f, "{}", e),
            Self::NoGoFiles(dir) => write!(f, "no Go files found in '{}'", dir),
            Self::PackageMismatch {
                expected,
                found,
                file,
            } => {
                write!(
                    f,
                    "found packages {} ({}) and {} in same directory",
                    found, file, expected
                )
            }
        }
    }
}

impl std::error::Error for PathParseError {}

/// Parse all Go files in a directory into a single merged AST.
///
/// This function reads all `.go` files in the specified directory (excluding
/// `_test.go` files and files starting with `.` or `_`), parses them, and
/// merges their declarations into a single AST.
///
/// All files must declare the same package name.
///
/// # Arguments
///
/// * `dir_path` - Path to a directory containing Go source files
///
/// # Returns
///
/// Returns a tuple of the merged AST and the list of (filename, content) pairs
/// for all parsed files.
fn parse_dir(
    dir_path: &str,
) -> std::result::Result<(ast::File<'static>, Vec<(String, String)>), PathParseError> {
    let entries = std::fs::read_dir(dir_path).map_err(|e| {
        PathParseError::IoError(format!("cannot read directory '{}': {}", dir_path, e))
    })?;

    // Collect all .go files (excluding _test.go and dotfiles)
    let mut go_files: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;

            // Skip hidden files, underscore-prefixed files, and test files
            if file_name.starts_with('.') || file_name.starts_with('_') {
                return None;
            }

            // Only include .go files, excluding _test.go
            if file_name.ends_with(".go") && !file_name.ends_with("_test.go") {
                Some(path.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();

    if go_files.is_empty() {
        return Err(PathParseError::NoGoFiles(dir_path.to_string()));
    }

    // Sort for deterministic ordering
    go_files.sort();

    // Parse all files and collect their ASTs
    let mut files_content: Vec<(String, String)> = Vec::new();
    let mut asts: Vec<ast::File<'static>> = Vec::new();

    for file_path in &go_files {
        let buffer = std::fs::read_to_string(file_path)
            .map_err(|e| PathParseError::IoError(format!("cannot read '{}': {}", file_path, e)))?;

        // Leak strings to get 'static lifetime
        let path_static: &'static str = Box::leak(file_path.clone().into_boxed_str());
        let buffer_static: &'static str = Box::leak(buffer.clone().into_boxed_str());

        let ast = parse_file(path_static, buffer_static).map_err(PathParseError::ParserError)?;

        files_content.push((file_path.clone(), buffer));
        asts.push(ast);
    }

    // Verify all files have the same package name
    let expected_package = asts[0].name.name;
    for (i, ast) in asts.iter().enumerate().skip(1) {
        if ast.name.name != expected_package {
            return Err(PathParseError::PackageMismatch {
                expected: expected_package.to_string(),
                found: ast.name.name.to_string(),
                file: go_files[i].clone(),
            });
        }
    }

    // Merge all ASTs into one
    let merged = merge_files(asts);

    Ok((merged, files_content))
}

/// Merge multiple Go AST files into a single file.
///
/// This combines all declarations from the input files into a single AST,
/// using the package information from the first file.
fn merge_files(mut files: Vec<ast::File<'static>>) -> ast::File<'static> {
    if files.len() == 1 {
        return files.remove(0);
    }

    let mut base = files.remove(0);

    for file in files {
        // Merge declarations
        base.decls.extend(file.decls);

        // Merge unresolved identifiers
        base.unresolved.extend(file.unresolved);

        // Merge comments
        base.comments.extend(file.comments);

        // Update file_end to the last file's end
        base.file_end = file.file_end;
    }

    base
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

fn is_go_version(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix("go") {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
    } else {
        false
    }
}

fn compare_go_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts: Vec<u32> = a
        .strip_prefix("go")
        .unwrap_or("")
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    let b_parts: Vec<u32> = b
        .strip_prefix("go")
        .unwrap_or("")
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    a_parts.cmp(&b_parts)
}

/// Extract effective go version from a build constraint expression.
/// Handles AND (&&), OR (||), NOT (!), and parenthesized groups.
fn find_go_version_in_constraint(constraint: &str) -> Option<&str> {
    // Split into top-level OR branches (respecting parentheses)
    let branches = split_top_level(constraint, b'|');

    let mut result: Option<&str> = None;

    for branch in &branches {
        let branch = branch.trim();
        if branch.is_empty() {
            continue;
        }

        match find_version_in_and_branch(branch) {
            None => return None,
            Some(v) => {
                result = Some(match result {
                    None => v,
                    Some(prev) => {
                        if compare_go_versions(v, prev) == std::cmp::Ordering::Less {
                            v
                        } else {
                            prev
                        }
                    }
                });
            }
        }
    }

    result
}

/// Split a constraint string by a top-level operator (|| or &&).
fn split_top_level(s: &str, op: u8) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            c if c == op && depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == op => {
                parts.push(&s[start..i]);
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// Find the effective go version in an AND branch (no top-level ||).
/// Returns None if the branch has no go version requirement.
fn find_version_in_and_branch(branch: &str) -> Option<&str> {
    let terms = split_top_level(branch, b'&');
    let mut max_version: Option<&str> = None;

    for term in &terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }

        // Negated term — skip any go version inside
        if term.starts_with('!') {
            continue;
        }

        // Parenthesized group — recurse
        if term.starts_with('(') && term.ends_with(')') {
            if let Some(v) = find_go_version_in_constraint(&term[1..term.len() - 1]) {
                max_version = Some(match max_version {
                    None => v,
                    Some(prev) => {
                        if compare_go_versions(v, prev) == std::cmp::Ordering::Greater {
                            v
                        } else {
                            prev
                        }
                    }
                });
            }
            continue;
        }

        // Check if this term is a go version
        if is_go_version(term) {
            max_version = Some(match max_version {
                None => term,
                Some(prev) => {
                    if compare_go_versions(term, prev) == std::cmp::Ordering::Greater {
                        term
                    } else {
                        prev
                    }
                }
            });
        }
    }

    max_version
}

struct Parser<'scanner> {
    steps: scanner::IntoIter<'scanner>,
    current_step: scanner::Step<'scanner>,
    expr_level: isize,
    go_version: &'scanner str,
    buffer: &'scanner str,
    original_directory: &'scanner str,
    original_file: &'scanner str,
    lead_comment: Option<ast::CommentGroup<'scanner>>,
    line_comment: Option<ast::CommentGroup<'scanner>>,
    all_comments: Vec<ast::CommentGroup<'scanner>>,
}

impl<'scanner> Parser<'scanner> {
    pub fn new(
        scanner: scanner::Scanner<'scanner>,
        go_version: &'scanner str,
        buffer: &'scanner str,
        filename: &'scanner str,
    ) -> Self {
        let (directory, file) = filename.rsplit_once('/').unwrap_or(("", filename));
        Self {
            steps: scanner.into_iter(),
            current_step: (Position::default(), Token::EOF, ""),
            expr_level: 0,
            go_version,
            buffer,
            original_directory: directory,
            original_file: file,
            lead_comment: None,
            line_comment: None,
            all_comments: Vec::new(),
        }
    }

    fn comment_end_line(comment: &ast::Comment) -> usize {
        if comment.text.starts_with("//") {
            comment.slash.line
        } else {
            comment.slash.line + comment.text.matches('\n').count()
        }
    }

    fn comment_end_offset(comment: &ast::Comment) -> usize {
        comment.slash.offset + comment.text.len()
    }

    fn newlines_between(&self, start: usize, end: usize) -> usize {
        self.buffer[start..end]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
    }

    fn consume_comment_group(
        comments: &[ast::Comment<'scanner>],
        n: usize,
    ) -> (ast::CommentGroup<'scanner>, usize, usize) {
        let mut list = vec![comments[0].clone()];
        let mut endline = Self::comment_end_line(&comments[0]);
        let mut consumed = 1;

        for comment in &comments[1..] {
            if comment.slash.line > endline + n {
                break;
            }
            endline = Self::comment_end_line(comment);
            list.push(comment.clone());
            consumed += 1;
        }

        (ast::CommentGroup { list }, endline, consumed)
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
    fn parse_source_file(&mut self) -> Result<Option<ast::File<'scanner>>> {
        log::debug!("Parser::parse_source_file()");

        let doc = self.lead_comment.take();

        let (package, package_name) = match self.parse_package_clause()? {
            Some(v) => v,
            None => return Ok(None),
        };

        self.token(Token::SEMICOLON).required()?;

        let file_start = Position {
            directory: self.original_directory,
            file: self.original_file,
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

        while let Some(import_decl) = self.parse_import_decl()? {
            self.token(Token::SEMICOLON).required()?;
            out.decls.push(ast::Decl::GenDecl(import_decl));
        }

        while let Some(top_level_decl) = self.parse_top_level_decl()? {
            self.token(Token::SEMICOLON).required()?;
            out.decls.push(top_level_decl);
        }

        let eof = self.token(Token::EOF).required()?;
        out.file_end = eof.0;

        out.comments = std::mem::take(&mut self.all_comments);

        Ok(Some(out))
    }

    // PackageClause = "package" PackageName .
    fn parse_package_clause(
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
    fn parse_package_name(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::parse_package_name()");

        self.identifier()
    }

    // ImportDecl = "import" ( ImportSpec | "(" { ImportSpec ";" } ")" ) .
    fn parse_import_decl(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
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
                    import_spec.comment = self.line_comment.take();
                    specs.push(ast::Spec::ImportSpec(import_spec));
                    if self.token(Token::SEMICOLON)?.is_none() {
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
    fn parse_import_spec(&mut self) -> Result<Option<ast::ImportSpec<'scanner>>> {
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
    fn parse_import_path(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::parse_import_path()");

        self.string_lit()
    }

    // TopLevelDecl = Declaration | FunctionDecl | MethodDecl .
    fn parse_top_level_decl(&mut self) -> Result<Option<ast::Decl<'scanner>>> {
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
    fn parse_declaration(&mut self) -> Result<Option<ast::GenDecl<'scanner>>> {
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
    fn parse_type_decl_with_doc(
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
                    type_spec.comment = self.line_comment.take();
                    specs.push(ast::Spec::TypeSpec(type_spec));
                    if self.token(Token::SEMICOLON)?.is_none() {
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
    fn parse_type_spec(&mut self) -> Result<Option<ast::TypeSpec<'scanner>>> {
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
            let result = self.parse_type_parameters()?;
            // If we got an empty list, this was [] for a slice type
            // TypeParameters already consumed [] so we need to account for that when parsing type
            match result {
                Some(field_list) if field_list.list.is_empty() => {
                    // This was [] - it's a slice type, not type params
                    // We need to construct the slice type here since [ ] was consumed
                    let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
                    let element_type = self.parse_type().required()?;
                    // opening should always be set when we have a field_list, use default position as fallback
                    let lbrack = field_list.opening.unwrap_or_default();
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
                Some(mut field_list)
                    if field_list.list.len() == 1 && field_list.list[0].names.is_none() =>
                {
                    // This was [expr] - it's an array type, not type params
                    // TypeParameters stored the length expression in the type_ field
                    let assign = self.token(Token::ASSIGN)?.map(|(pos, _, _)| pos);
                    let element_type = self.parse_type().required()?;
                    let len_expr = field_list.list.pop().and_then(|f| f.type_);
                    // opening should always be set when we have a field_list, use default position as fallback
                    let lbrack = field_list.opening.unwrap_or_default();
                    return Ok(Some(ast::TypeSpec {
                        doc: None,
                        name: Some(name),
                        type_params: None,
                        assign,
                        type_: ast::Expr::ArrayType(ast::ArrayType {
                            lbrack,
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
    fn parse_const_decl_with_doc(
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
                    const_spec.comment = self.line_comment.take();
                    specs.push(ast::Spec::ValueSpec(const_spec));
                    if self.token(Token::SEMICOLON)?.is_none() {
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
    fn parse_const_spec(&mut self) -> Result<Option<ast::ValueSpec<'scanner>>> {
        log::debug!("Parser::parse_const_spec()");

        let (names, _, _) = match self.parse_identifier_list()? {
            Some(v) => v,
            None => return Ok(None),
        };

        let (type_, values) = if self.token(Token::ASSIGN)?.is_some() {
            (None, Some(self.parse_expression_list().required()?))
        } else if let Some(type_) = self.parse_type()? {
            self.token(Token::ASSIGN).required()?;
            (Some(type_), Some(self.parse_expression_list().required()?))
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
    fn parse_var_decl_with_doc(
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
                    var_spec.comment = self.line_comment.take();
                    specs.push(ast::Spec::ValueSpec(var_spec));
                    if self.token(Token::SEMICOLON)?.is_none() {
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
    fn parse_var_spec(&mut self) -> Result<Option<ast::ValueSpec<'scanner>>> {
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
    fn parse_identifier_list(&mut self) -> Result<Option<(Vec<ast::Ident<'scanner>>, bool, bool)>> {
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

    // ExpressionList = Expression { "," Expression } .
    fn parse_expression_list(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
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
    fn parse_expression(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_expression()");

        let unary_expr = match self.parse_unary_expr()? {
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
    fn parse_unary_expr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_primary_expr(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_primary_expr()");

        let primary_expr = match self.parse_operand()? {
            Some(v) => v,
            None => return Ok(None),
        };

        Ok(Some(self.continue_primary_expr(primary_expr)?))
    }

    fn continue_primary_expr(
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
    fn parse_literal_value(
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
    fn parse_selector_or_type_assertion(
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
    fn parse_index_or_slice(
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
    fn parse_arguments(&mut self, x: ast::Expr<'scanner>) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_operand(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
                return Err(ParserError::UnexpectedToken);
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
    fn parse_literal_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_keyed_element(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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

    // FunctionLit = "func" Signature FunctionBody .
    fn parse_function_lit(&mut self) -> Result<Option<ast::FuncLit<'scanner>>> {
        log::debug!("Parser::parse_function_lit()");

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let type_ = self.parse_signature(Some(func.0)).required()?;
        let body = self.parse_function_body().required()?;

        Ok(Some(ast::FuncLit { type_, body }))
    }

    // BasicLit = int_lit | float_lit | imaginary_lit | rune_lit | string_lit .
    fn parse_basic_lit(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
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

    // Type = TypeName | TypeLit | "(" Type ")" .
    fn parse_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_type_list(&mut self) -> Result<Option<Vec<ast::Expr<'scanner>>>> {
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
    fn parse_type_name(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_type_lit(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_array_type_or_slice_type<const ELLIPSIS: bool>(
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

        let len = if ELLIPSIS {
            if let Some(ellipsis) = self.token(Token::ELLIPSIS)? {
                Some(ast::Expr::Ellipsis(ast::Ellipsis {
                    ellipsis: ellipsis.0,
                    elt: None,
                }))
            } else {
                self.parse_expression()?
            }
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
    fn parse_map_type(&mut self) -> Result<Option<ast::MapType<'scanner>>> {
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
    fn parse_key_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_key_type()");

        self.parse_type()
    }

    // ChannelType = ( "chan" | "chan" "<-" | "<-" "chan" ) ElementType .
    fn parse_channel_type(&mut self) -> Result<Option<ast::ChanType<'scanner>>> {
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
    fn parse_function_type(&mut self) -> Result<Option<ast::FuncType<'scanner>>> {
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
    fn parse_element_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_element_type()");

        self.parse_type()
    }

    // PointerType = "*" BaseType .
    fn parse_pointer_type(&mut self) -> Result<Option<ast::StarExpr<'scanner>>> {
        log::debug!("Parser::parse_pointer_type()");

        let star = match self.token(Token::MUL)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let x = Box::new(self.parse_base_type().required()?);
        Ok(Some(ast::StarExpr { star: star.0, x }))
    }

    // BaseType = Type .
    fn parse_base_type(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_base_type()");

        self.parse_type()
    }

    // InterfaceType = "interface" "{" { InterfaceElem ";" } "}" .
    // InterfaceElem = MethodElem | TypeElem .
    // MethodElem    = MethodName Signature .
    // TypeElem      = TypeTerm { "|" TypeTerm } .
    // TypeTerm      = Type | UnderlyingType .
    // UnderlyingType = "~" Type .
    fn parse_interface_type(&mut self) -> Result<Option<ast::InterfaceType<'scanner>>> {
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
                    fields.push(ast::Field {
                        doc,
                        names: Some(vec![method_spec]),
                        type_: Some(ast::Expr::FuncType(signature)),
                        tag: None,
                        comment: self.line_comment.take(),
                    });
                    if self.token(Token::SEMICOLON)?.is_none() {
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
    fn parse_method_name(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
        log::debug!("Parser::parse_method_name()");

        self.identifier()
    }

    // InterfaceTypeName = TypeName .
    fn parse_interface_type_name(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
        log::debug!("Parser::parse_interface_type_name()");

        self.parse_type_name()
    }

    // StructType = "struct" "{" { FieldDecl ";" } "}" .
    fn parse_struct_type(&mut self) -> Result<Option<ast::StructType<'scanner>>> {
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
                field_decl.comment = self.line_comment.take();
                fields.push(field_decl);
                if self.token(Token::SEMICOLON)?.is_none() {
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
    fn parse_field_decl(&mut self) -> Result<Option<ast::Field<'scanner>>> {
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

            return Err(ParserError::UnexpectedToken);
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
    fn parse_tag(&mut self) -> Result<Option<ast::BasicLit<'scanner>>> {
        log::debug!("Parser::parse_tag()");

        self.string_lit()
    }

    // Signature = Parameters [ Result ] .
    fn parse_signature(
        &mut self,
        func: Option<Position<'scanner>>,
    ) -> Result<Option<ast::FuncType<'scanner>>> {
        log::debug!("Parser::parse_signature()");

        let params = match self.parse_parameters()? {
            Some(v) => v,
            None => return Ok(None),
        };
        let results = self.parse_result()?;

        Ok(Some(ast::FuncType {
            func,
            type_params: None,
            params,
            results,
        }))
    }

    // Result = Parameters | Type .
    fn parse_result(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_result()");

        if let Some(parameters) = self.parse_parameters()? {
            Ok(Some(parameters))
        } else if let Some(type_) = self.parse_type()? {
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
    fn parse_parameters(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_parameters()");

        let lparen = match self.token(Token::LPAREN)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let list = self
            .parse_parameter_list()?
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
    fn parse_parameter_list(&mut self) -> Result<Option<Vec<ast::Field<'scanner>>>> {
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
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
                while self.token(Token::COMMA)?.is_some() {
                    // Handle trailing comma
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

    // FunctionBody = Block .
    fn parse_function_body(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::parse_function_body()");

        self.parse_block()
    }

    // Block         = "{" StatementList "}" .
    // StatementList = { Statement ";" } .
    fn parse_block(&mut self) -> Result<Option<ast::BlockStmt<'scanner>>> {
        log::debug!("Parser::parse_block()");

        let lbrace = match self.token(Token::LBRACE)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let mut list = vec![];
        while let Some(statement) = self.parse_statement()? {
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
    fn parse_statement(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
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
    fn parse_for_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
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
                        return Err(ParserError::UnexpectedToken);
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
    fn parse_go_stmt(&mut self) -> Result<Option<ast::GoStmt<'scanner>>> {
        log::debug!("Parser::parse_go_stmt()");

        let go = match self.token(Token::GO)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let call = match self.parse_expression().required()? {
            ast::Expr::CallExpr(v) => v,
            _ => return Err(ParserError::UnexpectedToken),
        };

        Ok(Some(ast::GoStmt { go: go.0, call }))
    }

    // IfStmt = "if" [ SimpleStmt ";" ] Expression Block [ "else" ( IfStmt | Block ) ] .
    fn parse_if_stmt(&mut self) -> Result<Option<ast::IfStmt<'scanner>>> {
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
                return Err(ParserError::UnexpectedToken);
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
    fn parse_simple_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
        log::debug!("Parser::parse_simple_stmt()");

        if let Some(mut exprs) = self.parse_expression_list()? {
            // ShortVarDecl
            if exprs.iter().all(|expr| matches!(expr, ast::Expr::Ident(_))) {
                if let Some(define_op) = self.token(Token::DEFINE)? {
                    let rhs = self.parse_expression_list().required()?;
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

            return Err(ParserError::UnexpectedToken);
        }

        Ok(None)
    }

    // DeferStmt = "defer" Expression .
    fn parse_defer_stmt(&mut self) -> Result<Option<ast::DeferStmt<'scanner>>> {
        log::debug!("Parser::parse_defer_stmt()");

        let defer = match self.token(Token::DEFER)? {
            Some(v) => v,
            None => return Ok(None),
        };

        let call = match self.parse_expression().required()? {
            ast::Expr::CallExpr(v) => v,
            _ => return Err(ParserError::UnexpectedToken),
        };

        Ok(Some(ast::DeferStmt {
            defer: defer.0,
            call,
        }))
    }

    // ReturnStmt = "return" [ ExpressionList ] .
    fn parse_return_stmt(&mut self) -> Result<Option<ast::ReturnStmt<'scanner>>> {
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
    fn parse_branch_stmt(&mut self) -> Result<Option<ast::BranchStmt<'scanner>>> {
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
    fn parse_switch_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
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
    fn parse_case_clause(
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

        // In type switch, parse types; otherwise parse expressions
        let list = if is_type_switch {
            self.parse_type_list()?
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
    fn parse_statement_list(&mut self) -> Result<Vec<ast::Stmt<'scanner>>> {
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
    fn parse_select_stmt(&mut self) -> Result<Option<ast::SelectStmt<'scanner>>> {
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
    fn parse_comm_clause(&mut self) -> Result<Option<ast::CommClause<'scanner>>> {
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
    fn parse_labeled_stmt_or_simple_stmt(&mut self) -> Result<Option<ast::Stmt<'scanner>>> {
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

    // Receiver = Parameters .
    fn parse_receiver(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_receiver()");

        self.parse_parameters()
    }

    // identifier | QualifiedIdent
    // QualifiedIdent = PackageName "." identifier .
    // PackageName    = identifier .
    fn parse_identifier_or_qualified_ident(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_period_or_package_name(&mut self) -> Result<Option<ast::Ident<'scanner>>> {
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
    fn parse_function_decl_or_method_decl(&mut self) -> Result<Option<ast::FuncDecl<'scanner>>> {
        log::debug!("Parser::parse_function_decl_or_method_decl()");

        let doc = self.lead_comment.take();

        let func = match self.token(Token::FUNC)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let recv = self.parse_receiver()?;
        let name = self.identifier().required()?;

        // Parse optional type parameters (Go 1.18+ generics)
        let type_params = self.parse_type_parameters()?;

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
    // NOTE: This function will NOT consume tokens if it determines this is not type parameters.
    // It distinguishes between:
    // - [T any]      -> type parameters
    // - []int        -> slice type (not type parameters)
    // - [5]int       -> array type (not type parameters)
    fn parse_type_parameters(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_type_parameters()");

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
            let len = self.parse_expression().required()?;
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
        } else {
            // Parse the rest of the expression starting from first_ident.
            // Following Go's parser: parse full expression, then determine
            // if it's a type parameter list or array length.
            //
            // If the next token can continue a primary expression (.sel, (args), etc.)
            // or is a binary operator, parse the full expression — it's an array length.
            // Otherwise it's still just an identifier, which might be a type param name.
            let is_expr_continuation = matches!(
                self.current_step.1,
                Token::PERIOD
                    | Token::LPAREN
                    | Token::ADD
                    | Token::SUB
                    | Token::MUL
                    | Token::QUO
                    | Token::REM
                    | Token::AND
                    | Token::OR
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

                let rbrack = self.token(Token::RBRACK).required()?;
                return Ok(Some(ast::FieldList {
                    opening: Some(lbrack.0),
                    list: vec![ast::Field {
                        doc: None,
                        names: None,
                        type_: Some(x),
                        tag: None,
                        comment: None,
                    }],
                    closing: Some(rbrack.0),
                }));
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
        let constraint = match self.parse_type_constraint()? {
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

        Ok(Some(ast::FieldList {
            opening: Some(lbrack.0),
            list: fields,
            closing: Some(rbrack.0),
        }))
    }

    // TypeParamDecl = IdentifierList TypeConstraint .
    // TypeConstraint = TypeElem .
    fn parse_type_param_decl(&mut self) -> Result<Option<ast::Field<'scanner>>> {
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
    fn parse_type_constraint(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_type_term(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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
    fn parse_type_with_instantiation(&mut self) -> Result<Option<ast::Expr<'scanner>>> {
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

    fn parse_optional_type_instance(
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

    fn parse_embedded_elem(
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

    fn next(&mut self) -> Result<()> {
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
                None => return Err(ParserError::UnexpectedEndOfFile),
            }
        }

        if comments.is_empty() {
            return Ok(());
        }

        let mut i = 0;

        if comments[0].slash.line == prev.line && prev.line > 0 {
            let (group, endline, consumed) = Self::consume_comment_group(&comments[i..], 0);
            i += consumed;
            self.all_comments.push(group.clone());

            let next_on_different_line = if i < comments.len() {
                comments[i].slash.line != endline
            } else {
                self.current_step.0.line != endline
                    || self.current_step.1 == Token::SEMICOLON
                    || self.current_step.1 == Token::EOF
            };

            if next_on_different_line {
                self.line_comment = Some(group);
            }
        }

        let mut last_group: Option<ast::CommentGroup<'scanner>> = None;
        while i < comments.len() {
            let (group, _, consumed) = Self::consume_comment_group(&comments[i..], 1);
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
fn is_type_switch_guard(expr: &ast::Expr) -> bool {
    if let ast::Expr::TypeAssertExpr(type_assert) = expr {
        // Type switch guard has type_ = None (nil in Go's AST)
        return type_assert.type_.is_none();
    }
    false
}
