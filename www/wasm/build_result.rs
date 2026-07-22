use gors::error::{Diagnostic, DiagnosticKind};
use gors::sourcemap::SourceMap;
use wasm_bindgen::prelude::*;

/// Result of a browser build operation.
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

    /// Return the complete Source Map v3 document.
    #[wasm_bindgen]
    pub fn get_source_map_json(&self) -> String {
        self.source_map
            .as_ref()
            .map(|source_map| {
                let mut bytes = Vec::new();
                source_map.to_writer(&mut bytes).ok();
                String::from_utf8(bytes).unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// Return mappings as `[output_line, output_col, go_line, go_col, name]`.
    #[wasm_bindgen]
    pub fn get_mappings_json(&self) -> String {
        let Some(ref source_map) = self.source_map else {
            return "[]".to_string();
        };

        let mut output = String::from("[");
        for index in 0..source_map.get_token_count() {
            let Some(token) = source_map.get_token(index as usize) else {
                continue;
            };

            if output.len() > 1 {
                output.push(',');
            }
            output.push('[');
            output.push_str(&token.get_dst_line().to_string());
            output.push(',');
            output.push_str(&token.get_dst_col().to_string());
            output.push(',');
            output.push_str(&token.get_src_line().to_string());
            output.push(',');
            output.push_str(&token.get_src_col().to_string());
            output.push(',');
            push_json_string(&mut output, token.get_name().unwrap_or(""));
            output.push(']');
        }
        output.push(']');
        output
    }

    /// Return flat groups of `[output_line, output_col, go_line, go_col]`.
    #[wasm_bindgen]
    pub fn get_mapping_positions(&self) -> Vec<u32> {
        let Some(ref source_map) = self.source_map else {
            return Vec::new();
        };

        let mut positions = Vec::with_capacity(source_map.get_token_count() as usize * 4);
        for index in 0..source_map.get_token_count() {
            let Some(token) = source_map.get_token(index as usize) else {
                continue;
            };
            positions.push(token.get_dst_line());
            positions.push(token.get_dst_col());
            positions.push(token.get_src_line());
            positions.push(token.get_src_col());
        }
        positions
    }

    /// Return token names corresponding to [`Self::get_mapping_positions`].
    #[wasm_bindgen]
    pub fn get_mapping_names_json(&self) -> String {
        let Some(ref source_map) = self.source_map else {
            return "[]".to_string();
        };

        let mut output = String::from("[");
        for index in 0..source_map.get_token_count() {
            let Some(token) = source_map.get_token(index as usize) else {
                continue;
            };
            if output.len() > 1 {
                output.push(',');
            }
            push_json_string(&mut output, token.get_name().unwrap_or(""));
        }
        output.push(']');
        output
    }

    /// Look up the 0-based Go position for a 0-based output position.
    #[wasm_bindgen]
    pub fn lookup_token(&self, output_line: u32, output_column: u32) -> Vec<u32> {
        self.source_map
            .as_ref()
            .and_then(|source_map| source_map.lookup_token(output_line, output_column))
            .map(|token| vec![token.get_src_line(), token.get_src_col()])
            .unwrap_or_default()
    }

    /// Map a 1-based output position to a 1-based Go span for Monaco.
    #[wasm_bindgen]
    pub fn output_to_go(&self, output_line: u32, output_column: u32) -> Vec<u32> {
        let Some(ref source_map) = self.source_map else {
            return vec![];
        };

        let target_line = output_line.saturating_sub(1);
        let target_column = output_column.saturating_sub(1);
        let mut best_token = None;
        let mut best_distance = u32::MAX;
        let mut best_is_before_cursor = false;

        for index in 0..source_map.get_token_count() {
            let Some(token) = source_map.get_token(index as usize) else {
                continue;
            };
            let token_line = token.get_dst_line();
            let token_column = token.get_dst_col();
            if token_line != target_line {
                continue;
            }

            let distance = target_column.abs_diff(token_column);
            let is_before_cursor = token_column <= target_column;
            let is_better = match (best_is_before_cursor, is_before_cursor) {
                (false, true) => true,
                (true, false) => false,
                _ => distance < best_distance,
            };
            if is_better {
                best_distance = distance;
                best_token = Some(token);
                best_is_before_cursor = is_before_cursor;
            }
        }

        let Some(token) = best_token else {
            return vec![];
        };
        let start_line = token.get_src_line() + 1;
        let start_column = token.get_src_col() + 1;
        let name_length = token
            .get_name()
            .map(|name| name.encode_utf16().count() as u32)
            .unwrap_or(1);
        vec![
            start_line,
            start_column,
            start_line,
            start_column + name_length,
        ]
    }

    /// Map a 1-based Go position to a 1-based output span for Monaco.
    #[wasm_bindgen]
    pub fn go_to_output(&self, go_line: u32, go_column: u32) -> Vec<u32> {
        let Some(ref source_map) = self.source_map else {
            return vec![];
        };

        let target_line = go_line.saturating_sub(1);
        let target_column = go_column.saturating_sub(1);
        let mut best_token = None;
        let mut best_distance = u32::MAX;
        let mut best_is_before_cursor = false;

        for index in 0..source_map.get_token_count() {
            let Some(token) = source_map.get_token(index as usize) else {
                continue;
            };
            let token_line = token.get_src_line();
            let token_column = token.get_src_col();
            if token_line != target_line {
                continue;
            }

            let distance = target_column.abs_diff(token_column);
            let is_before_cursor = token_column <= target_column;
            let is_better = match (best_is_before_cursor, is_before_cursor) {
                (false, true) => true,
                (true, false) => false,
                _ => distance < best_distance,
            };
            if is_better {
                best_distance = distance;
                best_token = Some(token);
                best_is_before_cursor = is_before_cursor;
            }
        }

        let Some(token) = best_token else {
            return vec![];
        };
        let output_line = token.get_dst_line();
        let output_column = token.get_dst_col();
        let start_line = output_line + 1;
        let start_column = output_column + 1;
        let name_length = extract_rust_token_at(&self.output, output_line, output_column)
            .map(|token| token.encode_utf16().count() as u32)
            .unwrap_or(1);
        vec![
            start_line,
            start_column,
            start_line,
            start_column + name_length,
        ]
    }

    /// Return the number of source mappings.
    #[wasm_bindgen]
    pub fn mapping_count(&self) -> u32 {
        self.source_map
            .as_ref()
            .map_or(0, SourceMap::get_token_count)
    }
}

impl BuildResult {
    pub(crate) fn success_rust(output: String, source_map: SourceMap) -> Self {
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

    pub(crate) fn error_result(diagnostic: Diagnostic) -> Self {
        let source_line = diagnostic.source_line.unwrap_or_default();
        let (error_column, error_end_column) = if source_line.is_empty() {
            (diagnostic.column as u32, diagnostic.end_column as u32)
        } else {
            (
                gors::sourcemap::utf8_byte_column_to_utf16(&source_line, diagnostic.column as u32),
                gors::sourcemap::utf8_byte_column_to_utf16(
                    &source_line,
                    diagnostic.end_column as u32,
                ),
            )
        };
        Self {
            success: false,
            output: String::new(),
            error_message: diagnostic.message,
            error_file: diagnostic.file,
            error_line: diagnostic.line as u32,
            error_column,
            error_end_column,
            error_kind: match diagnostic.kind {
                DiagnosticKind::Scanner => "scanner".to_string(),
                DiagnosticKind::Parser => "parser".to_string(),
                DiagnosticKind::Compiler => "compiler".to_string(),
            },
            error_source_line: source_line,
            source_map: None,
        }
    }
}

/// Extract the token at a 0-based UTF-16 position in generated Rust.
pub(crate) fn extract_rust_token_at(rust_source: &str, line: u32, column: u32) -> Option<String> {
    let line_text = rust_source.lines().nth(line as usize)?;
    let byte_offset = utf16_column_to_byte_offset(line_text, column)?;
    let tail = line_text.get(byte_offset..)?;
    let mut chars = tail.chars().peekable();
    let first = chars.next()?;

    if first == '/'
        && chars
            .peek()
            .is_some_and(|next| *next == '/' || *next == '*')
        && chars.peek() == Some(&'/')
    {
        return Some(tail.to_string());
    }

    if first.is_alphabetic() || first == '_' {
        let mut token = String::from(first);
        while chars
            .peek()
            .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
        {
            token.push(chars.next()?);
        }
        if chars.peek() == Some(&'!') {
            token.push('!');
        }
        return Some(token);
    }

    Some(first.to_string())
}

pub(crate) fn utf16_column_to_byte_offset(line: &str, utf16_column: u32) -> Option<usize> {
    let target = utf16_column as usize;
    let mut current = 0usize;
    for (byte_offset, ch) in line.char_indices() {
        if current == target {
            return Some(byte_offset);
        }
        current += ch.len_utf16();
        if current > target {
            return None;
        }
    }
    (current == target).then_some(line.len())
}

fn push_json_string(output: &mut String, value: &str) {
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let value = ch as usize;
                output.push_str("\\u00");
                output.push(HEX[(value >> 4) & 0xf] as char);
                output.push(HEX[value & 0xf] as char);
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
}
