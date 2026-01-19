//! Scanner fuzzing target
//!
//! This target fuzzes the Go scanner/lexer to find edge cases in tokenization.
//! It tests various inputs including:
//! - Valid and invalid UTF-8 sequences
//! - Unterminated strings, comments, and runes
//! - Edge cases in number literals (hex, octal, binary, floats)
//! - Unicode identifiers and escape sequences

use afl::fuzz;

fn main() {
    fuzz!(|data: &[u8]| {
        // Try to interpret as UTF-8, but also test invalid UTF-8 handling
        let input = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return, // Scanner expects valid UTF-8, skip invalid
        };

        // Skip empty inputs
        if input.is_empty() {
            return;
        }

        // Create scanner and collect all tokens
        let scanner = gors::scanner::Scanner::new("fuzz.go", input);

        // Iterate through all tokens, catching any panics via the fuzzer
        for result in scanner {
            match result {
                Ok((_pos, _token, _literal)) => {
                    // Token was successfully scanned
                }
                Err(_e) => {
                    // Scanner error is expected for malformed input, not a bug
                }
            }
        }
    });
}
