use gors::codegen::{BlankLineInfo, CommentToInsert};
use gors::error::{Diagnostic, DiagnosticKind};
use gors::mapping::SourceMap;
use wasm_bindgen::prelude::*;

/// Result of a build operation
#[wasm_bindgen]
pub struct BuildResult {
    success: bool,
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
    fn success_result(output: String, source_map: SourceMap) -> Self {
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

/// Build Go source code and return Rust code with structured error information
#[wasm_bindgen]
pub fn build(input: String) -> BuildResult {
    console_error_panic_hook::set_once();

    // Collect blank line information from the Go source
    let blank_lines = collect_blank_lines(&input);

    // Parse
    let ast = match gors::parser::parse_file("main.go", &input) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, "main.go", &input);
            return BuildResult::error_result(diagnostic);
        }
    };

    // Collect all comments from the AST for later insertion
    // Mark doc comments (those that appear right before functions) as already handled
    let mut comments_to_insert: Vec<CommentToInsert> = Vec::new();
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

    // Collect all comments, marking doc comments appropriately
    for comment_group in &ast.comments {
        for comment in &comment_group.list {
            let is_doc = doc_comment_lines.contains(&(comment.slash.line as u32));
            comments_to_insert.push(CommentToInsert {
                go_line: comment.slash.line as u32,
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

    // Codegen with comment insertion and blank line preservation
    let output = match gors::codegen::generate_with_comments_and_blanks(
        compiled,
        &comments_to_insert,
        &blank_lines,
    ) {
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

    // Build the source map from the tracked positions
    let source_map = gors::compiler::build_source_map(&output);

    BuildResult::success_result(output, source_map)
}

/// Collect information about blank lines in the Go source.
fn collect_blank_lines(source: &str) -> BlankLineInfo {
    let mut info = BlankLineInfo::default();
    let lines: Vec<&str> = source.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        // Check if this non-empty line is followed by a blank line
        if !line.trim().is_empty() {
            // Check if next line exists and is blank
            if i + 1 < lines.len() && lines[i + 1].trim().is_empty() {
                info.lines_with_trailing_blank.insert(line_num);
            }
        }
    }

    info
}
