use gors::error::{Diagnostic, DiagnosticKind};
use gors::mapping::SourceMap;
use wasm_bindgen::prelude::*;

/// Result of a build operation.
#[wasm_bindgen]
pub struct BuildResult {
    success: bool,
    /// Output text: Rust source
    output: String,
    error_message: String,
    error_file: String,
    error_line: u32,
    error_column: u32,
    error_end_column: u32,
    error_kind: String,
    error_source_line: String,
    /// Source mappings in standard v3 format
    source_map: Option<SourceMap>,
}

#[wasm_bindgen]
impl BuildResult {
    #[wasm_bindgen(getter)]
    pub fn success(&self) -> bool {
        self.success
    }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> String {
        self.output.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_message(&self) -> String {
        self.error_message.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_file(&self) -> String {
        self.error_file.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_line(&self) -> u32 {
        self.error_line
    }

    #[wasm_bindgen(getter)]
    pub fn error_column(&self) -> u32 {
        self.error_column
    }

    #[wasm_bindgen(getter)]
    pub fn error_end_column(&self) -> u32 {
        self.error_end_column
    }

    #[wasm_bindgen(getter)]
    pub fn error_kind(&self) -> String {
        self.error_kind.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_source_line(&self) -> String {
        self.error_source_line.clone()
    }

    /// Get the source map as a JSON string in standard v3 format.
    #[wasm_bindgen]
    pub fn get_source_map_json(&self) -> String {
        self.source_map
            .as_ref()
            .map(|sm| {
                let mut buf = Vec::new();
                sm.to_writer(&mut buf).ok();
                String::from_utf8(buf).unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// Look up original (Go) position for a generated (Rust) position.
    /// Returns [orig_line, orig_col] (0-based) or empty array if not found.
    #[wasm_bindgen]
    pub fn lookup_token(&self, gen_line: u32, gen_col: u32) -> Vec<u32> {
        self.source_map
            .as_ref()
            .and_then(|sm| sm.lookup_token(gen_line, gen_col))
            .map(|token| vec![token.get_src_line(), token.get_src_col()])
            .unwrap_or_default()
    }

    /// Map output position to Go position (returns span: [start_line, start_col, end_line, end_col]).
    /// Lines and columns are 1-based for Monaco editor compatibility.
    #[wasm_bindgen]
    pub fn output_to_go(&self, output_line: u32, output_column: u32) -> Vec<u32> {
        let Some(ref sm) = self.source_map else {
            return vec![];
        };

        // Convert to 0-based indices
        let target_line = output_line.saturating_sub(1);
        let target_col = output_column.saturating_sub(1);

        // Find the best matching token on the same output line
        // Strategy: prefer token at/before cursor, but accept closest token on line
        let mut best_token = None;
        let mut best_distance = u32::MAX;
        let mut best_is_before_cursor = false;

        for i in 0..sm.get_token_count() {
            if let Some(token) = sm.get_token(i as usize) {
                let dst_line = token.get_dst_line();
                let dst_col = token.get_dst_col();

                // Match tokens on the same generated line
                if dst_line == target_line {
                    let distance = target_col.abs_diff(dst_col);
                    let is_before_cursor = dst_col <= target_col;

                    // Prefer tokens at/before cursor over tokens after cursor
                    // Among same preference, pick the closest one
                    let dominated = match (best_is_before_cursor, is_before_cursor) {
                        (false, true) => true, // New token is before cursor, old wasn't
                        (true, false) => false, // Old token is before cursor, new isn't
                        _ => distance < best_distance, // Same preference, pick closer
                    };

                    if dominated {
                        best_distance = distance;
                        best_token = Some(token);
                        best_is_before_cursor = is_before_cursor;
                    }
                }
            }
        }

        let Some(token) = best_token else {
            return vec![];
        };

        // Convert to 1-based for Monaco
        let start_line = token.get_src_line() + 1;
        let start_col = token.get_src_col() + 1;

        // The stored name is the Go token name - use its length directly
        let name_len = token.get_name().map(|n| n.len() as u32).unwrap_or(1);
        let end_line = start_line;
        let end_col = start_col + name_len;

        vec![start_line, start_col, end_line, end_col]
    }

    /// Map Rust position to Go position (returns span: [start_line, start_col, end_line, end_col]).
    /// Alias for output_to_go for backward compatibility.
    /// Lines and columns are 1-based for Monaco editor compatibility.
    #[wasm_bindgen]
    pub fn rust_to_go(&self, rust_line: u32, rust_column: u32) -> Vec<u32> {
        self.output_to_go(rust_line, rust_column)
    }

    /// Map Go position to output position (returns span: [start_line, start_col, end_line, end_col]).
    /// Lines and columns are 1-based for Monaco editor compatibility.
    #[wasm_bindgen]
    pub fn go_to_output(&self, go_line: u32, go_column: u32) -> Vec<u32> {
        let Some(ref sm) = self.source_map else {
            return vec![];
        };

        // Convert to 0-based for comparison
        let target_line = go_line.saturating_sub(1);
        let target_col = go_column.saturating_sub(1);

        // Find the token that best matches the Go position
        // Strategy: prefer token at/before cursor, but accept closest token on line
        let mut best_token = None;
        let mut best_distance = u32::MAX;
        let mut best_is_before_cursor = false;

        for i in 0..sm.get_token_count() {
            if let Some(token) = sm.get_token(i as usize) {
                let src_line = token.get_src_line();
                let src_col = token.get_src_col();

                // Exact line match
                if src_line == target_line {
                    let distance = target_col.abs_diff(src_col);
                    let is_before_cursor = src_col <= target_col;

                    // Prefer tokens at/before cursor over tokens after cursor
                    // Among same preference, pick the closest one
                    let dominated = match (best_is_before_cursor, is_before_cursor) {
                        (false, true) => true, // New token is before cursor, old wasn't
                        (true, false) => false, // Old token is before cursor, new isn't
                        _ => distance < best_distance, // Same preference, pick closer
                    };

                    if dominated {
                        best_distance = distance;
                        best_token = Some(token);
                        best_is_before_cursor = is_before_cursor;
                    }
                }
            }
        }

        let Some(token) = best_token else {
            return vec![];
        };

        // Convert generated position to 1-based for Monaco
        let dst_line = token.get_dst_line();
        let dst_col = token.get_dst_col();
        let start_line = dst_line + 1;
        let start_col = dst_col + 1;

        // Extract the actual Rust token at the destination position to get its length
        // This avoids hardcoding length mappings between Go and Rust tokens
        let name_len = extract_rust_token_at(&self.output, dst_line, dst_col)
            .map(|t| t.len() as u32)
            .unwrap_or(1);
        let end_line = start_line;
        let end_col = start_col + name_len;

        vec![start_line, start_col, end_line, end_col]
    }

    /// Map Go position to Rust position (returns span: [start_line, start_col, end_line, end_col]).
    /// Alias for go_to_output for backward compatibility.
    /// Lines and columns are 1-based for Monaco editor compatibility.
    #[wasm_bindgen]
    pub fn go_to_rust(&self, go_line: u32, go_column: u32) -> Vec<u32> {
        self.go_to_output(go_line, go_column)
    }

    /// Get the number of tokens/mappings.
    #[wasm_bindgen]
    pub fn mapping_count(&self) -> u32 {
        self.source_map
            .as_ref()
            .map(|sm| sm.get_token_count())
            .unwrap_or(0)
    }
}

impl BuildResult {
    fn success_rust(output: String, source_map: SourceMap) -> Self {
        Self {
            success: true,
            output,
            error_message: String::new(),
            error_file: String::new(),
            error_line: 0,
            error_column: 0,
            error_end_column: 0,
            error_kind: String::new(),
            error_source_line: String::new(),
            source_map: Some(source_map),
        }
    }

    fn error_result(diagnostic: Diagnostic) -> Self {
        Self {
            success: false,
            output: String::new(),
            error_message: diagnostic.message.clone(),
            error_file: diagnostic.file.clone(),
            error_line: diagnostic.line as u32,
            error_column: diagnostic.column as u32,
            error_end_column: diagnostic.end_column as u32,
            error_kind: match diagnostic.kind {
                DiagnosticKind::Scanner => "scanner".to_string(),
                DiagnosticKind::Parser => "parser".to_string(),
                DiagnosticKind::Compiler => "compiler".to_string(),
            },
            error_source_line: diagnostic.source_line.unwrap_or_default(),
            source_map: None,
        }
    }
}

/// Extract the token at a given position from Rust source code.
/// Returns the token text if found.
fn extract_rust_token_at(rust_source: &str, line: u32, col: u32) -> Option<String> {
    let lines: Vec<&str> = rust_source.lines().collect();
    let line_idx = line as usize;
    if line_idx >= lines.len() {
        return None;
    }
    
    let line_text = lines[line_idx];
    let col_idx = col as usize;
    if col_idx >= line_text.len() {
        return None;
    }
    
    let chars: Vec<char> = line_text.chars().collect();
    if col_idx >= chars.len() {
        return None;
    }
    
    let start_char = chars[col_idx];
    
    // Check for comment
    if col_idx + 1 < chars.len() && start_char == '/' && (chars[col_idx + 1] == '/' || chars[col_idx + 1] == '*') {
        // Return the rest of the line for line comments, or find end for block comments
        if chars[col_idx + 1] == '/' {
            return Some(line_text[col_idx..].to_string());
        }
    }
    
    // Check for identifier/keyword
    if start_char.is_alphabetic() || start_char == '_' {
        let mut end = col_idx;
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        // Handle macro invocation (e.g., println!)
        if end < chars.len() && chars[end] == '!' {
            end += 1;
        }
        return Some(chars[col_idx..end].iter().collect());
    }
    
    // For other tokens (operators, etc.), return single char
    Some(start_char.to_string())
}

/// Build Go source code and return Rust code with structured error information.
/// This is an alias for build_rust() for backward compatibility.
#[wasm_bindgen]
pub fn build(input: String) -> BuildResult {
    build_rust(input)
}

/// Build Go source code and return Rust code with structured error information.
#[wasm_bindgen]
pub fn build_rust(input: String) -> BuildResult {
    console_error_panic_hook::set_once();

    // Parse
    let ast = match gors::parser::parse_file("main.go", &input) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, "main.go", &input);
            return BuildResult::error_result(diagnostic);
        }
    };

    // Collect all comments from the AST
    // Mark doc comments (those attached to function declarations) as already handled
    let mut comments: Vec<CommentInfo> = Vec::new();
    let mut doc_comment_lines: std::collections::HashSet<u32> = std::collections::HashSet::new();

    // Find doc comment lines (comments attached to function declarations)
    for decl in &ast.decls {
        if let gors::ast::Decl::FuncDecl(func_decl) = decl {
            if let Some(ref doc) = func_decl.doc {
                for comment in &doc.list {
                    doc_comment_lines.insert(comment.slash.line as u32);
                }
            }
        }
    }

    // Collect all comments with their positions
    for comment_group in &ast.comments {
        for comment in &comment_group.list {
            let is_doc = doc_comment_lines.contains(&(comment.slash.line as u32));
            comments.push(CommentInfo {
                go_line: comment.slash.line as u32,
                go_col: comment.slash.column.saturating_sub(1) as u32, // Convert to 0-based
                text: comment.text.to_string(),
                is_doc,
            });
        }
    }

    // Compile with source map tracking
    let compiled = match gors::compiler::compile_with_source_map(ast, "main.go", &input) {
        Ok(result) => result,
        Err(err) => {
            let diagnostic = Diagnostic::new(
                "main.go",
                0,
                0,
                err.to_string(),
                DiagnosticKind::Compiler,
            );
            return BuildResult::error_result(diagnostic);
        }
    };

    // Generate Rust code WITHOUT comments first
    let rust_code = match gors::backend_rust::generate(compiled) {
        Ok(output) => output,
        Err(err) => {
            let diagnostic = Diagnostic::new(
                "main.go",
                0,
                0,
                err.to_string(),
                DiagnosticKind::Compiler,
            );
            return BuildResult::error_result(diagnostic);
        }
    };

    // Build the initial source map from the generated code
    let initial_source_map = gors::compiler::build_source_map(&rust_code);

    // Insert comments at exact positions using the source map
    // This also returns the final positions of comments for source map tracking
    let (output, comment_mappings) = insert_comments_with_sourcemap(&rust_code, &comments, &initial_source_map);

    // Build final source map that includes both code and comment mappings
    let source_map = build_source_map_with_comments(&output, &initial_source_map, &comment_mappings);

    BuildResult::success_rust(output, source_map)
}

/// A mapping for a comment's position in both Go and Rust
struct CommentMapping {
    go_line: u32,      // 0-based
    go_col: u32,       // 0-based, position of // or /* in Go source
    rust_line: u32,    // 0-based (final position in output)
    rust_col: u32,     // 0-based
    /// The original Rust line this comment was inserted before (0-based)
    /// Used to calculate line shifts for code mappings
    inserted_before_original_line: u32,
    /// The comment text
    text: String,
}

/// Build a source map that includes both code mappings and comment mappings.
/// Adjusts code mapping line numbers to account for inserted comment lines.
fn build_source_map_with_comments(
    _rust_code: &str,
    initial_source_map: &SourceMap,
    comment_mappings: &[CommentMapping],
) -> SourceMap {
    use gors::mapping::SourceMapBuilder;

    let mut builder = SourceMapBuilder::new(Some("output.rs"));
    let src_idx = builder.add_source("main.go");

    // Copy source content if available
    if let Some(content) = initial_source_map.get_source_contents(0) {
        builder.set_source_contents(src_idx, Some(content));
    }

    // Build a list of which original lines have comments inserted before them
    // Each comment was inserted before `inserted_before_original_line`
    let mut comments_before_line: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();
    for mapping in comment_mappings {
        *comments_before_line
            .entry(mapping.inserted_before_original_line)
            .or_insert(0) += 1;
    }

    // Calculate cumulative shift for each original line
    // shift[L] = total number of comments inserted before lines 0..=L
    let max_original_line = initial_source_map
        .tokens()
        .map(|t| t.get_dst_line())
        .max()
        .unwrap_or(0);

    let mut cumulative_shift = vec![0u32; (max_original_line + 2) as usize];
    let mut running_shift = 0u32;
    for line in 0..=max_original_line + 1 {
        // Add comments inserted before this line
        running_shift += comments_before_line.get(&line).copied().unwrap_or(0);
        cumulative_shift[line as usize] = running_shift;
    }

    // Add all existing code mappings with adjusted line numbers
    for i in 0..initial_source_map.get_token_count() {
        if let Some(token) = initial_source_map.get_token(i as usize) {
            let original_dst_line = token.get_dst_line();

            // Get the shift for this line (comments inserted before it)
            let shift = if (original_dst_line as usize) < cumulative_shift.len() {
                cumulative_shift[original_dst_line as usize]
            } else {
                *cumulative_shift.last().unwrap_or(&0)
            };

            let new_dst_line = original_dst_line + shift;

            let name_idx = token.get_name().map(|n| builder.add_name(n));
            builder.add_raw(
                new_dst_line,
                token.get_dst_col(),
                token.get_src_line(),
                token.get_src_col(),
                Some(src_idx),
                name_idx,
                false,
            );
        }
    }

    // Add comment mappings (already have correct final positions)
    for mapping in comment_mappings {
        // Store the full comment text as the name (used for span length calculation)
        let name_idx = builder.add_name(&mapping.text);

        builder.add_raw(
            mapping.rust_line,
            mapping.rust_col,
            mapping.go_line,
            mapping.go_col,
            Some(src_idx),
            Some(name_idx),
            false,
        );
    }

    builder.into_sourcemap()
}

/// Information about a comment to insert.
struct CommentInfo {
    go_line: u32,
    go_col: u32,
    text: String,
    is_doc: bool,
}

/// Insert comments into Rust code using source map for exact placement.
/// Returns the output string and a list of comment position mappings.
fn insert_comments_with_sourcemap(
    rust_code: &str,
    comments: &[CommentInfo],
    source_map: &SourceMap,
) -> (String, Vec<CommentMapping>) {
    let rust_lines: Vec<&str> = rust_code.lines().collect();
    let mut comment_mappings: Vec<CommentMapping> = Vec::new();

    // Build a mapping from Go line -> Rust line using all source map tokens
    let mut go_to_rust_line: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();

    for i in 0..source_map.get_token_count() {
        if let Some(token) = source_map.get_token(i as usize) {
            let go_line = token.get_src_line(); // 0-based
            let rust_line = token.get_dst_line(); // 0-based
            // Keep the smallest Rust line for each Go line (first occurrence)
            go_to_rust_line
                .entry(go_line)
                .and_modify(|existing| {
                    if rust_line < *existing {
                        *existing = rust_line;
                    }
                })
                .or_insert(rust_line);
        }
    }

    // Find the maximum Go line that has a mapping (for detecting trailing comments)
    let max_mapped_go_line = go_to_rust_line.keys().copied().max().unwrap_or(0);

    // For each comment, find the Rust line where it should be inserted
    // A comment on Go line N should appear before the code on Go line N+1 (or later)
    // Note: go_line is 1-based, but source map uses 0-based line numbers
    let mut comments_by_rust_line: std::collections::HashMap<u32, Vec<&CommentInfo>> =
        std::collections::HashMap::new();
    let mut leading_comments: Vec<&CommentInfo> = Vec::new();
    let mut trailing_comments: Vec<&CommentInfo> = Vec::new();

    for comment in comments {
        if comment.is_doc {
            // Doc comments are already handled by the compiler
            continue;
        }

        // Convert to 0-based for source map lookup
        let comment_go_line_0based = comment.go_line.saturating_sub(1);

        // Find the Rust line that corresponds to the Go line of or after this comment
        // For inline comments (same line as code), we insert before that line
        // For standalone comments, we insert before the next code line
        let mut target_rust_line: Option<u32> = None;

        // First check if there's code on the same line as the comment (inline comment)
        if let Some(&rust_line) = go_to_rust_line.get(&comment_go_line_0based) {
            // Inline comment - insert before the code line it's associated with
            target_rust_line = Some(rust_line);
        } else {
            // Standalone comment - find the next line with code
            for next_go_line_0based in (comment_go_line_0based + 1)..comment_go_line_0based + 20 {
                if let Some(&rust_line) = go_to_rust_line.get(&next_go_line_0based) {
                    target_rust_line = Some(rust_line);
                    break;
                }
            }
        }

        if let Some(rust_line) = target_rust_line {
            comments_by_rust_line
                .entry(rust_line)
                .or_default()
                .push(comment);
        } else if comment.go_line <= 2 {
            // Leading comment (before package declaration)
            leading_comments.push(comment);
        } else if comment_go_line_0based > max_mapped_go_line {
            // Trailing comment (after all mapped code)
            trailing_comments.push(comment);
        }
        // If we still can't place the comment, it's dropped
    }

    // Insert trailing comments before the last closing brace
    if !trailing_comments.is_empty() {
        // Find the line with the last closing brace
        let closing_brace_line = rust_lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.trim() == "}")
            .map(|(i, _)| i as u32);

        if let Some(brace_line) = closing_brace_line {
            for comment in trailing_comments {
                comments_by_rust_line
                    .entry(brace_line)
                    .or_default()
                    .push(comment);
            }
        }
    }

    // Build the output with comments inserted, tracking line numbers
    let mut output = String::new();
    let mut current_rust_line: u32 = 0; // 0-based line number in output

    // Insert leading comments (inserted before line 0)
    for comment in &leading_comments {
        let indent = 0usize;
        // Calculate Go column from the comment text (find where // or /* starts)
        let go_col = comment.go_col;
        comment_mappings.push(CommentMapping {
            go_line: comment.go_line.saturating_sub(1), // Convert to 0-based
            go_col,
            rust_line: current_rust_line,
            rust_col: indent as u32,
            inserted_before_original_line: 0, // Inserted before the first line
            text: comment.text.clone(),
        });
        output.push_str(&comment.text);
        output.push('\n');
        current_rust_line += 1;
    }
    if !leading_comments.is_empty() && !rust_lines.is_empty() {
        output.push('\n');
        current_rust_line += 1;
    }

    for (i, line) in rust_lines.iter().enumerate() {
        let rust_line_idx = i as u32;

        // Insert any comments that should appear before this line
        if let Some(line_comments) = comments_by_rust_line.get(&rust_line_idx) {
            // Determine indentation from the current line
            // If the current line is just a closing brace, look at the previous line's indentation
            let indent = if line.trim() == "}" {
                // Use default indentation (4 spaces) for content inside braces
                4
            } else {
                line.len() - line.trim_start().len()
            };
            let indent_str: String = " ".repeat(indent);

            for comment in line_comments {
                // Preserve the Go column position from the original source
                let go_col = comment.go_col;
                comment_mappings.push(CommentMapping {
                    go_line: comment.go_line.saturating_sub(1), // Convert to 0-based
                    go_col,
                    rust_line: current_rust_line,
                    rust_col: indent as u32,
                    inserted_before_original_line: rust_line_idx, // Track which original line
                    text: comment.text.clone(),
                });
                output.push_str(&indent_str);
                output.push_str(&comment.text);
                output.push('\n');
                current_rust_line += 1;
            }
        }

        output.push_str(line);
        output.push('\n');
        current_rust_line += 1;
    }

    (output, comment_mappings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to build Go code and return output (for testing without wasm_bindgen)
    fn build_go(input: &str) -> Result<String, String> {
        let ast = gors::parser::parse_file("main.go", input)
            .map_err(|e| format!("Parse error: {:?}", e))?;

        // Collect comments
        let mut comments: Vec<CommentInfo> = Vec::new();
        let mut doc_comment_lines: std::collections::HashSet<u32> = std::collections::HashSet::new();

        for decl in &ast.decls {
            if let gors::ast::Decl::FuncDecl(func_decl) = decl {
                if let Some(ref doc) = func_decl.doc {
                    for comment in &doc.list {
                        doc_comment_lines.insert(comment.slash.line as u32);
                    }
                }
            }
        }

        for comment_group in &ast.comments {
            for comment in &comment_group.list {
                let is_doc = doc_comment_lines.contains(&(comment.slash.line as u32));
                comments.push(CommentInfo {
                    go_line: comment.slash.line as u32,
                    go_col: comment.slash.column.saturating_sub(1) as u32, // Convert to 0-based
                    text: comment.text.to_string(),
                    is_doc,
                });
            }
        }

        let compiled = gors::compiler::compile_with_source_map(ast, "main.go", input)
            .map_err(|e| format!("Compile error: {:?}", e))?;

        let rust_code = gors::backend_rust::generate(compiled)
            .map_err(|e| format!("Codegen error: {:?}", e))?;

        let source_map = gors::compiler::build_source_map(&rust_code);
        let (output, _comment_mappings) = insert_comments_with_sourcemap(&rust_code, &comments, &source_map);

        Ok(output)
    }
}

/// Result of compiling Go to WebAssembly.
#[wasm_bindgen]
pub struct WasmBuildResult {
    success: bool,
    /// WASM binary (empty on error)
    wasm_bytes: Vec<u8>,
    error_message: String,
}

#[wasm_bindgen]
impl WasmBuildResult {
    #[wasm_bindgen(getter)]
    pub fn success(&self) -> bool {
        self.success
    }

    #[wasm_bindgen(getter)]
    pub fn wasm_bytes(&self) -> Vec<u8> {
        self.wasm_bytes.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_message(&self) -> String {
        self.error_message.clone()
    }
}

/// Compile Go source code directly to WebAssembly bytecode.
///
/// This function parses Go code, transpiles it to Rust AST, and then
/// compiles it directly to WASM using the Walrus library.
/// No external Rust toolchain is required.
#[wasm_bindgen]
pub fn compile_to_wasm(input: String) -> WasmBuildResult {
    console_error_panic_hook::set_once();

    // Parse Go source
    let ast = match gors::parser::parse_file("main.go", &input) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, "main.go", &input);
            return WasmBuildResult {
                success: false,
                wasm_bytes: vec![],
                error_message: diagnostic.message,
            };
        }
    };

    // Compile to Rust AST
    let compiled = match gors::compiler::compile(ast) {
        Ok(compiled) => compiled,
        Err(err) => {
            return WasmBuildResult {
                success: false,
                wasm_bytes: vec![],
                error_message: format!("Compiler error: {err}"),
            };
        }
    };

    // Compile Rust AST to WASM
    match gors::backend_wasm::compile_to_wasm(&compiled) {
        Ok(wasm_bytes) => WasmBuildResult {
            success: true,
            wasm_bytes,
            error_message: String::new(),
        },
        Err(err) => WasmBuildResult {
            success: false,
            wasm_bytes: vec![],
            error_message: err.to_string(),
        },
    }
}

/// Result of running Go code via WASM.
#[wasm_bindgen]
pub struct RunResult {
    success: bool,
    output: String,
    error_message: String,
}

#[wasm_bindgen]
impl RunResult {
    #[wasm_bindgen(getter)]
    pub fn success(&self) -> bool {
        self.success
    }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> String {
        self.output.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_message(&self) -> String {
        self.error_message.clone()
    }
}

impl RunResult {
    fn success_with_output(output: String) -> Self {
        Self {
            success: true,
            output,
            error_message: String::new(),
        }
    }

    fn error(message: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error_message: message,
        }
    }
}

/// Compile and run Go source code.
///
/// This function compiles Go code to WASM and executes it using the wasmi
/// interpreter. It captures output from print_i32 calls and returns the result.
/// This works both natively and when gors itself is compiled to WASM.
///
/// # Arguments
///
/// * `input` - Go source code to compile and run
///
/// # Returns
///
/// Returns a `RunResult` with:
/// - `success`: true if compilation and execution succeeded
/// - `output`: captured output from the program (print_i32 calls, newline-separated)
/// - `error_message`: error description if compilation or execution failed
///
/// # Example
///
/// ```javascript
/// const result = run_go(`
///     package main
///     
///     func main() {
///         print_i32(42)
///     }
/// `);
/// if (result.success) {
///     console.log(result.output); // "42"
/// } else {
///     console.error(result.error_message);
/// }
/// ```
#[wasm_bindgen]
pub fn run_go(input: String) -> RunResult {
    console_error_panic_hook::set_once();

    // First compile to WASM
    let wasm_result = compile_to_wasm(input);
    if !wasm_result.success {
        return RunResult::error(wasm_result.error_message);
    }

    // Execute the WASM using wasmi
    match execute_wasm_with_wasmi(&wasm_result.wasm_bytes) {
        Ok(output) => RunResult::success_with_output(output),
        Err(e) => RunResult::error(format!("Execution error: {e}")),
    }
}

/// Execute WASM bytes using the wasmi interpreter.
/// This works both natively and when gors itself is compiled to WASM.
fn execute_wasm_with_wasmi(wasm_bytes: &[u8]) -> Result<String, String> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasmi::{Caller, Engine, Extern, Func, Linker, Module, Store};

    // Create output collector
    let output: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // Create engine and store with the output collector as state
    let engine = Engine::default();
    let output_clone = Rc::clone(&output);
    let mut store = Store::new(&engine, output_clone);

    // Compile the WASM module
    let module =
        Module::new(&engine, wasm_bytes).map_err(|e| format!("Failed to compile WASM: {e}"))?;

    // Create linker and add import functions
    let mut linker = Linker::new(&engine);

    // print_i32 function that captures output to the store's state
    linker
        .func_wrap(
            "env",
            "print_i32",
            |caller: Caller<'_, Rc<RefCell<Vec<String>>>>, value: i32| {
                caller.data().borrow_mut().push(value.to_string());
            },
        )
        .map_err(|e| format!("Failed to add print_i32: {e}"))?;

    // print_str function that reads a string from memory and outputs it
    linker
        .func_wrap(
            "env",
            "print_str",
            |caller: Caller<'_, Rc<RefCell<Vec<String>>>>, ptr: i32, len: i32| {
                // Get memory from the caller's instance
                if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                    let mut buffer = vec![0u8; len as usize];
                    if memory.read(&caller, ptr as usize, &mut buffer).is_ok() {
                        if let Ok(s) = String::from_utf8(buffer) {
                            caller.data().borrow_mut().push(s);
                        }
                    }
                }
            },
        )
        .map_err(|e| format!("Failed to add print_str: {e}"))?;

    // Instantiate the module
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("Failed to instantiate: {e}"))?
        .start(&mut store)
        .map_err(|e| format!("Failed to start: {e}"))?;

    // Get and call the main function
    let main_func: Func = instance
        .get_export(&store, "main")
        .and_then(|e| e.into_func())
        .ok_or("main function not found")?;

    main_func
        .call(&mut store, &[], &mut [])
        .map_err(|e| format!("main() execution failed: {e}"))?;

    // Collect output
    let output_vec = output.borrow();
    Ok(output_vec.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_between_statements() {
        let go_code = r#"package main

func main() {
	a
	// comment between a and b
	b
}
"#;
        let output = build_go(go_code).unwrap();

        // Verify comment appears between a and b
        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines.iter().position(|l| l.contains("a;")).unwrap();
        let comment_line = lines.iter().position(|l| l.contains("// comment between a and b")).unwrap();
        let b_line = lines.iter().position(|l| l.contains("b;")).unwrap();

        assert!(a_line < comment_line, "Comment should be after 'a'");
        assert!(comment_line < b_line, "Comment should be before 'b'");
    }

    #[test]
    fn test_multiple_comments_in_order() {
        let go_code = r#"package main

func main() {
	// comment 1
	a
	// comment 2
	b
	// comment 3
	c
}
"#;
        let output = build_go(go_code).unwrap();

        // Verify comments appear in order
        let pos1 = output.find("// comment 1").expect("comment 1 should exist");
        let pos2 = output.find("// comment 2").expect("comment 2 should exist");
        let pos3 = output.find("// comment 3").expect("comment 3 should exist");

        assert!(pos1 < pos2, "comment 1 should come before comment 2");
        assert!(pos2 < pos3, "comment 2 should come before comment 3");

        // Verify comments are before their respective statements
        let pos_a = output.find("a;").expect("a should exist");
        let pos_b = output.find("b;").expect("b should exist");
        let pos_c = output.find("c;").expect("c should exist");

        assert!(pos1 < pos_a, "comment 1 should be before a");
        assert!(pos2 < pos_b, "comment 2 should be before b");
        assert!(pos3 < pos_c, "comment 3 should be before c");
    }

    #[test]
    fn test_trailing_comment_before_closing_brace() {
        let go_code = r#"package main

func main() {
	a
	// trailing comment
}
"#;
        let output = build_go(go_code).unwrap();

        // Verify trailing comment appears before the closing brace
        let comment_pos = output.find("// trailing comment").expect("trailing comment should exist");
        let brace_pos = output.rfind('}').expect("closing brace should exist");

        assert!(comment_pos < brace_pos, "Trailing comment should be before closing brace");

        // Verify indentation (should be 4 spaces)
        let lines: Vec<&str> = output.lines().collect();
        let comment_line = lines.iter().find(|l| l.contains("// trailing comment")).unwrap();
        assert!(comment_line.starts_with("    "), "Trailing comment should be indented");
    }

    #[test]
    fn test_comment_with_code_on_multiple_lines() {
        let go_code = r#"package main

import "fmt"

func main() {
	fmt.Println("Hello")

	first
	// middle comment
	second
}
"#;
        let output = build_go(go_code).unwrap();

        // Verify the comment is between first and second
        let first_pos = output.find("first;").expect("first should exist");
        let comment_pos = output.find("// middle comment").expect("middle comment should exist");
        let second_pos = output.find("second;").expect("second should exist");

        assert!(first_pos < comment_pos, "Comment should be after 'first'");
        assert!(comment_pos < second_pos, "Comment should be before 'second'");
    }

    #[test]
    fn test_comment_preserves_indentation() {
        let go_code = r#"package main

func main() {
	a
	// indented comment
	b
}
"#;
        let output = build_go(go_code).unwrap();

        // Verify comment has proper indentation (matching the code)
        let lines: Vec<&str> = output.lines().collect();
        let comment_line = lines.iter().find(|l| l.contains("// indented comment")).unwrap();
        let b_line = lines.iter().find(|l| l.contains("b;")).unwrap();

        let comment_indent = comment_line.len() - comment_line.trim_start().len();
        let b_indent = b_line.len() - b_line.trim_start().len();

        assert_eq!(comment_indent, b_indent, "Comment should have same indentation as code");
    }

    #[test]
    fn test_exact_placement_user_example() {
        // This is the exact example from the user's bug report
        let go_code = r#"package main

import "fmt"

func main() {
	fmt.Println("Hello, 世界")

	arosietnaotn
	// Start typing and see the changes!
	arstarstararsto
}
"#;
        let output = build_go(go_code).unwrap();

        // Verify exact placement: comment should be between arosietnaotn and arstarstararsto
        let first_pos = output.find("arosietnaotn;").expect("arosietnaotn should exist");
        let comment_pos = output.find("// Start typing and see the changes!").expect("comment should exist");
        let second_pos = output.find("arstarstararsto;").expect("arstarstararsto should exist");

        assert!(first_pos < comment_pos, "Comment should be after arosietnaotn");
        assert!(comment_pos < second_pos, "Comment should be before arstarstararsto");
    }

    #[test]
    fn test_doc_comments_not_duplicated() {
        // Doc comments are handled by the compiler as #[doc] attributes
        // They should not be duplicated by the comment insertion logic
        let go_code = r#"package main

// hello is a function
func hello() {
	a
}
"#;
        let output = build_go(go_code).unwrap();

        // Count occurrences of the doc comment text
        let count = output.matches("hello is a function").count();

        // Should appear exactly once (as a doc comment, not duplicated)
        assert!(count <= 1, "Doc comment should not be duplicated, found {} times", count);
    }

    #[test]
    fn test_comment_only_before_closing_brace() {
        // When there's code before a trailing comment, the comment should be preserved
        let go_code = r#"package main

func main() {
	a
	// comment before closing brace
}
"#;
        let output = build_go(go_code).unwrap();

        // The comment should appear before the closing brace
        assert!(output.contains("// comment before closing brace"), "Comment should be preserved");

        // Verify it's inside the function (before the closing brace)
        let comment_pos = output.find("// comment before closing brace").unwrap();
        let brace_pos = output.rfind('}').unwrap();
        assert!(comment_pos < brace_pos, "Comment should be before closing brace");
    }

    #[test]
    fn test_empty_function_body_limitation() {
        // Note: Comments in otherwise empty function bodies may be lost
        // because prettyplease formats them as `fn foo() {}` on one line.
        // This is a known limitation.
        let go_code = r#"package main

func main() {
	// only a comment in empty body
}
"#;
        let output = build_go(go_code).unwrap();

        // The function should compile, even if the comment is lost
        assert!(output.contains("fn main()"), "Function should exist");
        // We don't assert the comment exists because it may be lost in empty bodies
    }

    #[test]
    fn test_multiple_functions_with_comments() {
        let go_code = r#"package main

func foo() {
	// comment in foo
	a
}

func bar() {
	// comment in bar
	b
}
"#;
        let output = build_go(go_code).unwrap();

        // Both comments should exist
        assert!(output.contains("// comment in foo"), "Comment in foo should exist");
        assert!(output.contains("// comment in bar"), "Comment in bar should exist");

        // Verify order: foo's comment before bar's comment
        let foo_comment_pos = output.find("// comment in foo").unwrap();
        let bar_comment_pos = output.find("// comment in bar").unwrap();
        assert!(foo_comment_pos < bar_comment_pos, "foo's comment should come before bar's");
    }

    /// Helper to build and return the source map along with output
    fn build_with_sourcemap(input: &str) -> Result<(String, SourceMap), String> {
        let ast = gors::parser::parse_file("main.go", input)
            .map_err(|e| format!("Parse error: {:?}", e))?;

        let mut comments: Vec<CommentInfo> = Vec::new();
        let mut doc_comment_lines: std::collections::HashSet<u32> = std::collections::HashSet::new();

        for decl in &ast.decls {
            if let gors::ast::Decl::FuncDecl(func_decl) = decl {
                if let Some(ref doc) = func_decl.doc {
                    for comment in &doc.list {
                        doc_comment_lines.insert(comment.slash.line as u32);
                    }
                }
            }
        }

        for comment_group in &ast.comments {
            for comment in &comment_group.list {
                let is_doc = doc_comment_lines.contains(&(comment.slash.line as u32));
                comments.push(CommentInfo {
                    go_line: comment.slash.line as u32,
                    go_col: comment.slash.column.saturating_sub(1) as u32, // Convert to 0-based
                    text: comment.text.to_string(),
                    is_doc,
                });
            }
        }

        let compiled = gors::compiler::compile_with_source_map(ast, "main.go", input)
            .map_err(|e| format!("Compile error: {:?}", e))?;

        let rust_code = gors::backend_rust::generate(compiled)
            .map_err(|e| format!("Codegen error: {:?}", e))?;

        let initial_source_map = gors::compiler::build_source_map(&rust_code);
        let (output, comment_mappings) =
            insert_comments_with_sourcemap(&rust_code, &comments, &initial_source_map);
        let source_map =
            build_source_map_with_comments(&output, &initial_source_map, &comment_mappings);

        Ok((output, source_map))
    }

    #[test]
    fn test_comment_hover_go_to_rust() {
        // Test that hovering on a comment in Go highlights the correct position in Rust
        let go_code = r#"package main

func main() {
	a
	// my test comment
	b
}
"#;
        let (output, source_map) = build_with_sourcemap(go_code).unwrap();

        // Find which Rust line has the comment
        let rust_lines: Vec<&str> = output.lines().collect();
        let comment_rust_line = rust_lines
            .iter()
            .position(|l| l.contains("// my test comment"))
            .expect("Comment should be in output");

        // Go line 5 has the comment (1-based)
        let go_comment_line = 5u32;

        // Find the token for Go line 5 (0-based: 4)
        let mut found_comment_mapping = false;
        for i in 0..source_map.get_token_count() {
            if let Some(token) = source_map.get_token(i as usize) {
                if token.get_src_line() == go_comment_line - 1 {
                    // The Rust line should match where we found the comment
                    assert_eq!(
                        token.get_dst_line() as usize,
                        comment_rust_line,
                        "Comment should map to correct Rust line"
                    );
                    found_comment_mapping = true;
                    break;
                }
            }
        }
        assert!(found_comment_mapping, "Should find mapping for comment");
    }

    #[test]
    fn test_comment_hover_rust_to_go() {
        // Test that hovering on a comment in Rust highlights the correct position in Go
        let go_code = r#"package main

func main() {
	a
	// my test comment
	b
}
"#;
        let (output, source_map) = build_with_sourcemap(go_code).unwrap();

        // Find which Rust line has the comment
        let rust_lines: Vec<&str> = output.lines().collect();
        let comment_rust_line = rust_lines
            .iter()
            .position(|l| l.contains("// my test comment"))
            .expect("Comment should be in output") as u32;

        // Go line 5 has the comment (1-based), so 0-based is 4
        let expected_go_line = 4u32;

        // Find a token on the comment's Rust line
        let mut found = false;
        for i in 0..source_map.get_token_count() {
            if let Some(token) = source_map.get_token(i as usize) {
                if token.get_dst_line() == comment_rust_line {
                    assert_eq!(
                        token.get_src_line(),
                        expected_go_line,
                        "Rust comment line should map back to Go comment line"
                    );
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "Should find token on comment Rust line");
    }

    #[test]
    fn test_code_mappings_adjusted_after_comment_insertion() {
        // Test that code mappings are correctly adjusted when comments are inserted
        let go_code = r#"package main

func main() {
	a
	// comment shifts b down
	b
}
"#;
        let (output, source_map) = build_with_sourcemap(go_code).unwrap();

        // In the output, 'b' should be after the comment
        let rust_lines: Vec<&str> = output.lines().collect();
        let b_rust_line = rust_lines
            .iter()
            .position(|l| l.trim() == "b;")
            .expect("b should be in output") as u32;

        // Go line 6 has 'b' (1-based), so 0-based is 5
        let go_b_line = 5u32;

        // Find the mapping for 'b'
        let mut found_b_mapping = false;
        for i in 0..source_map.get_token_count() {
            if let Some(token) = source_map.get_token(i as usize) {
                if token.get_name() == Some("b") {
                    assert_eq!(
                        token.get_src_line(),
                        go_b_line,
                        "'b' should map to Go line 6 (0-based: 5)"
                    );
                    assert_eq!(
                        token.get_dst_line(),
                        b_rust_line,
                        "'b' should map to correct Rust line after comment insertion"
                    );
                    found_b_mapping = true;
                    break;
                }
            }
        }
        assert!(found_b_mapping, "Should find mapping for 'b'");
    }
}
