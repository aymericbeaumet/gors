//! Source mapping using standard Source Map v3 format.
//!
//! This module provides source map tracking during Go-to-Rust compilation.
//! Go positions are collected during compilation, and the final source map
//! is built during code generation when Rust positions become available.

pub use sourcemap::{SourceMap, SourceMapBuilder};

use std::collections::HashMap;

/// A pending mapping collected during compilation.
/// Contains Go source position and optional name, waiting for Rust position.
#[derive(Debug, Clone)]
pub struct PendingMapping {
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
    /// Go source file path
    go_file: Option<String>,
    /// Rust output file path
    rust_file: Option<String>,
    /// Go source content
    go_source: Option<String>,
}

impl SourceMapTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking for a compilation.
    pub fn start(&mut self, go_file: &str, rust_file: &str, go_source: Option<&str>) {
        self.pending.clear();
        self.go_file = Some(go_file.to_string());
        self.rust_file = Some(rust_file.to_string());
        self.go_source = go_source.map(|s| s.to_string());
    }

    /// Check if tracking is active.
    pub fn is_active(&self) -> bool {
        self.go_file.is_some()
    }

    /// Record a Go position during compilation.
    /// The Rust position will be determined during code generation.
    pub fn record(&mut self, orig_line: u32, orig_col: u32, name: Option<&str>) {
        if self.go_file.is_some() {
            self.pending.push(PendingMapping {
                orig_line,
                orig_col,
                name: name.map(|s| s.to_string()),
            });
        }
    }

    /// Get pending mappings (for use during codegen).
    pub fn pending_mappings(&self) -> &[PendingMapping] {
        &self.pending
    }

    /// Build the final source map given the generated Rust source.
    /// This matches pending mappings to tokens in the Rust output.
    pub fn build_source_map(&self, rust_source: &str) -> SourceMap {
        let mut builder = SourceMapBuilder::new(self.rust_file.as_deref());

        let go_file = self.go_file.as_deref().unwrap_or("input.go");
        let src_idx = builder.add_source(go_file);
        if let Some(ref content) = self.go_source {
            builder.set_source_contents(src_idx, Some(content.as_str()));
        }

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
                    if *idx < matching_tokens.len() {
                        let token = matching_tokens[*idx];
                        // Store the Go name in the source map (not the Rust name)
                        let name_idx = builder.add_name(go_name);
                        builder.add_raw(
                            token.start_line.saturating_sub(1),   // generated line (0-based)
                            token.start_column.saturating_sub(1), // generated column (0-based)
                            pending.orig_line.saturating_sub(1),  // original line (0-based)
                            pending.orig_col.saturating_sub(1),   // original column (0-based)
                            Some(src_idx),
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
        self.go_file = None;
        self.rust_file = None;
        self.go_source = None;
    }
}

/// Map Go token name to the corresponding Rust token name for matching.
/// This is used during source map building to find Rust tokens that correspond to Go tokens.
/// The actual Go name is still stored in the source map for highlighting.
fn go_name_to_rust_name(go_name: &str) -> &str {
    match go_name {
        // Keywords that change name
        "func" => "fn",
        "Println" => "println",
        "Printf" => "print", // fmt.Printf -> print! macro
        "Print" => "print",
        // Most identifiers keep the same name
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

/// Extract token positions from Rust source code.
fn extract_tokens(rust_source: &str) -> Vec<TokenInfo> {
    let mut tokens = Vec::new();
    let mut line: u32 = 1;
    let mut column: u32 = 1;
    let chars: Vec<char> = rust_source.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // Skip whitespace (but track position)
        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            i += 1;
            continue;
        }

        // Skip comments
        if ch == '/' && i + 1 < chars.len() {
            if chars[i + 1] == '/' {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                    column += 1;
                }
                continue;
            } else if chars[i + 1] == '*' {
                i += 2;
                column += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    if chars[i] == '\n' {
                        line += 1;
                        column = 1;
                    } else {
                        column += 1;
                    }
                    i += 1;
                }
                if i + 1 < chars.len() {
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
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                text.push(chars[i]);
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

        // Number literal
        if ch.is_ascii_digit() {
            let mut text = String::new();
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'x'
                    || chars[i] == 'X'
                    || chars[i] == 'b'
                    || chars[i] == 'B'
                    || chars[i] == 'o'
                    || chars[i] == 'O'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || chars[i] == '_'
                    || chars[i].is_ascii_hexdigit())
            {
                text.push(chars[i]);
                column += 1;
                i += 1;
            }
            // Handle type suffixes
            while i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_') {
                text.push(chars[i]);
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
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    text.push(chars[i]);
                    column += 1;
                    i += 1;
                }
                if chars[i] == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
                text.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                text.push(chars[i]);
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

        // Character literal
        if ch == '\'' {
            let mut text = String::new();
            text.push(ch);
            column += 1;
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    text.push(chars[i]);
                    column += 1;
                    i += 1;
                }
                text.push(chars[i]);
                column += 1;
                i += 1;
            }
            if i < chars.len() {
                text.push(chars[i]);
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

        // Skip other characters (operators, punctuation)
        column += 1;
        i += 1;
    }

    tokens
}

#[cfg(test)]
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
    fn test_source_map_tracker_inactive() {
        let mut tracker = SourceMapTracker::new();
        assert!(!tracker.is_active());

        // Recording when inactive should be a no-op
        tracker.record(1, 1, Some("test"));
        assert!(tracker.pending_mappings().is_empty());
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

        let fn_token = &tokens[0];
        assert_eq!(fn_token.text, "fn");
        assert_eq!(fn_token.start_line, 1);
        assert_eq!(fn_token.start_column, 1);

        let main_token = &tokens[1];
        assert_eq!(main_token.text, "main");
        assert_eq!(main_token.start_line, 1);
        assert_eq!(main_token.start_column, 4);
    }
}
