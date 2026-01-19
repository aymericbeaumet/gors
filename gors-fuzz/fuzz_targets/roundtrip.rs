//! Roundtrip fuzzing target
//!
//! This target tests that parsing and then printing produces consistent output.
//! This helps find issues where:
//! - Parsing loses information
//! - Printing produces invalid output
//! - Re-parsing printed output gives different AST

// Fuzz targets use panics to signal bugs found during fuzzing
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use afl::fuzz;

fn main() {
    fuzz!(|data: &[u8]| {
        // Try to interpret as UTF-8
        let input = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Skip empty inputs
        if input.is_empty() {
            return;
        }

        // Try to parse the input
        let ast = match gors::parser::parse_file("fuzz.go", input) {
            Ok(ast) => ast,
            Err(_) => return, // Invalid input, skip
        };

        // Print the AST to a buffer
        let mut output = Vec::new();
        if gors::ast::fprint(&mut output, ast).is_err() {
            return; // Print failed, skip (might be expected for some edge cases)
        }

        // The printed output should be valid UTF-8
        let printed = match std::str::from_utf8(&output) {
            Ok(s) => s.to_owned(),
            Err(_) => {
                // This would be a bug - we printed invalid UTF-8
                panic!("fprint produced invalid UTF-8");
            }
        };

        // Re-parse the printed output
        let ast2 = match gors::parser::parse_file("fuzz.go", &printed) {
            Ok(ast) => ast,
            Err(e) => {
                // This would be a bug - we printed something that can't be re-parsed
                panic!(
                    "roundtrip failed: printed output could not be re-parsed: {}\nOriginal:\n{}\nPrinted:\n{}",
                    e, input, printed
                );
            }
        };

        // Print the re-parsed AST
        let mut output2 = Vec::new();
        if gors::ast::fprint(&mut output2, ast2).is_ok() {
            // The second print should match the first print (idempotence)
            if output != output2 {
                panic!(
                    "roundtrip not idempotent:\nFirst print:\n{}\nSecond print:\n{}",
                    printed,
                    std::str::from_utf8(&output2).unwrap_or("<invalid utf8>")
                );
            }
        }
    });
}
