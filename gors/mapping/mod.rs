//! Source mapping between Go and Rust code.
//!
//! This module provides data structures for tracking the correspondence
//! between positions in Go source code and the generated Rust output.

use std::collections::BTreeMap;

/// A span in Go source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoSpan {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl GoSpan {
    pub fn new(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> Self {
        Self {
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    /// Create a span from a single position (point span).
    pub fn point(line: u32, column: u32) -> Self {
        Self {
            start_line: line,
            start_column: column,
            end_line: line,
            end_column: column,
        }
    }

    /// Check if a position falls within this span.
    pub fn contains(&self, line: u32, column: u32) -> bool {
        if line < self.start_line || line > self.end_line {
            return false;
        }
        if line == self.start_line && column < self.start_column {
            return false;
        }
        if line == self.end_line && column > self.end_column {
            return false;
        }
        true
    }
}

/// A span in Rust output code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RustSpan {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl RustSpan {
    pub fn new(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> Self {
        Self {
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    /// Check if a position falls within this span.
    pub fn contains(&self, line: u32, column: u32) -> bool {
        if line < self.start_line || line > self.end_line {
            return false;
        }
        if line == self.start_line && column < self.start_column {
            return false;
        }
        if line == self.end_line && column > self.end_column {
            return false;
        }
        true
    }
}

/// The kind of AST node being mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingKind {
    /// Function declaration
    Function,
    /// Identifier (variable, function name, etc.)
    Identifier,
    /// Literal value (number, string, etc.)
    Literal,
    /// Binary operator
    Operator,
    /// Statement
    Statement,
    /// Expression
    Expression,
    /// Keyword (if, for, return, etc.)
    Keyword,
}

/// A single mapping between Go source and Rust output.
#[derive(Debug, Clone)]
pub struct SpanMapping {
    /// The span in Go source code
    pub go_span: GoSpan,
    /// The span in Rust output (filled in during codegen)
    pub rust_span: RustSpan,
    /// What kind of AST node this represents
    pub kind: MappingKind,
    /// Optional name for debugging (e.g., identifier name)
    pub name: Option<String>,
    /// Unique ID for this mapping (used to correlate during codegen)
    pub id: u32,
}

impl SpanMapping {
    pub fn new(go_span: GoSpan, kind: MappingKind, id: u32) -> Self {
        Self {
            go_span,
            rust_span: RustSpan::default(),
            kind,
            name: None,
            id,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// Collection of source mappings with lookup capabilities.
#[derive(Debug, Default)]
pub struct SourceMap {
    /// All mappings, indexed by their ID
    mappings: Vec<SpanMapping>,
    /// Index for Go position lookups: (line, column) -> mapping indices
    go_index: BTreeMap<(u32, u32), Vec<usize>>,
    /// Index for Rust position lookups: (line, column) -> mapping indices
    rust_index: BTreeMap<(u32, u32), Vec<usize>>,
    /// Counter for generating unique mapping IDs
    next_id: u32,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new mapping and return its ID.
    pub fn add(&mut self, go_span: GoSpan, kind: MappingKind) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        let mapping = SpanMapping::new(go_span, kind, id);
        let index = self.mappings.len();
        self.mappings.push(mapping);

        // Index by Go start position
        self.go_index
            .entry((go_span.start_line, go_span.start_column))
            .or_default()
            .push(index);

        id
    }

    /// Add a mapping with a name.
    pub fn add_named(&mut self, go_span: GoSpan, kind: MappingKind, name: impl Into<String>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        let mapping = SpanMapping::new(go_span, kind, id).with_name(name);
        let index = self.mappings.len();
        self.mappings.push(mapping);

        // Index by Go start position
        self.go_index
            .entry((go_span.start_line, go_span.start_column))
            .or_default()
            .push(index);

        id
    }

    /// Update the Rust span for a mapping by ID.
    pub fn set_rust_span(&mut self, id: u32, rust_span: RustSpan) {
        if let Some(mapping) = self.mappings.iter_mut().find(|m| m.id == id) {
            mapping.rust_span = rust_span;

            // Update the Rust index
            let index = self.mappings.iter().position(|m| m.id == id).unwrap();
            self.rust_index
                .entry((rust_span.start_line, rust_span.start_column))
                .or_default()
                .push(index);
        }
    }

    /// Find mappings that contain the given Go position.
    pub fn find_by_go_position(&self, line: u32, column: u32) -> Vec<&SpanMapping> {
        self.mappings
            .iter()
            .filter(|m| m.go_span.contains(line, column))
            .collect()
    }

    /// Find mappings that contain the given Rust position.
    pub fn find_by_rust_position(&self, line: u32, column: u32) -> Vec<&SpanMapping> {
        self.mappings
            .iter()
            .filter(|m| m.rust_span.contains(line, column))
            .collect()
    }

    /// Get the Rust span for a Go position (returns the most specific/smallest span).
    pub fn go_to_rust(&self, line: u32, column: u32) -> Option<RustSpan> {
        let mappings = self.find_by_go_position(line, column);
        // Return the smallest (most specific) span
        mappings
            .into_iter()
            .filter(|m| m.rust_span != RustSpan::default())
            .min_by_key(|m| {
                let go = &m.go_span;
                (go.end_line - go.start_line) * 1000
                    + (go.end_column.saturating_sub(go.start_column))
            })
            .map(|m| m.rust_span)
    }

    /// Get the Go span for a Rust position (returns the most specific/smallest span).
    pub fn rust_to_go(&self, line: u32, column: u32) -> Option<GoSpan> {
        let mappings = self.find_by_rust_position(line, column);
        // Return the smallest (most specific) span
        mappings
            .into_iter()
            .min_by_key(|m| {
                let rust = &m.rust_span;
                (rust.end_line - rust.start_line) * 1000
                    + (rust.end_column.saturating_sub(rust.start_column))
            })
            .map(|m| m.go_span)
    }

    /// Get all mappings.
    pub fn mappings(&self) -> &[SpanMapping] {
        &self.mappings
    }

    /// Get all mappings mutably.
    pub fn mappings_mut(&mut self) -> &mut [SpanMapping] {
        &mut self.mappings
    }

    /// Get the number of mappings.
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// Check if there are no mappings.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Serialize mappings to a flat array for JS interop.
    /// Format: [go_start_line, go_start_col, go_end_line, go_end_col,
    ///          rust_start_line, rust_start_col, rust_end_line, rust_end_col, ...]
    pub fn to_flat_array(&self) -> Vec<u32> {
        let mut result = Vec::with_capacity(self.mappings.len() * 8);
        for mapping in &self.mappings {
            result.push(mapping.go_span.start_line);
            result.push(mapping.go_span.start_column);
            result.push(mapping.go_span.end_line);
            result.push(mapping.go_span.end_column);
            result.push(mapping.rust_span.start_line);
            result.push(mapping.rust_span.start_column);
            result.push(mapping.rust_span.end_line);
            result.push(mapping.rust_span.end_column);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_span_contains() {
        let span = GoSpan::new(1, 5, 1, 10);
        assert!(span.contains(1, 5));
        assert!(span.contains(1, 7));
        assert!(span.contains(1, 10));
        assert!(!span.contains(1, 4));
        assert!(!span.contains(1, 11));
        assert!(!span.contains(2, 5));
    }

    #[test]
    fn test_multiline_span_contains() {
        let span = GoSpan::new(1, 5, 3, 10);
        assert!(span.contains(1, 5));
        assert!(span.contains(1, 100)); // Any column on line 1 after start
        assert!(span.contains(2, 1)); // Any column on middle line
        assert!(span.contains(3, 1)); // Start of end line
        assert!(span.contains(3, 10)); // End position
        assert!(!span.contains(1, 4)); // Before start column on start line
        assert!(!span.contains(3, 11)); // After end column on end line
    }

    #[test]
    fn test_source_map_add_and_lookup() {
        let mut map = SourceMap::new();

        let id1 = map.add_named(GoSpan::new(1, 1, 1, 4), MappingKind::Identifier, "main");
        let id2 = map.add_named(GoSpan::new(2, 5, 2, 10), MappingKind::Literal, "42");

        map.set_rust_span(id1, RustSpan::new(1, 1, 1, 4));
        map.set_rust_span(id2, RustSpan::new(2, 5, 2, 7));

        // Look up by Go position
        let rust_span = map.go_to_rust(1, 2).unwrap();
        assert_eq!(rust_span.start_line, 1);
        assert_eq!(rust_span.start_column, 1);

        // Look up by Rust position
        let go_span = map.rust_to_go(2, 6).unwrap();
        assert_eq!(go_span.start_line, 2);
        assert_eq!(go_span.start_column, 5);
    }
}
