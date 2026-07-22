#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn source_map_tracker_basic() {
    let mut tracker = SourceMapTracker::new();
    let go_source = "package main\n\nfunc main() {}";

    tracker.start("test.go", "test.rs", Some(go_source));
    assert!(tracker.is_active());
    tracker.record(3, 1, Some("fn"));
    tracker.record(3, 6, Some("main"));

    let source_map = tracker.build_source_map("pub fn main() {}\n");
    let mut buffer = Vec::new();
    source_map.to_writer(&mut buffer).unwrap();
    let parsed = SourceMap::from_reader(&buffer[..]).unwrap();

    assert!(parsed.get_token_count() > 0);
    assert_eq!(parsed.get_source(0), Some("test.go"));
    assert_eq!(parsed.get_file(), Some("test.rs"));
}

#[test]
fn source_map_tracker_multiple_sources() {
    let mut tracker = SourceMapTracker::new();
    tracker.start_many(
        vec![
            ("main.go".to_string(), Some("package main".to_string())),
            ("helper.go".to_string(), Some("package main".to_string())),
        ],
        "main.rs",
    );

    tracker.record_for_source(Some("main.go".to_string()), 3, 1, Some("main"));
    tracker.record_for_source(Some("helper.go".to_string()), 3, 1, Some("helper"));

    let source_map = tracker.build_source_map("fn main() { helper(); }\nfn helper() {}\n");
    let mut buffer = Vec::new();
    source_map.to_writer(&mut buffer).unwrap();
    let parsed = SourceMap::from_reader(&buffer[..]).unwrap();

    assert_eq!(parsed.get_source(0), Some("main.go"));
    assert_eq!(parsed.get_source(1), Some("helper.go"));
    assert!(parsed.get_token_count() >= 2);
}

#[test]
fn source_map_tracker_inactive() {
    let mut tracker = SourceMapTracker::new();
    assert!(!tracker.is_active());

    tracker.record(1, 1, Some("test"));
    assert!(tracker.pending_mappings().is_empty());
}

#[test]
fn source_map_tracker_pause_retains_completed_mappings() {
    let mut tracker = SourceMapTracker::new();
    tracker.start("main.go", "main.rs", Some("package main"));
    tracker.record(1, 1, Some("main"));
    tracker.pause();
    tracker.record_for_source(Some("stdlib.go".to_string()), 2, 1, Some("ignored"));

    assert!(tracker.is_active());
    assert_eq!(tracker.pending_mappings().len(), 1);
    assert_eq!(
        tracker
            .pending_mappings()
            .first()
            .and_then(|mapping| mapping.source.as_deref()),
        Some("main.go")
    );
}

#[test]
fn extracts_tokens() {
    let tokens = extract_tokens("fn main() { let x = 42; }");
    let names: Vec<&str> = tokens.iter().map(|token| token.text.as_str()).collect();

    assert!(names.contains(&"fn"));
    assert!(names.contains(&"main"));
    assert!(names.contains(&"let"));
    assert!(names.contains(&"x"));
    assert!(names.contains(&"42"));
}

#[test]
fn tracks_token_positions() {
    let tokens = extract_tokens("fn main() {}");

    let fn_token = tokens.first().unwrap();
    assert_eq!(fn_token.text, "fn");
    assert_eq!(fn_token.start_line, 1);
    assert_eq!(fn_token.start_column, 1);

    let main_token = tokens.get(1).unwrap();
    assert_eq!(main_token.text, "main");
    assert_eq!(main_token.start_line, 1);
    assert_eq!(main_token.start_column, 4);
}

#[test]
fn converts_go_byte_columns_to_utf16_columns() {
    assert_eq!(utf8_byte_column_to_utf16("éx", 3), 2);
    assert_eq!(utf8_byte_column_to_utf16("😀x", 5), 3);
    assert_eq!(utf8_byte_column_to_utf16("é😀x", 7), 4);
    assert_eq!(utf8_byte_column_to_utf16("é😀x", 0), 0);
}

#[test]
fn source_map_columns_use_utf16_for_go_and_rust() {
    let go_source = "var _ = \"é😀\"; target := 1";
    let rust_source = "fn f() { let _ = \"é😀\"; let target = 1; }";
    let mut tracker = SourceMapTracker::new();
    tracker.start("main.go", "main.rs", Some(go_source));
    tracker.record(
        1,
        (go_source.find("target").unwrap() + 1) as u32,
        Some("target"),
    );

    let source_map = tracker.build_source_map(rust_source);
    let token = source_map
        .tokens()
        .find(|token| token.get_name() == Some("target"))
        .unwrap();
    let go_byte_offset = go_source.find("target").unwrap();
    let rust_byte_offset = rust_source.find("target").unwrap();

    assert_eq!(
        token.get_src_col(),
        go_source[..go_byte_offset].encode_utf16().count() as u32
    );
    assert_eq!(
        token.get_dst_col(),
        rust_source[..rust_byte_offset].encode_utf16().count() as u32
    );
}

#[test]
fn source_map_does_not_convert_columns_with_unavailable_source_text() {
    let mut tracker = SourceMapTracker::new();
    tracker.start("main.go", "main.rs", Some("é😀target"));
    tracker.record_for_source(Some("virtual.go".to_string()), 1, 7, Some("target"));

    let source_map = tracker.build_source_map("let target = 1;");
    let token = source_map
        .tokens()
        .find(|token| token.get_name() == Some("target"))
        .unwrap();

    assert_eq!(token.get_src_col(), 6);
}
