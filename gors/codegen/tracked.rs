//! Position-tracking Rust code generation.
//!
//! This module provides code generation that tracks output positions,
//! enabling correlation between Go source positions and Rust output positions.

use crate::mapping::{RustSpan, SourceMap};
use std::collections::HashMap;

/// Token information extracted from the Rust output.
#[derive(Debug, Clone)]
struct TokenInfo {
    /// The token text
    text: String,
    /// Start line (1-based)
    start_line: u32,
    /// Start column (1-based)
    start_column: u32,
    /// End line (1-based)
    end_line: u32,
    /// End column (1-based)
    end_column: u32,
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
                // Line comment
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                    column += 1;
                }
                continue;
            } else if chars[i + 1] == '*' {
                // Block comment
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
                end_line: line,
                end_column: column,
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
                    || (chars[i].is_ascii_hexdigit()))
            {
                text.push(chars[i]);
                column += 1;
                i += 1;
            }
            // Handle type suffixes like i32, u64, etc.
            while i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_') {
                text.push(chars[i]);
                column += 1;
                i += 1;
            }
            tokens.push(TokenInfo {
                text,
                start_line,
                start_column,
                end_line: line,
                end_column: column,
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
                end_line: line,
                end_column: column,
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
                end_line: line,
                end_column: column,
            });
            continue;
        }

        // Operators and punctuation (multi-character)
        let remaining: String = chars[i..].iter().take(3).collect();
        let op_len = if remaining.starts_with("<<=")
            || remaining.starts_with(">>=")
            || remaining.starts_with("...")
        {
            3
        } else if remaining.starts_with("->")
            || remaining.starts_with("=>")
            || remaining.starts_with("::")
            || remaining.starts_with("==")
            || remaining.starts_with("!=")
            || remaining.starts_with("<=")
            || remaining.starts_with(">=")
            || remaining.starts_with("&&")
            || remaining.starts_with("||")
            || remaining.starts_with("<<")
            || remaining.starts_with(">>")
            || remaining.starts_with("+=")
            || remaining.starts_with("-=")
            || remaining.starts_with("*=")
            || remaining.starts_with("/=")
            || remaining.starts_with("%=")
            || remaining.starts_with("&=")
            || remaining.starts_with("|=")
            || remaining.starts_with("^=")
        {
            2
        } else {
            1
        };

        let text: String = chars[i..i + op_len].iter().collect();
        tokens.push(TokenInfo {
            text,
            start_line,
            start_column,
            end_line: line,
            end_column: column + op_len as u32,
        });
        column += op_len as u32;
        i += op_len;
    }

    tokens
}

/// Update a SourceMap with Rust positions by matching tokens.
///
/// This function scans the Rust output and matches identifiers/literals
/// back to the source map entries by name.
pub fn update_source_map_positions(source_map: &mut SourceMap, rust_source: &str) {
    let tokens = extract_tokens(rust_source);

    // Identify lines that are `use` statements (hoisted imports should be skipped for matching)
    let use_statement_lines: std::collections::HashSet<u32> = rust_source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("use "))
        .map(|(i, _)| (i + 1) as u32)
        .collect();

    // Build a map of name -> tokens for quick lookup, excluding tokens in use statements
    let mut name_to_tokens: HashMap<&str, Vec<&TokenInfo>> = HashMap::new();
    for token in &tokens {
        // Skip tokens that are part of use statements (these are hoisted and shouldn't match)
        if use_statement_lines.contains(&token.start_line) {
            continue;
        }
        name_to_tokens.entry(&token.text).or_default().push(token);
    }

    // Track which token index we've used for each name
    let mut name_indices: HashMap<String, usize> = HashMap::new();

    // Update each mapping's Rust span
    for mapping in source_map.mappings_mut() {
        if let Some(ref name) = mapping.name {
            // Try to find a matching token
            if let Some(matching_tokens) = name_to_tokens.get(name.as_str()) {
                let idx = name_indices.entry(name.clone()).or_insert(0);
                if *idx < matching_tokens.len() {
                    let token = matching_tokens[*idx];
                    mapping.rust_span = RustSpan::new(
                        token.start_line,
                        token.start_column,
                        token.end_line,
                        token.end_column,
                    );
                    *idx += 1;
                }
            }
        }
    }
}

/// Information about a comment to be inserted.
#[derive(Debug, Clone)]
pub struct CommentToInsert {
    /// The Go line number (1-based)
    pub go_line: u32,
    /// The comment text (including // or /* */)
    pub text: String,
    /// Whether this is a doc comment (already handled by attributes)
    pub is_doc: bool,
}

/// Information about blank lines in the Go source.
#[derive(Debug, Clone, Default)]
pub struct BlankLineInfo {
    /// Set of Go line numbers that are followed by one or more blank lines
    pub lines_with_trailing_blank: std::collections::HashSet<u32>,
}

/// Generate Rust code with position tracking.
pub fn generate_with_positions(
    file: syn::File,
    source_map: &mut SourceMap,
) -> Result<String, Box<dyn std::error::Error>> {
    // First, generate the Rust code using prettyplease
    let formatted = prettyplease::unparse(&file);

    // Add extra newlines before function definitions for readability
    let mut output = String::new();
    for (i, line) in formatted.lines().enumerate() {
        if i > 0 && (line.starts_with("fn") || line.starts_with("pub fn")) {
            output.push('\n');
        }
        output.push_str(line);
        output.push('\n');
    }

    // Update the source map with Rust positions
    update_source_map_positions(source_map, &output);

    Ok(output)
}

/// Generate Rust code with position tracking and comment insertion.
pub fn generate_with_comments(
    file: syn::File,
    source_map: &mut SourceMap,
    comments: &[CommentToInsert],
) -> Result<String, Box<dyn std::error::Error>> {
    generate_with_comments_and_blanks(file, source_map, comments, &BlankLineInfo::default())
}

/// Generate Rust code with position tracking, comment insertion, and blank line preservation.
pub fn generate_with_comments_and_blanks(
    file: syn::File,
    source_map: &mut SourceMap,
    comments: &[CommentToInsert],
    blank_lines: &BlankLineInfo,
) -> Result<String, Box<dyn std::error::Error>> {
    // First, generate the Rust code using prettyplease
    let formatted = prettyplease::unparse(&file);

    // Update source map positions first so we have accurate line mappings
    update_source_map_positions(source_map, &formatted);

    // Build direct line mapping
    let direct_mapping = build_line_mapping(source_map);
    let mut mapped_lines: Vec<u32> = direct_mapping.keys().copied().collect();
    mapped_lines.sort();

    // Build reverse mapping: Rust line -> Go line
    let mut rust_to_go: HashMap<u32, u32> = HashMap::new();
    for (&go_line, &rust_line) in &direct_mapping {
        rust_to_go.entry(rust_line).or_insert(go_line);
    }

    // Group non-doc comments by their placement (before/after which Rust line)
    let mut comments_before: HashMap<u32, Vec<&CommentToInsert>> = HashMap::new();
    let mut comments_after: HashMap<u32, Vec<&CommentToInsert>> = HashMap::new();

    for comment in comments {
        if comment.is_doc {
            continue; // Doc comments are already handled via attributes
        }

        if let Some(placement) = find_comment_placement(comment.go_line, &direct_mapping, &mapped_lines) {
            if placement.place_before {
                comments_before
                    .entry(placement.rust_line)
                    .or_default()
                    .push(comment);
            } else {
                comments_after
                    .entry(placement.rust_line)
                    .or_default()
                    .push(comment);
            }
        }
    }

    // Sort comments within each group by their original Go line number
    for comments in comments_before.values_mut() {
        comments.sort_by_key(|c| c.go_line);
    }
    for comments in comments_after.values_mut() {
        comments.sort_by_key(|c| c.go_line);
    }

    // Build output with comments and blank lines inserted
    let mut output = String::new();
    let lines: Vec<&str> = formatted.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let rust_line = (i + 1) as u32;

        // Add extra newline before function definitions
        if i > 0 && (line.starts_with("fn") || line.starts_with("pub fn")) {
            output.push('\n');
        }

        // Insert any comments that should appear BEFORE this line
        if let Some(line_comments) = comments_before.get(&rust_line) {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            for comment in line_comments {
                output.push_str(&indent);
                output.push_str(&comment.text);
                output.push('\n');
            }
        }

        // Output the actual line
        output.push_str(line);
        output.push('\n');

        // Check if we need to insert a blank line after this Rust line
        let needs_blank = rust_to_go
            .get(&rust_line)
            .map(|&go_line| blank_lines.lines_with_trailing_blank.contains(&go_line))
            .unwrap_or(false);

        // Insert blank line BEFORE any trailing comments (so blank separates code from comments)
        if needs_blank {
            output.push('\n');
        }

        // Insert any comments that should appear AFTER this line
        if let Some(line_comments) = comments_after.get(&rust_line) {
            // Get indentation from the current line (the line the comment follows)
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();

            for comment in line_comments {
                output.push_str(&indent);
                output.push_str(&comment.text);
                output.push('\n');
            }
        }
    }

    // Re-update source map positions with final output (comments may have shifted lines)
    update_source_map_positions(source_map, &output);

    Ok(output)
}

/// Build a mapping from Go line numbers to Rust line numbers.
fn build_line_mapping(source_map: &SourceMap) -> HashMap<u32, u32> {
    let mut go_to_rust: HashMap<u32, u32> = HashMap::new();

    // First, collect all direct mappings
    for mapping in source_map.mappings() {
        // Only consider mappings that have valid Rust positions
        if mapping.rust_span.start_line > 0 {
            go_to_rust
                .entry(mapping.go_span.start_line)
                .or_insert(mapping.rust_span.start_line);
        }
    }

    go_to_rust
}

/// Result of finding the placement for a comment.
#[derive(Debug, Clone, Copy)]
struct CommentPlacement {
    /// The Rust line to place the comment relative to
    rust_line: u32,
    /// Whether to place before (true) or after (false) the rust_line
    place_before: bool,
}

/// Find where to place a comment given its Go line number.
/// Returns the Rust line and whether to place before or after it.
fn find_comment_placement(
    go_line: u32,
    direct_mapping: &HashMap<u32, u32>,
    mapped_lines: &[u32],
) -> Option<CommentPlacement> {
    // Direct mapping - place before that line
    if let Some(&rust_line) = direct_mapping.get(&go_line) {
        return Some(CommentPlacement {
            rust_line,
            place_before: true,
        });
    }

    // Find previous and next mapped Go lines
    let prev_mapped = mapped_lines.iter().rev().find(|&&l| l < go_line).copied();
    let next_mapped = mapped_lines.iter().find(|&&l| l > go_line).copied();

    match (prev_mapped, next_mapped) {
        (Some(prev), Some(next)) => {
            // Comment is between two mapped lines
            // If closer to previous, place after it; if closer to next, place before it
            let dist_to_prev = go_line - prev;
            let dist_to_next = next - go_line;

            if dist_to_prev <= dist_to_next {
                // Closer to or equal distance to previous - place after previous code
                direct_mapping.get(&prev).map(|&rust_line| CommentPlacement {
                    rust_line,
                    place_before: false,
                })
            } else {
                // Closer to next - place before next code
                direct_mapping.get(&next).map(|&rust_line| CommentPlacement {
                    rust_line,
                    place_before: true,
                })
            }
        }
        (Some(prev), None) => {
            // Only previous exists - place after it (trailing comment)
            direct_mapping.get(&prev).map(|&rust_line| CommentPlacement {
                rust_line,
                place_before: false,
            })
        }
        (None, Some(next)) => {
            // Only next exists - place before it (leading comment)
            direct_mapping.get(&next).map(|&rust_line| CommentPlacement {
                rust_line,
                place_before: true,
                })
        }
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tokens_simple() {
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
    fn test_extract_tokens_with_string() {
        let source = r#"let s = "hello";"#;
        let tokens = extract_tokens(source);

        let names: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert!(names.contains(&"let"));
        assert!(names.contains(&"s"));
        assert!(names.contains(&"\"hello\""));
    }

    #[test]
    fn test_extract_tokens_multiline() {
        let source = "fn foo() {\n    let x = 1;\n}";
        let tokens = extract_tokens(source);

        // Check that x is on line 2
        let x_token = tokens.iter().find(|t| t.text == "x").unwrap();
        assert_eq!(x_token.start_line, 2);
    }

    #[test]
    fn test_token_positions() {
        let source = "fn main() {}";
        let tokens = extract_tokens(source);

        let fn_token = &tokens[0];
        assert_eq!(fn_token.text, "fn");
        assert_eq!(fn_token.start_line, 1);
        assert_eq!(fn_token.start_column, 1);
        assert_eq!(fn_token.end_column, 3);

        let main_token = &tokens[1];
        assert_eq!(main_token.text, "main");
        assert_eq!(main_token.start_line, 1);
        assert_eq!(main_token.start_column, 4);
    }
}
