// Error formatting module for nicely displaying lexer/parser/compiler errors
// with source context, similar to rustc or Go's error output.

use crate::parser::ParserError;
use crate::scanner::ScannerError;
use serde::Serialize;
use std::fmt;

/// Represents a diagnostic error with source location and context.
///
/// This struct provides structured error information that can be used
/// for both terminal output and IDE integration.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// The file path where the error occurred
    pub file: String,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (1-indexed, start of error)
    pub column: usize,
    /// End column number (1-indexed, for highlighting range)
    pub end_column: usize,
    /// The error message
    pub message: String,
    /// Optional source line for context
    pub source_line: Option<String>,
    /// Kind of error
    pub kind: DiagnosticKind,
}

impl Diagnostic {
    /// Create a Diagnostic from a ScannerError with source context
    pub fn from_scanner_error(err: &ScannerError, file: &str, source: &str) -> Self {
        Self::new(file, err.line, err.column, err.message(), DiagnosticKind::Scanner)
            .with_source(source)
    }

    /// Create a Diagnostic from a ParserError with source context
    pub fn from_parser_error(err: &ParserError, file: &str, source: &str) -> Self {
        match err {
            ParserError::ScannerError(scanner_err) => {
                Self::from_scanner_error(scanner_err, file, source)
            }
            ParserError::UnexpectedEndOfFile => {
                // Position at the end of the source
                let (line, column) = if source.is_empty() {
                    (1, 1)
                } else {
                    offset_to_line_col(source, source.len())
                };
                Self::new(file, line, column, err.message(), DiagnosticKind::Parser)
                    .with_source(source)
            }
            ParserError::UnexpectedToken => {
                // No position info available
                Self::new(file, 0, 0, err.message(), DiagnosticKind::Parser)
            }
            ParserError::UnexpectedTokenAt { file: err_file, line, column, .. } => {
                let actual_file = if err_file.is_empty() || err_file == "/" { file } else { err_file };
                Self::new(actual_file, *line, *column, err.message(), DiagnosticKind::Parser)
                    .with_source(source)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticKind {
    Scanner,
    Parser,
    Compiler,
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scanner => write!(f, "scanner error"),
            Self::Parser => write!(f, "syntax error"),
            Self::Compiler => write!(f, "compile error"),
        }
    }
}

impl Diagnostic {
    /// Create a new Diagnostic with the given location and message.
    pub fn new(
        file: impl Into<String>,
        line: usize,
        column: usize,
        message: impl Into<String>,
        kind: DiagnosticKind,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            column,
            end_column: column + 1, // Default to single character
            message: message.into(),
            source_line: None,
            kind,
        }
    }

    /// Add source context and calculate end_column for error highlighting.
    pub fn with_source(mut self, source: &str) -> Self {
        if self.line > 0 {
            if let Some(line) = source.lines().nth(self.line - 1) {
                self.source_line = Some(line.to_string());
                self.end_column = self.calculate_end_column(line);
            }
        }
        self
    }

    /// Manually set the source line for context.
    pub fn with_source_line(mut self, source_line: impl Into<String>) -> Self {
        let line = source_line.into();
        self.end_column = self.calculate_end_column(&line);
        self.source_line = Some(line);
        self
    }

    /// Calculate the end column for error highlighting based on the token at the error position.
    fn calculate_end_column(&self, source_line: &str) -> usize {
        let col = self.column.saturating_sub(1); // 0-indexed
        let chars: Vec<char> = source_line.chars().collect();

        if col >= chars.len() {
            return self.column + 1;
        }

        let mut end = col;
        let start_char = chars[col];

        if start_char.is_alphanumeric() || start_char == '_' {
            // Identifier or keyword - find end of word
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
        } else if start_char == '"' || start_char == '\'' || start_char == '`' {
            // String/char literal - find closing quote or end of line
            end += 1;
            while end < chars.len() && chars[end] != start_char {
                if chars[end] == '\\' && end + 1 < chars.len() {
                    end += 1; // Skip escaped char
                }
                end += 1;
            }
            if end < chars.len() {
                end += 1; // Include closing quote
            }
        } else {
            // Single character token or operator
            end += 1;
            // Check for multi-character operators
            if end < chars.len() {
                let two_char: String = [start_char, chars[end]].iter().collect();
                if matches!(
                    two_char.as_str(),
                    ":=" | "==" | "!=" | "<=" | ">=" | "&&" | "||"
                        | "++" | "--" | "+=" | "-=" | "*=" | "/="
                        | "<<" | ">>"
                ) {
                    end += 1;
                }
            }
        }

        end + 1 // Convert back to 1-indexed
    }

    /// Format for terminal output with colors (when supported)
    pub fn format_terminal(&self, use_colors: bool) -> String {
        let mut output = String::new();

        // Location line
        let location = if self.file.is_empty() {
            format!("{}:{}", self.line, self.column)
        } else {
            format!("{}:{}:{}", self.file, self.line, self.column)
        };

        if use_colors {
            // Bold location, red error kind
            output.push_str(&format!(
                "\x1b[1m{}\x1b[0m: \x1b[31m{}\x1b[0m: {}\n",
                location, self.kind, self.message
            ));
        } else {
            output.push_str(&format!("{}: {}: {}\n", location, self.kind, self.message));
        }

        // Source context if available
        if let Some(ref source_line) = self.source_line {
            // Line number gutter
            let line_num = format!("{:>4} | ", self.line);
            let gutter_width = line_num.len();

            if use_colors {
                output.push_str(&format!("\x1b[34m{}\x1b[0m", line_num));
            } else {
                output.push_str(&line_num);
            }
            output.push_str(source_line);
            output.push('\n');

            // Caret line pointing to error position
            let spaces = " ".repeat(gutter_width);
            let prefix = if self.column > 1 {
                // Calculate visual position accounting for tabs
                let visual_col: usize = source_line
                    .chars()
                    .take(self.column - 1)
                    .map(|c| if c == '\t' { 4 } else { 1 })
                    .sum();
                " ".repeat(visual_col)
            } else {
                String::new()
            };

            if use_colors {
                output.push_str(&format!("{}{}\x1b[32m^\x1b[0m\n", spaces, prefix));
            } else {
                output.push_str(&format!("{}{}^\n", spaces, prefix));
            }
        }

        output
    }

    /// Format for plain text (no colors)
    pub fn format_plain(&self) -> String {
        self.format_terminal(false)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_plain())
    }
}

/// A collection of diagnostics that can be displayed together
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    pub errors: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self { errors: vec![] }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.errors.push(diagnostic);
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in &self.errors {
            write!(f, "{}", error)?;
        }
        if self.errors.len() > 1 {
            writeln!(f, "\n{} errors generated.", self.errors.len())?;
        }
        Ok(())
    }
}

/// Helper to extract a source line from a buffer given an offset
pub fn get_line_at_offset(source: &str, offset: usize) -> Option<(usize, &str)> {
    let mut current_offset = 0;
    for (line_num, line) in source.lines().enumerate() {
        let line_end = current_offset + line.len();
        if offset >= current_offset && offset <= line_end {
            return Some((line_num + 1, line));
        }
        current_offset = line_end + 1; // +1 for newline
    }
    None
}

/// Helper to convert an offset to line and column
pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_display() {
        let diag = Diagnostic::new("test.go", 5, 10, "unexpected token", DiagnosticKind::Parser)
            .with_source_line("func main() {");

        let output = diag.format_plain();
        assert!(output.contains("test.go:5:10"));
        assert!(output.contains("syntax error"));
        assert!(output.contains("unexpected token"));
        assert!(output.contains("func main() {"));
        assert!(output.contains("^"));
    }

    #[test]
    fn test_offset_to_line_col() {
        let source = "line1\nline2\nline3";
        assert_eq!(offset_to_line_col(source, 0), (1, 1));
        assert_eq!(offset_to_line_col(source, 5), (1, 6)); // at newline
        assert_eq!(offset_to_line_col(source, 6), (2, 1));
        assert_eq!(offset_to_line_col(source, 12), (3, 1));
    }
}
