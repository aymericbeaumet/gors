#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use super::{Scanner, Token};
use crate::parser::{ParserError, parse_file};
use crate::token::{Position, SourceOrigin};

fn position_of_ident<'a>(filename: &'a str, source: &'a str, name: &str) -> Position<'a> {
    Scanner::new(filename, source)
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|(position, token, literal)| {
            (token == Token::IDENT && literal == name).then_some(position)
        })
        .unwrap()
}

#[test]
fn initial_source_names_are_preserved_exactly() {
    for filename in [
        "main.go",
        "/workspace/pkg/main.go",
        r"C:\workspace\pkg\main.go",
        "mem://workspace/pkg/main.go",
    ] {
        let position = position_of_ident(filename, "package sample\n", "sample");
        assert_eq!(position.origin, SourceOrigin::Initial(filename));
        assert_eq!(position.filename(), filename);
        assert_eq!(position.to_string(), format!("{filename}:1:9"));

        let json = serde_json::to_value(position).unwrap();
        assert_eq!(json.get("Filename").unwrap(), filename);
    }
}

#[test]
fn relative_line_names_use_the_initial_source_separator() {
    for (filename, expected) in [
        ("main.go", "generated.go"),
        ("/workspace/pkg/main.go", "/workspace/pkg/generated.go"),
        (
            r"C:\workspace\pkg\main.go",
            r"C:\workspace\pkg\generated.go",
        ),
        (
            "mem://workspace/pkg/main.go",
            "mem://workspace/pkg/generated.go",
        ),
    ] {
        let position =
            position_of_ident(filename, "//line generated.go:40\ngenerated\n", "generated");
        assert!(position.origin.is_line_directive());
        assert_eq!(position.filename(), expected);
        assert_eq!(position.line, 40);
        assert_eq!(position.column, 0);
    }
}

#[test]
fn rooted_line_names_are_never_prefixed_by_the_initial_directory() {
    for (directive_name, expected) in [
        ("/virtual/generated.go", "/virtual/generated.go"),
        (r"C:\virtual\generated.go", r"C:\virtual\generated.go"),
        ("mem://generated/pkg/main.go", "mem://generated/pkg/main.go"),
    ] {
        let source = format!("//line {directive_name}:40\ngenerated\n");
        let position = position_of_ident("/workspace/pkg/main.go", &source, "generated");
        assert!(position.origin.is_line_directive());
        assert_eq!(position.filename(), expected);
    }
}

#[test]
fn filename_free_line_directive_retains_the_active_origin() {
    let source = "//line first.go:10\nfirst\n//line :20:7\nsecond\n";
    let first = position_of_ident("/workspace/main.go", source, "first");
    let second = position_of_ident("/workspace/main.go", source, "second");

    assert_eq!(first.filename(), "/workspace/first.go");
    assert_eq!(second.filename(), first.filename());
    assert_eq!(second.origin, first.origin);
    assert_eq!(second.line, 20);
    assert_eq!(second.column, 7);
}

#[test]
fn parser_errors_report_exact_initial_and_virtual_names() {
    let error = parse_file("main.go", "package )\n").unwrap_err();
    assert_eq!(error.location().unwrap().0, "main.go");

    let error = parse_file(
        r"C:\workspace\main.go",
        "package sample\n//line generated.go:40\nfunc )\n",
    )
    .unwrap_err();
    let (file, line, _) = error.location().unwrap();
    assert_eq!(file, r"C:\workspace\generated.go");
    assert_eq!(line, 40);
}

#[test]
fn scanner_errors_report_the_active_virtual_name() {
    let error = Scanner::new("mem://workspace/main.go", "//line generated.go:40\n@\n")
        .into_iter()
        .find_map(Result::err)
        .unwrap();

    assert_eq!(error.file, "mem://workspace/generated.go");
    assert_eq!(error.line, 40);
    assert!(matches!(
        ParserError::from(error).location(),
        Some((file, 40, 1)) if file == "mem://workspace/generated.go"
    ));
}

#[test] // fuzz
fn it_should_return_an_error_on_missing_line_number() {
    let input = "/*line :*/";
    let mut out: Vec<_> = Scanner::new(file!(), input).into_iter().collect();
    assert!(out.pop().unwrap().is_err());
}

#[test]
fn it_should_insert_semicolon_after_multiline_comment_with_newlines() {
    // When an identifier is followed by a multi-line comment containing newlines,
    // and then a non-newline token, a semicolon should be inserted after the comment
    // with position at the first newline inside the comment.
    let input = "x /* comment\n */y";
    let tokens: Vec<_> = Scanner::new("test.go", input)
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|(pos, tok, lit)| (pos.line, pos.column, tok, lit))
        .collect();

    // Expected: IDENT "x", COMMENT, SEMICOLON (at line 1, column 13), IDENT "y", SEMICOLON, EOF
    assert_eq!(tokens.len(), 6);
    assert_eq!(*tokens.first().unwrap(), (1, 1, Token::IDENT, "x"));
    assert_eq!(tokens.get(1).unwrap().2, Token::COMMENT);
    assert_eq!(*tokens.get(2).unwrap(), (1, 13, Token::SEMICOLON, "\n")); // Position at first newline
    assert_eq!(tokens.get(3).unwrap().2, Token::IDENT);
    assert_eq!(tokens.get(4).unwrap().2, Token::SEMICOLON); // Semicolon after y at EOF
    assert_eq!(tokens.get(5).unwrap().2, Token::EOF);
}

#[test]
fn it_should_insert_semicolon_after_multiline_comment_followed_by_rparen() {
    // Test case from issue14520.go: identifier followed by multi-line comment, then )
    let input = "x /* comment\n\n*/)";
    let tokens: Vec<_> = Scanner::new("test.go", input)
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|(pos, tok, lit)| (pos.line, pos.column, tok, lit))
        .collect();

    // Expected: IDENT "x", COMMENT, SEMICOLON (at line 1), RPAREN, SEMICOLON, EOF
    assert_eq!(tokens.len(), 6);
    assert_eq!(*tokens.first().unwrap(), (1, 1, Token::IDENT, "x"));
    assert_eq!(tokens.get(1).unwrap().2, Token::COMMENT);
    assert_eq!(tokens.get(2).unwrap().0, 1); // Semicolon at line 1
    assert_eq!(tokens.get(2).unwrap().2, Token::SEMICOLON);
    assert_eq!(tokens.get(3).unwrap().2, Token::RPAREN);
}

#[test]
fn it_should_insert_semicolon_after_multiline_comment_without_internal_newlines() {
    // When a multi-line comment has no internal newlines but is followed by a newline,
    // semicolon should be inserted with normal position (after the comment).
    let input = "x /* comment */\ny";
    let tokens: Vec<_> = Scanner::new("test.go", input)
        .into_iter()
        .filter_map(|r| r.ok())
        .map(|(pos, tok, lit)| (pos.line, pos.column, tok, lit))
        .collect();

    // Expected: IDENT "x", COMMENT, SEMICOLON, IDENT "y", SEMICOLON, EOF
    assert_eq!(tokens.len(), 6);
    assert_eq!(*tokens.first().unwrap(), (1, 1, Token::IDENT, "x"));
    assert_eq!(tokens.get(1).unwrap().2, Token::COMMENT);
    assert_eq!(tokens.get(2).unwrap().2, Token::SEMICOLON);
    assert_eq!(tokens.get(3).unwrap().2, Token::IDENT);
    assert_eq!(tokens.get(4).unwrap().2, Token::SEMICOLON); // Semicolon after y at EOF
    assert_eq!(tokens.get(5).unwrap().2, Token::EOF);
}
