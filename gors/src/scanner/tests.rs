#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use super::{Scanner, Token};

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
