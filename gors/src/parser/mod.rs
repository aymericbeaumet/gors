// Go parser implementation following the Go language specification.

mod core;
mod declarations;
mod error;
mod expressions;
mod functions;
mod import_path;
mod output;
mod parameters;
mod signatures;
mod statements;
#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests;
mod types;
mod version;

use crate::ast;
use crate::scanner;
use crate::source::{SourceCoordinateMap, TextSize};
use crate::token::Position;
use core::Parser;
use error::{RawParserError, Result};
use version::extract_go_version;

pub use error::{ParserError, ParserErrorKind};
pub use import_path::ImportPathIssue;
pub(crate) use import_path::decode_and_validate as decode_import_path_literal;
pub use output::ParsedFile;

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
        self.and_then(|node| node.ok_or(RawParserError::UnexpectedToken))
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
/// Returns the ephemeral AST together with the scanner-built source coordinate
/// map. Every failure has an exact physical byte anchor, a typed Go-adjusted
/// display coordinate, and the map for the consumed source prefix.
///
/// # Example
///
/// ```
/// use gors::parser::parse_file;
///
/// let source = "package main\n\nfunc main() {}";
/// let parsed = parse_file("example.go", source).unwrap();
/// assert_eq!(parsed.ast().name.name, "main");
/// assert_eq!(parsed.source_coordinate_map().initial_filename(), "example.go");
/// ```
pub fn parse_file<'a>(
    filename: &'a str,
    buffer: &'a str,
) -> std::result::Result<ParsedFile<'a>, ParserError> {
    if let Err(error) = TextSize::try_from(buffer.len()) {
        return Err(ParserError::source_too_large(
            filename,
            error,
            SourceCoordinateMap::empty(filename),
        ));
    }

    // Extract go version from //go:build directive before parsing
    let go_version = extract_go_version(buffer);

    let scanner = scanner::Scanner::new(filename, buffer);
    let mut parser = Parser::new(scanner, go_version, buffer, filename);
    let parsed = parser
        .next()
        .and_then(|()| parser.parse_source_file().required());
    let current = parser.current_step;
    let coordinate_map = match parser.steps.source_coordinate_map() {
        Ok(coordinate_map) => coordinate_map,
        Err(error) => {
            let physical_offset = match TextSize::try_from(current.0.offset) {
                Ok(offset) => offset,
                Err(size_error) => {
                    return Err(ParserError::source_too_large(
                        filename,
                        size_error,
                        SourceCoordinateMap::empty(filename),
                    ));
                }
            };
            return Err(ParserError::coordinate_failure(
                error,
                physical_offset,
                SourceCoordinateMap::empty(filename),
            ));
        }
    };

    match parsed {
        Ok(ast) => Ok(ParsedFile::new(ast, coordinate_map)),
        Err(error) => Err(locate_error(error, current, coordinate_map)),
    }
}

fn locate_error(
    error: RawParserError,
    current: scanner::Step<'_>,
    coordinate_map: SourceCoordinateMap,
) -> ParserError {
    let (kind, offset) = match error {
        RawParserError::Scanner(error) => (ParserErrorKind::Scanner(error.kind), error.offset),
        RawParserError::UnexpectedEndOfFile { offset } => {
            (ParserErrorKind::UnexpectedEndOfFile, offset)
        }
        RawParserError::UnexpectedToken if current.1 == crate::token::Token::EOF => {
            (ParserErrorKind::UnexpectedEndOfFile, current.0.offset)
        }
        RawParserError::UnexpectedToken => (
            ParserErrorKind::UnexpectedToken {
                token: current.1,
                literal: current.2.into(),
            },
            current.0.offset,
        ),
    };
    match TextSize::try_from(offset) {
        Ok(physical_offset) => ParserError::located(kind, physical_offset, coordinate_map),
        Err(error) => {
            let filename = coordinate_map.initial_filename().to_owned();
            ParserError::source_too_large(&filename, error, coordinate_map)
        }
    }
}
