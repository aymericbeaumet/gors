//! Source mapping using standard Source Map v3 format.
//!
//! This module provides source map tracking during Go-to-Rust compilation.
//! Go positions are collected during compilation, and the final source map
//! is built during code generation when Rust positions become available.

pub use sourcemap::{SourceMap, SourceMapBuilder};

use std::collections::HashMap;

/// Convert a 1-based UTF-8 byte column from the Go scanner into a 1-based
/// UTF-16 code-unit column for Source Map v3 and Monaco.
///
/// A zero column is preserved for positions hidden by Go `//line` directives.
/// Columns beyond the line are clamped to its end, and a column inside a
/// multi-byte scalar is clamped to that scalar's start.
pub fn utf8_byte_column_to_utf16(line: &str, column: u32) -> u32 {
    if column == 0 {
        return 0;
    }

    let mut byte_offset = (column.saturating_sub(1) as usize).min(line.len());
    while byte_offset > 0 && !line.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    let utf16_offset = line[..byte_offset]
        .encode_utf16()
        .count()
        .min(u32::MAX as usize) as u32;
    utf16_offset.saturating_add(1)
}

/// A pending mapping collected during compilation.
/// Contains Go source position and optional name, waiting for Rust position.
#[derive(Debug, Clone)]
pub struct PendingMapping {
    /// Source file for this original position
    pub source: Option<String>,
    /// Original line (1-based)
    pub orig_line: u32,
    /// Original column (1-based)
    pub orig_col: u32,
    /// Optional identifier name
    pub name: Option<String>,
}

/// Tracker for collecting source mappings during compilation.
#[derive(Default)]
pub struct SourceMapTracker {
    /// Pending mappings collected during compilation
    pending: Vec<PendingMapping>,
    /// Go source files and optional contents
    sources: Vec<(String, Option<String>)>,
    /// Rust output file path
    rust_file: Option<String>,
    /// Whether lowering should append new mappings. Sources and completed
    /// mappings remain available after recording is paused for code generation.
    recording: bool,
}

impl SourceMapTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking for a compilation.
    pub fn start(&mut self, go_file: &str, rust_file: &str, go_source: Option<&str>) {
        self.start_many(
            vec![(go_file.to_string(), go_source.map(ToString::to_string))],
            rust_file,
        );
    }

    /// Start tracking for a compilation with multiple Go source files.
    pub fn start_many(&mut self, sources: Vec<(String, Option<String>)>, rust_file: &str) {
        self.pending.clear();
        self.sources = sources;
        self.rust_file = Some(rust_file.to_string());
        self.recording = true;
    }

    /// Stop accepting mappings while retaining the completed map inputs.
    pub fn pause(&mut self) {
        self.recording = false;
    }

    /// Check if tracking is active.
    pub fn is_active(&self) -> bool {
        !self.sources.is_empty()
    }

    /// Record a Go position during compilation.
    /// The Rust position will be determined during code generation.
    pub fn record(&mut self, orig_line: u32, orig_col: u32, name: Option<&str>) {
        let source = self.sources.first().map(|(source, _)| source.clone());
        self.record_for_source(source, orig_line, orig_col, name);
    }

    /// Record a Go position for a specific source file.
    pub fn record_for_source(
        &mut self,
        source: Option<String>,
        orig_line: u32,
        orig_col: u32,
        name: Option<&str>,
    ) {
        if self.sources.is_empty() || !self.recording {
            return;
        }
        self.pending.push(PendingMapping {
            source,
            orig_line,
            orig_col,
            name: name.map(|s| s.to_string()),
        });
    }

    /// Get pending mappings (for use during codegen).
    pub fn pending_mappings(&self) -> &[PendingMapping] {
        &self.pending
    }

    /// Build the final source map given the generated Rust source.
    /// This matches pending mappings to tokens in the Rust output. Scanner
    /// columns remain UTF-8 byte offsets until this boundary; emitted Source
    /// Map v3 columns are UTF-16 code-unit offsets.
    pub fn build_source_map(&self, rust_source: &str) -> SourceMap {
        let mut builder = SourceMapBuilder::new(self.rust_file.as_deref());

        let mut source_indices = HashMap::new();
        let mut source_lines = HashMap::new();
        if self.sources.is_empty() {
            let src_idx = builder.add_source("input.go");
            source_indices.insert("input.go".to_string(), src_idx);
        } else {
            for (source, content) in &self.sources {
                let src_idx = builder.add_source(source);
                if let Some(content) = content {
                    builder.set_source_contents(src_idx, Some(content.as_str()));
                    source_lines.insert(source.as_str(), content.lines().collect::<Vec<_>>());
                }
                source_indices.insert(source.clone(), src_idx);
            }
        }
        let fallback_source = self.sources.first().map(|(source, _)| source.as_str());
        let fallback_source_idx = fallback_source
            .and_then(|source| source_indices.get(source))
            .copied()
            .or_else(|| source_indices.values().next().copied());
        let fallback_source_lines = fallback_source.and_then(|source| source_lines.get(source));

        // Extract tokens from the Rust source
        let tokens = extract_tokens(rust_source);

        // Build a map of name -> tokens for matching
        let mut name_to_tokens: HashMap<&str, Vec<&TokenInfo>> = HashMap::new();
        for token in &tokens {
            name_to_tokens.entry(&token.text).or_default().push(token);
        }

        // Track which token index we've used for each Rust token name
        let mut name_indices: HashMap<String, usize> = HashMap::new();

        // Match pending mappings to Rust tokens
        // Go names are stored in pending.name, but we need to find the corresponding Rust token
        for pending in &self.pending {
            if let Some(ref go_name) = pending.name {
                // Get the Rust token name to search for
                let rust_name = go_name_to_rust_name(go_name);

                if let Some(matching_tokens) = name_to_tokens.get(rust_name) {
                    let idx = name_indices.entry(rust_name.to_string()).or_insert(0);
                    if let Some(token) = matching_tokens.get(*idx) {
                        // Store the Go name in the source map (not the Rust name)
                        let name_idx = builder.add_name(go_name);
                        let src_idx = pending
                            .source
                            .as_ref()
                            .and_then(|source| source_indices.get(source))
                            .copied()
                            .or(fallback_source_idx);
                        let original_lines = match pending.source.as_deref() {
                            Some(source) => source_lines.get(source),
                            None => fallback_source_lines,
                        };
                        let original_column = original_lines
                            .and_then(|lines| {
                                lines.get(pending.orig_line.saturating_sub(1) as usize)
                            })
                            .map(|line| {
                                utf8_byte_column_to_utf16(line, pending.orig_col).saturating_sub(1)
                            })
                            .unwrap_or_else(|| pending.orig_col.saturating_sub(1));
                        builder.add_raw(
                            token.start_line.saturating_sub(1),   // generated line (0-based)
                            token.start_column.saturating_sub(1), // generated column (0-based)
                            pending.orig_line.saturating_sub(1),  // original line (0-based)
                            original_column, // original UTF-16 column (0-based)
                            src_idx,
                            Some(name_idx),
                            false, // is_range: false for point mappings
                        );
                        *idx += 1;
                    }
                }
            }
        }

        builder.into_sourcemap()
    }

    /// Clear the tracker state.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.sources.clear();
        self.rust_file = None;
        self.recording = false;
    }
}

/// Map Go token name to the corresponding Rust token name for matching.
/// This is used during source map building to find Rust tokens that correspond to Go tokens.
/// The actual Go name is still stored in the source map for highlighting.
fn go_name_to_rust_name(go_name: &str) -> &str {
    match go_name {
        "func" => "fn",
        _ => go_name,
    }
}

/// Token information extracted from Rust source.
#[derive(Debug, Clone)]
struct TokenInfo {
    text: String,
    start_line: u32,
    start_column: u32,
}

fn utf16_width(ch: char) -> u32 {
    ch.len_utf16() as u32
}

/// Extract token positions from Rust source code.
fn extract_tokens(rust_source: &str) -> Vec<TokenInfo> {
    let mut tokens = Vec::new();
    let mut line: u32 = 1;
    let mut column: u32 = 1;
    let chars: Vec<char> = rust_source.chars().collect();
    let mut i = 0;

    while let Some(ch) = chars.get(i).copied() {
        // Skip whitespace (but track position)
        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += utf16_width(ch);
            }
            i += 1;
            continue;
        }

        // Skip comments
        if ch == '/' {
            if chars.get(i + 1).is_some_and(|next| *next == '/') {
                while let Some(current) = chars.get(i).copied() {
                    if current == '\n' {
                        break;
                    }
                    i += 1;
                    column += utf16_width(current);
                }
                continue;
            } else if chars.get(i + 1).is_some_and(|next| *next == '*') {
                i += 2;
                column += 2;
                while let Some(current) = chars.get(i).copied() {
                    if chars
                        .get(i + 1)
                        .is_some_and(|next| current == '*' && *next == '/')
                    {
                        break;
                    }
                    if current == '\n' {
                        line += 1;
                        column = 1;
                    } else {
                        column += utf16_width(current);
                    }
                    i += 1;
                }
                if chars.get(i + 1).is_some() {
                    i += 2;
                    column += 2;
                }
                continue;
            }
        }

        let start_line = line;
        let start_column = column;

        // Identifier or keyword
        if ch.is_alphabetic() || ch == '_' {
            let mut text = String::new();
            while let Some(current) = chars.get(i).copied() {
                if !(current.is_alphanumeric() || current == '_') {
                    break;
                }
                text.push(current);
                column += utf16_width(current);
                i += 1;
            }
            tokens.push(TokenInfo {
                text,
                start_line,
                start_column,
            });
            continue;
        }

        // Number literal
        if ch.is_ascii_digit() {
            let mut text = String::new();
            while let Some(current) = chars.get(i).copied() {
                if !(current.is_ascii_digit()
                    || current == '.'
                    || current == 'x'
                    || current == 'X'
                    || current == 'b'
                    || current == 'B'
                    || current == 'o'
                    || current == 'O'
                    || current == 'e'
                    || current == 'E'
                    || current == '_'
                    || current.is_ascii_hexdigit())
                {
                    break;
                }
                text.push(current);
                column += utf16_width(current);
                i += 1;
            }
            // Handle type suffixes
            while let Some(current) = chars.get(i).copied() {
                if !(current.is_alphabetic() || current == '_') {
                    break;
                }
                text.push(current);
                column += 1;
                i += 1;
            }
            tokens.push(TokenInfo {
                text,
                start_line,
                start_column,
            });
            continue;
        }

        // String literal
        if ch == '"' {
            let mut text = String::new();
            text.push(ch);
            column += 1;
            i += 1;
            while chars.get(i).is_some_and(|current| *current != '"') {
                let Some(current) = chars.get(i).copied() else {
                    break;
                };
                if current == '\\' && chars.get(i + 1).is_some() {
                    text.push(current);
                    column += 1;
                    i += 1;
                }
                let Some(current) = chars.get(i).copied() else {
                    break;
                };
                if current == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += utf16_width(current);
                }
                text.push(current);
                i += 1;
            }
            if let Some(current) = chars.get(i).copied() {
                text.push(current);
                column += utf16_width(current);
                i += 1;
            }
            tokens.push(TokenInfo {
                text,
                start_line,
                start_column,
            });
            continue;
        }

        // Character literal
        if ch == '\'' {
            let mut text = String::new();
            text.push(ch);
            column += 1;
            i += 1;
            while chars.get(i).is_some_and(|current| *current != '\'') {
                let Some(current) = chars.get(i).copied() else {
                    break;
                };
                if current == '\\' && chars.get(i + 1).is_some() {
                    text.push(current);
                    column += 1;
                    i += 1;
                }
                let Some(current) = chars.get(i).copied() else {
                    break;
                };
                text.push(current);
                column += utf16_width(current);
                i += 1;
            }
            if let Some(current) = chars.get(i).copied() {
                text.push(current);
                column += utf16_width(current);
                i += 1;
            }
            tokens.push(TokenInfo {
                text,
                start_line,
                start_column,
            });
            continue;
        }

        // Skip other characters (operators, punctuation)
        column += utf16_width(ch);
        i += 1;
    }

    tokens
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_source_map_tracker_basic() {
        let mut tracker = SourceMapTracker::new();
        let go_source = "package main\n\nfunc main() {}";

        tracker.start("test.go", "test.rs", Some(go_source));
        assert!(tracker.is_active());

        // Record some Go positions
        tracker.record(3, 1, Some("fn"));
        tracker.record(3, 6, Some("main"));

        // Build source map with Rust output
        let rust_source = "pub fn main() {}\n";
        let sm = tracker.build_source_map(rust_source);

        // Verify we can serialize and parse it back
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        let parsed = SourceMap::from_reader(&buf[..]).unwrap();

        assert!(parsed.get_token_count() > 0);
        assert_eq!(parsed.get_source(0), Some("test.go"));
        assert_eq!(parsed.get_file(), Some("test.rs"));
    }

    #[test]
    fn test_source_map_tracker_multiple_sources() {
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

        let sm = tracker.build_source_map("fn main() { helper(); }\nfn helper() {}\n");
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        let parsed = SourceMap::from_reader(&buf[..]).unwrap();

        assert_eq!(parsed.get_source(0), Some("main.go"));
        assert_eq!(parsed.get_source(1), Some("helper.go"));
        assert!(parsed.get_token_count() >= 2);
    }

    #[test]
    fn test_source_map_tracker_inactive() {
        let mut tracker = SourceMapTracker::new();
        assert!(!tracker.is_active());

        // Recording when inactive should be a no-op
        tracker.record(1, 1, Some("test"));
        assert!(tracker.pending_mappings().is_empty());
    }

    #[test]
    fn test_source_map_tracker_pause_retains_completed_mappings() {
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
    fn test_extract_tokens() {
        let source = "fn main() { let x = 42; }";
        let tokens = extract_tokens(source);

        let names: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert!(names.contains(&"fn"));
        assert!(names.contains(&"main"));
        assert!(names.contains(&"let"));
        assert!(names.contains(&"x"));
        assert!(names.contains(&"42"));
    }

    #[test]
    fn test_token_positions() {
        let source = "fn main() {}";
        let tokens = extract_tokens(source);

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
}
