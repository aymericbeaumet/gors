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

/// Information about a function in the Rust output.
#[derive(Debug, Clone)]
struct RustFunction {
    /// Line index where the function starts (fn keyword)
    start_line: usize,
    /// Line index where the function ends (closing brace)
    end_line: usize,
    /// Estimated Go line where this function starts
    estimated_go_start: u32,
}

/// Find function boundaries in Rust code.
fn find_rust_functions(lines: &[&str]) -> Vec<RustFunction> {
    let mut functions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            let start_line = i;
            // Find the closing brace by tracking brace depth
            let mut depth = 0;
            let mut found_open = false;

            for (j, line) in lines.iter().enumerate().skip(i) {
                for ch in line.chars() {
                    if ch == '{' {
                        depth += 1;
                        found_open = true;
                    } else if ch == '}' {
                        depth -= 1;
                        if found_open && depth == 0 {
                            functions.push(RustFunction {
                                start_line,
                                end_line: j,
                                estimated_go_start: 0, // Will be set later
                            });
                            i = j;
                            break;
                        }
                    }
                }
                if found_open && depth == 0 {
                    break;
                }
            }
        }
        i += 1;
    }

    // Estimate Go line numbers for each function
    // Heuristic: first function starts around line 5 (after package + import)
    // Each subsequent function starts roughly (total_lines / num_functions) lines apart
    if !functions.is_empty() {
        let base_go_line: u32 = 5; // Typical start for first function
        let spacing: u32 = 10; // Estimated lines between function starts

        for (idx, func) in functions.iter_mut().enumerate() {
            func.estimated_go_start = base_go_line + (idx as u32 * spacing);
        }
    }

    functions
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

    let mut output = String::new();
    let rust_lines: Vec<&str> = formatted.lines().collect();

    // Sort comments by line number
    let mut sorted_comments: Vec<_> = comments.iter().filter(|c| !c.is_doc).collect();
    sorted_comments.sort_by_key(|c| c.go_line);

    // Partition leading comments (before package/first code - typically line <= 2)
    let (leading_comments, other_comments): (Vec<_>, Vec<_>) =
        sorted_comments.into_iter().partition(|c| c.go_line <= 2);

    // Insert leading comments at the top
    for comment in &leading_comments {
        output.push_str(&comment.text);
        output.push('\n');
    }

    let has_leading_comments = !leading_comments.is_empty();

    // Find function boundaries in the Rust code
    let functions = find_rust_functions(&rust_lines);

    // Categorize comments:
    // - before_fn: comments that appear before a function definition
    // - inside_fn: comments that appear inside a function body
    let mut comments_before_fn: std::collections::HashMap<usize, Vec<&CommentToInsert>> =
        std::collections::HashMap::new();
    let mut comments_inside_fn: std::collections::HashMap<usize, Vec<&CommentToInsert>> =
        std::collections::HashMap::new();
    let mut trailing_comments: Vec<&CommentToInsert> = Vec::new();

    // Heuristic for Go line ranges:
    // - Package declaration: line 1
    // - Import: lines 2-4
    // - First function starts around line 5
    // - A function definition line in Go (func xxx) is typically 1-2 lines
    // - Comments after the function signature but before closing brace are inside

    let first_fn_go_line: u32 = 5; // Typical start line for first function in Go

    for comment in other_comments {
        if functions.is_empty() {
            trailing_comments.push(comment);
            continue;
        }

        // Find which function this comment belongs to
        let mut placed = false;

        for (fn_idx, func) in functions.iter().enumerate() {
            let fn_go_start = func.estimated_go_start;
            let next_fn_go_start = functions
                .get(fn_idx + 1)
                .map(|f| f.estimated_go_start)
                .unwrap_or(u32::MAX);

            // Comment is before the first function
            if comment.go_line < first_fn_go_line && fn_idx == 0 {
                comments_before_fn
                    .entry(func.start_line)
                    .or_default()
                    .push(comment);
                placed = true;
                break;
            }

            // Comment is a doc comment (right before function definition, within 2 lines)
            if comment.go_line >= fn_go_start.saturating_sub(2) && comment.go_line < fn_go_start {
                comments_before_fn
                    .entry(func.start_line)
                    .or_default()
                    .push(comment);
                placed = true;
                break;
            }

            // Comment is inside this function (after function start, before next function)
            if comment.go_line >= fn_go_start && comment.go_line < next_fn_go_start {
                comments_inside_fn
                    .entry(fn_idx)
                    .or_default()
                    .push(comment);
                placed = true;
                break;
            }
        }

        if !placed {
            trailing_comments.push(comment);
        }
    }

    // Output Rust code with comments inserted
    for (i, line) in rust_lines.iter().enumerate() {
        // Add extra newline before function definitions or after leading comments
        let is_fn_start = line.trim().starts_with("fn ") || line.trim().starts_with("pub fn ");
        let needs_blank_before =
            (i > 0 && is_fn_start) || (i == 0 && has_leading_comments && !line.is_empty());

        if needs_blank_before {
            output.push('\n');
        }

        // Insert comments that belong before this line (doc comments for functions)
        if let Some(comments_for_line) = comments_before_fn.get(&i) {
            for comment in comments_for_line {
                output.push_str(&comment.text);
                output.push('\n');
            }
        }

        // Check if this is a function's closing brace - insert inline comments before it
        for (fn_idx, func) in functions.iter().enumerate() {
            if i == func.end_line {
                if let Some(inline_comments) = comments_inside_fn.get(&fn_idx) {
                    // Get the indentation of the closing brace
                    let indent = line.len() - line.trim_start().len();
                    let indent_str: String = " ".repeat(indent + 4); // Add extra indent for inside body

                    for comment in inline_comments {
                        output.push_str(&indent_str);
                        output.push_str(&comment.text);
                        output.push('\n');
                    }
                }
            }
        }

        output.push_str(line);
        output.push('\n');
    }

    // Add trailing comments at the end
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
