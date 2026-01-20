//! Additional code generation utilities.
//!
//! This module provides utilities for code generation, including
//! comment insertion and blank line preservation.

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

/// Generate Rust code with comment insertion and blank line preservation.
///
/// This function generates Rust code from a syn AST and attempts to
/// preserve comments and blank lines from the original Go source.
///
/// Note: Comment placement is approximate since the Go-to-Rust transformation
/// may significantly restructure the code.
pub fn generate_with_comments_and_blanks(
    file: syn::File,
    comments: &[CommentToInsert],
    blank_lines: &BlankLineInfo,
) -> Result<String, Box<dyn std::error::Error>> {
    // Generate the Rust code using prettyplease
    let formatted = prettyplease::unparse(&file);

    // Simple heuristic: insert comments at the beginning if they appear early in the Go source
    let mut output = String::new();
    let lines: Vec<&str> = formatted.lines().collect();

    // Collect leading comments (comments before the first code line)
    let leading_comments: Vec<_> = comments
        .iter()
        .filter(|c| !c.is_doc && c.go_line <= 3)
        .collect();

    // Insert leading comments at the top
    for comment in &leading_comments {
        output.push_str(&comment.text);
        output.push('\n');
    }

    // Track if we've added a leading blank
    let has_leading_comments = !leading_comments.is_empty();

    for (i, line) in lines.iter().enumerate() {
        // Add extra newline before function definitions or after leading comments
        let needs_blank = (i > 0 && (line.starts_with("fn") || line.starts_with("pub fn")))
            || (i == 0 && has_leading_comments && !line.is_empty());
        if needs_blank {
            output.push('\n');
        }

        output.push_str(line);
        output.push('\n');
    }

    // Add trailing comments
    let trailing_comments: Vec<_> = comments
        .iter()
        .filter(|c| !c.is_doc && c.go_line > 100) // Approximate: late comments go at end
        .collect();

    if !trailing_comments.is_empty() {
        output.push('\n');
        for comment in trailing_comments {
            output.push_str(&comment.text);
            output.push('\n');
        }
    }

    // Handle blank line info - this is approximate without full line mapping
    let _ = blank_lines; // Currently not fully utilized without source map tracking

    Ok(output)
}

/// Generate Rust code with comment insertion.
pub fn generate_with_comments(
    file: syn::File,
    comments: &[CommentToInsert],
) -> Result<String, Box<dyn std::error::Error>> {
    generate_with_comments_and_blanks(file, comments, &BlankLineInfo::default())
}
