use crate::compiler::source::{LogicalColumn, TextSize};
use crate::scanner::ScannerErrorKind;

use super::{ParserErrorKind, parse_file};

#[test]
fn successful_parse_publishes_the_complete_scanner_map() {
    let source = "package main\n//line generated.go:40\nfunc main() {}\n";
    let parsed = parse_file("main.go", source).unwrap();
    let map = parsed.source_coordinate_map();
    let eof = TextSize::try_from(source.len()).unwrap();

    assert_eq!(map.text_len(), eof);
    assert!(map.physical_coordinate(eof).unwrap().is_some());
    let adjusted = map.adjusted_coordinate(eof).unwrap().unwrap();
    assert_eq!(adjusted.filename(), "generated.go");
    assert_eq!(adjusted.position().column(), LogicalColumn::Hidden);
}

#[test]
fn unexpected_token_uses_physical_anchor_and_virtual_hidden_coordinate() {
    let source = "package main\n//line generated.go:40\nfunc broken( {";
    let error = parse_file("main.go", source).unwrap_err();

    assert!(matches!(
        error.kind(),
        ParserErrorKind::UnexpectedToken { .. }
    ));
    assert_eq!(
        error.physical_range().start().to_usize(),
        source.find('{').unwrap()
    );
    assert_eq!(error.adjusted_filename(), "generated.go");
    assert_eq!(error.logical_position().line().get(), 40);
    assert_eq!(error.logical_position().column(), LogicalColumn::Hidden);
}

#[test]
fn unexpected_eof_is_exactly_anchored_and_located() {
    let source = "package main\n//line eof.go:70\nfunc broken(";
    let error = parse_file("main.go", source).unwrap_err();

    assert!(matches!(error.kind(), ParserErrorKind::UnexpectedEndOfFile));
    assert_eq!(error.physical_range().start().to_usize(), source.len());
    assert_eq!(error.adjusted_filename(), "eof.go");
    assert_eq!(error.logical_position().line().get(), 70);
    assert_eq!(error.logical_position().column(), LogicalColumn::Hidden);
    assert_eq!(
        error.source_coordinate_map().text_len().to_usize(),
        source.len()
    );
}

#[test]
fn scanner_failure_uses_the_same_consumed_map_and_exact_anchor() {
    let source = "package main\n//line scan.go:50\n@";
    let error = parse_file("main.go", source).unwrap_err();

    assert_eq!(
        error.kind(),
        &ParserErrorKind::Scanner(ScannerErrorKind::IllegalCharacter)
    );
    assert_eq!(
        error.physical_range().start().to_usize(),
        source.find('@').unwrap()
    );
    assert_eq!(error.adjusted_filename(), "scan.go");
    assert_eq!(error.logical_position().line().get(), 50);
    assert_eq!(error.logical_position().column(), LogicalColumn::Hidden);
    assert_eq!(
        error.source_coordinate_map().text_len().to_usize(),
        source.find('@').unwrap()
    );
}

#[test]
fn empty_two_field_directive_filename_stays_empty() {
    let source = "package main\n//line :30\n@";
    let error = parse_file("main.go", source).unwrap_err();

    assert_eq!(error.adjusted_filename(), "");
    assert_eq!(error.logical_position().line().get(), 30);
    assert_eq!(error.logical_position().column(), LogicalColumn::Hidden);
    assert_eq!(error.to_string(), "30: illegal character");
}

#[test]
fn directive_transition_at_eof_is_not_published() {
    let source = "package main\n//line ignored.go:99";
    let parsed = parse_file("main.go", source).unwrap();
    let eof = TextSize::try_from(source.len()).unwrap();
    let adjusted = parsed
        .source_coordinate_map()
        .adjusted_coordinate(eof)
        .unwrap()
        .unwrap();

    assert_eq!(adjusted.filename(), "main.go");
}
