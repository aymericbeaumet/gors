//! Parser fuzzing target
//!
//! This target fuzzes the Go parser to find edge cases in syntax parsing.
//! It tests various inputs including:
//! - Malformed package declarations
//! - Invalid expression syntax
//! - Deeply nested structures
//! - Edge cases in type declarations
//! - Generic syntax (type parameters)
//! - Complex statement combinations

// Fuzz targets use panics to signal bugs found during fuzzing
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use afl::fuzz;

fn main() {
    fuzz!(|data: &[u8]| {
        // Try to interpret as UTF-8
        let input = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return, // Parser expects valid UTF-8, skip invalid
        };

        // Skip empty inputs
        if input.is_empty() {
            return;
        }

        // Try to parse the input as a Go source file
        match gors::parser::parse_file("fuzz.go", input) {
            Ok(_ast) => {
                // Successfully parsed - this is valid Go code
            }
            Err(_e) => {
                // Parse error is expected for malformed input, not a bug
            }
        }
    });
}
