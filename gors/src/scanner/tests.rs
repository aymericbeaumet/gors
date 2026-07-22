#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use super::{Scanner, Token};
use crate::parser::parse_file;
use crate::source::{AdjustedSourceCoordinate, LogicalColumn, SourceCoordinateMap, TextSize};
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

fn coordinate_map(filename: &str, source: &str) -> SourceCoordinateMap {
    let mut scanner = Scanner::new(filename, source);
    loop {
        let (_, token, _) = scanner.scan().unwrap();
        if token == Token::EOF {
            break;
        }
    }
    scanner.source_coordinate_map().unwrap()
}

fn adjusted_at(map: &SourceCoordinateMap, source: &str, needle: &str) -> AdjustedSourceCoordinate {
    let offset = source.find(needle).unwrap();
    map.adjusted_coordinate(TextSize::try_from(offset).unwrap())
        .unwrap()
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
fn coordinate_map_rebases_only_relative_names_for_presentation() {
    let source = concat!(
        "//line generated.go:10\nrelative\n",
        "//line /virtual/absolute.go:20\nabsolute\n",
        "//line mem://generated/uri.go:30\nuri\n",
    );
    let map = coordinate_map("pkg/main.go", source);
    let project = |needle: &str, presentation: &str| {
        let payload = format!("\n{needle}\n");
        let offset = source.find(&payload).unwrap() + 1;
        map.adjusted_coordinate_for(TextSize::try_from(offset).unwrap(), presentation)
            .unwrap()
            .unwrap()
    };

    assert_eq!(
        project("relative", "/checkout/pkg/main.go").filename(),
        "/checkout/pkg/generated.go"
    );
    assert_eq!(
        project("relative", r"C:\checkout\pkg\main.go").filename(),
        r"C:\checkout\pkg\generated.go"
    );
    assert_eq!(
        project("absolute", "/checkout/pkg/main.go").filename(),
        "/virtual/absolute.go"
    );
    assert_eq!(
        project("uri", "/checkout/pkg/main.go").filename(),
        "mem://generated/uri.go"
    );
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
fn two_field_directives_hide_columns_and_empty_filename_clears_origin() {
    let source = "//line virtual.go:40\nfirst\nnext\n//line :7\ncleared\n";
    let map = coordinate_map("/workspace/main.go", source);

    let first = adjusted_at(&map, source, "first");
    assert_eq!(first.filename(), "/workspace/virtual.go");
    assert_eq!(first.position().line().get(), 40);
    assert_eq!(first.position().column(), LogicalColumn::Hidden);

    let next = adjusted_at(&map, source, "next");
    assert_eq!(next.position().line().get(), 41);
    assert_eq!(next.position().column(), LogicalColumn::Hidden);

    let cleared = adjusted_at(&map, source, "cleared");
    assert_eq!(cleared.filename(), "");
    assert_eq!(cleared.position().line().get(), 7);
    assert_eq!(cleared.position().column(), LogicalColumn::Hidden);

    assert_eq!(map.segments().len(), 2);
    let first_segment = map.segments().first().unwrap();
    assert_eq!(
        first_segment.physical_start(),
        TextSize::try_from(source.find("first").unwrap()).unwrap()
    );
    assert_eq!(first_segment.physical_position().line().get(), 2);
    assert_eq!(first_segment.physical_position().byte_column().get(), 1);
}

#[test]
fn explicit_columns_retain_filename_and_reset_on_later_physical_lines() {
    let source = "//line first.go:10\nfirst\n//line :3:7\nsecond\nthird\n";
    let map = coordinate_map("/workspace/main.go", source);

    let second = adjusted_at(&map, source, "second");
    assert_eq!(second.filename(), "/workspace/first.go");
    assert_eq!(second.position().line().get(), 3);
    assert_eq!(second.position().column().to_go_column(), 7);

    let third = adjusted_at(&map, source, "third");
    assert_eq!(third.filename(), "/workspace/first.go");
    assert_eq!(third.position().line().get(), 4);
    assert_eq!(third.position().column().to_go_column(), 1);
}

#[test]
fn block_directive_columns_are_relative_to_the_immediate_byte_anchor() {
    let source = "α /*line generated.go:8:20*/ β\nz";
    let map = coordinate_map("mem://workspace/main.go", source);

    let beta = adjusted_at(&map, source, "β");
    assert_eq!(beta.filename(), "mem://workspace/generated.go");
    assert_eq!(beta.position().line().get(), 8);
    assert_eq!(beta.position().column().to_go_column(), 21);

    let z = adjusted_at(&map, source, "z");
    assert_eq!(z.position().line().get(), 9);
    assert_eq!(z.position().column().to_go_column(), 1);
}

#[test]
fn coordinate_map_uses_utf8_byte_columns_and_crlf_line_starts() {
    let source = "//line generated.go:40:7\r\nαβ\r\nz";
    let map = coordinate_map(r"C:\workspace\main.go", source);

    let alpha = adjusted_at(&map, source, "α");
    assert_eq!(alpha.filename(), r"C:\workspace\generated.go");
    assert_eq!(alpha.position().line().get(), 40);
    assert_eq!(alpha.position().column().to_go_column(), 7);

    let beta_offset = source.find("β").unwrap();
    let beta = map
        .adjusted_coordinate(TextSize::try_from(beta_offset).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(beta.position().line().get(), 40);
    assert_eq!(beta.position().column().to_go_column(), 9);

    let z = adjusted_at(&map, source, "z");
    assert_eq!(z.position().line().get(), 41);
    assert_eq!(z.position().column().to_go_column(), 1);
}

#[test]
fn coordinate_map_owns_exact_windows_uri_and_rooted_names() {
    for (initial, directive, expected) in [
        (
            r"C:\workspace\pkg\main.go",
            "generated.go",
            r"C:\workspace\pkg\generated.go",
        ),
        (
            "mem://workspace/pkg/main.go",
            "generated.go",
            "mem://workspace/pkg/generated.go",
        ),
        (
            "/workspace/pkg/main.go",
            "mem://generated/main.go",
            "mem://generated/main.go",
        ),
    ] {
        let source = format!("//line {directive}:40\nvalue\n");
        let map = coordinate_map(initial, &source);
        let value = adjusted_at(&map, &source, "value");
        assert_eq!(value.filename(), expected);
        assert_eq!(
            map.segments().first().unwrap().adjusted_filename(),
            expected
        );
    }
}

#[test]
fn multiple_directives_can_move_lines_downward_and_cover_eof() {
    let source = "//line one.go:100\none\n//line two.go:2:9\ntwo\n//line eof.go:5\n";
    let map = coordinate_map("main.go", source);

    let one = adjusted_at(&map, source, "one\n");
    assert_eq!(one.position().line().get(), 100);
    let two = adjusted_at(&map, source, "two\n");
    assert_eq!(two.position().line().get(), 2);
    assert_eq!(two.position().column().to_go_column(), 9);

    let eof = map
        .adjusted_coordinate(TextSize::try_from(source.len()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(eof.filename(), "two.go");
    assert_eq!(eof.position().line().get(), 3);
    assert_eq!(eof.position().column().to_go_column(), 17);
    assert_eq!(map.segments().len(), 2);
}

#[test]
fn trailing_newline_does_not_invent_a_physical_eof_line() {
    let source = "x\n";
    let map = coordinate_map("main.go", source);
    let eof = map
        .adjusted_coordinate(TextSize::try_from(source.len()).unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(eof.filename(), "main.go");
    assert_eq!(eof.position().line().get(), 1);
    assert_eq!(eof.position().column().to_go_column(), 3);

    let eof_token = Scanner::new("main.go", source)
        .into_iter()
        .find_map(|step| {
            let (position, token, _) = step.unwrap();
            (token == Token::EOF).then_some(position)
        })
        .unwrap();
    assert_eq!(eof_token.offset, source.len());
    assert_eq!((eof_token.line, eof_token.column), (1, 3));
}

#[test]
fn directive_transitions_at_eof_are_ignored_like_go_token_file() {
    for source in ["//line eof.go:40\n", "/*line eof.go:40*/"] {
        let map = coordinate_map("main.go", source);
        let eof = map
            .adjusted_coordinate(TextSize::try_from(source.len()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(eof.filename(), "main.go");
        assert!(map.segments().is_empty());
    }
}

#[test]
fn scanner_iterator_exposes_the_in_pass_map_after_eof() {
    let source = "//line generated.go:40\nvalue\n";
    let mut tokens = Scanner::new("main.go", source).into_iter();
    while tokens.next().is_some() {}

    let map = tokens.source_coordinate_map().unwrap();
    let value = adjusted_at(&map, source, "value");
    assert_eq!(value.filename(), "generated.go");
    assert_eq!(value.position().line().get(), 40);
    assert_eq!(map.text_len(), TextSize::try_from(source.len()).unwrap());
}

#[test]
fn line_directive_numbers_follow_go_bounds_and_spacing() {
    let maximum = 1_usize << 30;
    let valid = format!("//line generated.go:{maximum}:{maximum}\nvalue");
    let map = coordinate_map("main.go", &valid);
    let value = adjusted_at(&map, &valid, "value");
    assert_eq!(value.position().line().get(), maximum as u32);
    assert_eq!(value.position().column().to_go_column(), maximum as u32);

    for invalid in [
        "//line generated.go:0\n",
        "//line generated.go:1073741825\n",
        "//line generated.go:1:0\n",
        "//line generated.go:1:1073741825\n",
        "//line generated.go:1 \n",
    ] {
        let error = Scanner::new("main.go", invalid)
            .into_iter()
            .find_map(Result::err)
            .expect("invalid directive must fail scanning");
        assert_eq!(error.kind, super::ScannerErrorKind::InvalidDirective);
    }
}

#[test]
fn parser_errors_report_exact_initial_and_virtual_names() {
    let error = parse_file("main.go", "package )\n").unwrap_err();
    assert_eq!(error.adjusted_filename(), "main.go");

    let error = parse_file(
        r"C:\workspace\main.go",
        "package sample\n//line generated.go:40\nfunc )\n",
    )
    .unwrap_err();
    assert_eq!(error.adjusted_filename(), r"C:\workspace\generated.go");
    assert_eq!(error.logical_position().line().get(), 40);
    assert_eq!(error.logical_position().column(), LogicalColumn::Hidden);
}

#[test]
fn scanner_errors_report_the_active_virtual_name() {
    let error = Scanner::new("mem://workspace/main.go", "//line generated.go:40\n@\n")
        .into_iter()
        .find_map(Result::err)
        .unwrap();

    assert_eq!(error.file, "mem://workspace/generated.go");
    assert_eq!((error.line, error.column), (40, 0));
    assert_eq!(
        error.to_string(),
        "mem://workspace/generated.go:40: illegal character"
    );
    let parser_error =
        parse_file("mem://workspace/main.go", "//line generated.go:40\n@\n").unwrap_err();
    assert_eq!(
        parser_error.adjusted_filename(),
        "mem://workspace/generated.go"
    );
    assert_eq!(parser_error.logical_position().line().get(), 40);
    assert_eq!(
        parser_error.logical_position().column(),
        LogicalColumn::Hidden
    );
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
