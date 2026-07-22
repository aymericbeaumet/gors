// Go parser implementation following the Go language specification.

mod core;
mod declarations;
mod error;
mod expressions;
mod functions;
mod import_path;
mod parameters;
mod signatures;
mod statements;
mod types;
mod version;

use crate::ast;
use crate::scanner;
use crate::token::Position;
use core::Parser;
use version::extract_go_version;

pub use error::{ParserError, Result};
pub use import_path::ImportPathIssue;
pub(crate) use import_path::decode_and_validate as decode_import_path_literal;

enum TypeParameterParse<'scanner> {
    None,
    TypeParameters(ast::FieldList<'scanner>),
    ConsumedSlice {
        lbrack: Position<'scanner>,
        rbrack: Position<'scanner>,
    },
    ConsumedArray {
        lbrack: Position<'scanner>,
        rbrack: Position<'scanner>,
        len: ast::Expr<'scanner>,
    },
}

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
                file: parser.current_step.0.filename().into_owned(),
                offset: parser.current_step.0.offset,
                line: parser.current_step.0.line,
                column: parser.current_step.0.column,
                token: parser.current_step.1,
                literal: parser.current_step.2.to_owned(),
            },
            err => err,
        })
}
