//! Property-based tests for gors
//!
//! These tests use proptest to generate random inputs and verify properties.
//! They can run on stable Rust without AFL, making them suitable for CI.

// Tests may use unwrap for assertions
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;

/// Strategy for generating valid-ish Go source code
fn go_source_strategy() -> impl Strategy<Value = String> {
    // Generate semi-structured Go-like code
    let package_name = prop::sample::select(vec!["main", "foo", "bar", "test"]);
    let identifier = "[a-z][a-zA-Z0-9_]{0,10}";
    let number = prop::sample::select(vec![
        "0",
        "42",
        "0x1F",
        "0o77",
        "0b1010",
        "3.14",
        "1e10",
        "1_000_000",
    ]);

    // Generate a basic Go source with various constructs
    (package_name, identifier, number).prop_map(|(pkg, ident, num)| {
        format!(
            r#"package {}

const {} = {}

func main() {{
}}
"#,
            pkg, ident, num
        )
    })
}

/// Strategy for generating arbitrary UTF-8 strings
fn arbitrary_utf8() -> impl Strategy<Value = String> {
    prop::string::string_regex(".{0,1000}").unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Test that the scanner doesn't panic on arbitrary UTF-8 input
    #[test]
    fn scanner_no_panic(input in arbitrary_utf8()) {
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            // Just iterate through all tokens, ignoring errors
            let _ = result;
        }
    }

    /// Test that the parser doesn't panic on arbitrary UTF-8 input
    #[test]
    fn parser_no_panic(input in arbitrary_utf8()) {
        let _ = gors::parser::parse_file("test.go", &input);
    }

    /// Test that valid Go source parses successfully
    #[test]
    fn valid_go_parses(source in go_source_strategy()) {
        let result = gors::parser::parse_file("test.go", &source);
        prop_assert!(result.is_ok(), "Failed to parse valid Go: {}", source);
    }

    /// Test that valid Go source can be parsed and printed without error
    #[test]
    fn parse_and_print_no_error(source in go_source_strategy()) {
        // Parse the source
        let ast = gors::parser::parse_file("test.go", &source);
        prop_assert!(ast.is_ok(), "Parse failed: {:?}", ast.err());

        // Print the AST
        let mut output = Vec::new();
        let print_result = gors::ast::fprint(&mut output, ast.unwrap());
        prop_assert!(print_result.is_ok(), "Print failed: {:?}", print_result.err());

        // Verify the output is valid UTF-8
        prop_assert!(std::str::from_utf8(&output).is_ok(), "Print produced invalid UTF-8");
    }
}

/// Strategy for generating edge case strings
fn edge_case_strings() -> impl Strategy<Value = String> {
    prop_oneof![
        // Empty and whitespace
        Just("".to_string()),
        Just(" ".to_string()),
        Just("\n".to_string()),
        Just("\t".to_string()),
        Just("\r\n".to_string()),
        // Partial keywords
        Just("package".to_string()),
        Just("package ".to_string()),
        Just("func".to_string()),
        Just("func()".to_string()),
        // Unterminated constructs
        Just("\"unterminated string".to_string()),
        Just("'".to_string()),
        Just("/*".to_string()),
        Just("/* unclosed".to_string()),
        Just("`raw".to_string()),
        // Number edge cases
        Just("0x".to_string()),
        Just("0o".to_string()),
        Just("0b".to_string()),
        Just("1e".to_string()),
        Just("1.".to_string()),
        Just(".1".to_string()),
        // Unicode edge cases
        Just("// 日本語".to_string()),
        Just("var 世界 = 1".to_string()),
        Just("\"\\u0000\"".to_string()),
        Just("\"\\U00000000\"".to_string()),
        // Deep nesting
        Just("((((((((((1))))))))))".to_string()),
        Just("[[[[[[[[[[1]]]]]]]]]]".to_string()),
        Just("{{{{{{{{{{}}}}}}}}}}".to_string()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test edge cases don't cause panics
    #[test]
    fn edge_cases_no_panic(input in edge_case_strings()) {
        // Scanner
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            let _ = result;
        }

        // Parser
        let _ = gors::parser::parse_file("test.go", &input);
    }
}
