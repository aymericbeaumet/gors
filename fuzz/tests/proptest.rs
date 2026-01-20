//! Property-based tests for gors
//!
//! These tests use proptest to generate random inputs and verify properties.
//! They can run on stable Rust without AFL, making them suitable for CI.
//!
//! The number of test cases can be controlled via the PROPTEST_CASES environment
//! variable (default: 100,000 cases for thorough fuzzing).

// Tests may use unwrap for assertions
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;

/// Get the number of test cases from environment or use default
fn get_test_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000)
}

/// Get the number of edge case tests from environment or use default
fn get_edge_test_cases() -> u32 {
    std::env::var("PROPTEST_EDGE_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000)
}

/// Go keywords that cannot be used as identifiers
const GO_KEYWORDS: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else",
    "fallthrough", "for", "func", "go", "goto", "if", "import", "interface",
    "map", "package", "range", "return", "select", "struct", "switch", "type",
    "var",
];

/// Strategy for generating Go identifiers (excluding keywords)
fn go_identifier() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z_][a-zA-Z0-9_]{0,20}")
        .unwrap()
        .prop_filter("non-empty and not keyword", |s| {
            !s.is_empty() && !GO_KEYWORDS.contains(&s.as_str())
        })
}

/// Strategy for generating Go type names
fn go_type() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("int".to_string()),
        Just("int8".to_string()),
        Just("int16".to_string()),
        Just("int32".to_string()),
        Just("int64".to_string()),
        Just("uint".to_string()),
        Just("uint8".to_string()),
        Just("uint16".to_string()),
        Just("uint32".to_string()),
        Just("uint64".to_string()),
        Just("float32".to_string()),
        Just("float64".to_string()),
        Just("complex64".to_string()),
        Just("complex128".to_string()),
        Just("bool".to_string()),
        Just("string".to_string()),
        Just("byte".to_string()),
        Just("rune".to_string()),
        Just("error".to_string()),
        Just("any".to_string()),
        go_identifier(),
    ]
}

/// Strategy for generating Go literal values
fn go_literal() -> impl Strategy<Value = String> {
    prop_oneof![
        // Integer literals
        prop::num::i64::ANY.prop_map(|n| n.to_string()),
        prop::string::string_regex("0x[0-9a-fA-F]{1,16}").unwrap(),
        prop::string::string_regex("0o[0-7]{1,21}").unwrap(),
        prop::string::string_regex("0b[01]{1,64}").unwrap(),
        prop::string::string_regex("[0-9]{1,3}(_[0-9]{3}){0,5}").unwrap(),
        // Float literals
        prop::num::f64::NORMAL.prop_map(|n| format!("{:e}", n)),
        prop::string::string_regex("[0-9]{1,10}\\.[0-9]{1,10}").unwrap(),
        // String literals
        prop::string::string_regex("\"[a-zA-Z0-9 ]{0,50}\"").unwrap(),
        prop::string::string_regex("`[a-zA-Z0-9 \\n]{0,50}`").unwrap(),
        // Rune literals
        prop::string::string_regex("'[a-zA-Z0-9]'").unwrap(),
        // Boolean literals
        Just("true".to_string()),
        Just("false".to_string()),
        // Nil
        Just("nil".to_string()),
    ]
}

/// Strategy for generating Go expressions
fn go_expression(depth: u32) -> impl Strategy<Value = String> {
    if depth == 0 {
        prop_oneof![go_literal(), go_identifier(),].boxed()
    } else {
        prop_oneof![
            5 => go_literal(),
            5 => go_identifier(),
            // Unary expressions (only valid prefix operators in Go)
            // Note: -- and ++ are statements in Go, not expressions
            1 => go_identifier().prop_map(|e| format!("-{}", e)),
            1 => go_identifier().prop_map(|e| format!("!{}", e)),
            1 => go_identifier().prop_map(|e| format!("&{}", e)),
            1 => go_identifier().prop_map(|e| format!("*{}", e)),
            1 => go_identifier().prop_map(|e| format!("^{}", e)),
            // Binary expressions
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} + {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} - {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} * {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} / {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} == {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} != {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} < {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} && {})", a, b)),
            1 => (go_expression(depth - 1), go_expression(depth - 1))
                .prop_map(|(a, b)| format!("({} || {})", a, b)),
            // Parenthesized
            1 => go_expression(depth - 1).prop_map(|e| format!("({})", e)),
            // Function call
            1 => (go_identifier(), go_expression(depth - 1))
                .prop_map(|(f, arg)| format!("{}({})", f, arg)),
            // Index expression
            1 => (go_identifier(), go_expression(depth - 1))
                .prop_map(|(arr, idx)| format!("{}[{}]", arr, idx)),
            // Selector expression
            1 => (go_identifier(), go_identifier()).prop_map(|(obj, field)| format!("{}.{}", obj, field)),
        ]
        .boxed()
    }
}

/// Strategy for generating simple Go statements (non-recursive)
fn go_simple_statement() -> impl Strategy<Value = String> {
    prop_oneof![
        // Variable declaration
        (go_identifier(), go_type(), go_literal())
            .prop_map(|(name, typ, val)| format!("var {} {} = {}", name, typ, val)),
        // Short variable declaration with literal (safer)
        (go_identifier(), go_literal()).prop_map(|(name, val)| format!("{} := {}", name, val)),
        // Assignment with literal
        (go_identifier(), go_literal()).prop_map(|(name, val)| format!("{} = {}", name, val)),
        // Expression statement (just a function call)
        (go_identifier(), go_literal()).prop_map(|(f, arg)| format!("{}({})", f, arg)),
        // Return statement with literal
        go_literal().prop_map(|e| format!("return {}", e)),
        // Go statement
        (go_identifier(), go_literal()).prop_map(|(f, arg)| format!("go {}({})", f, arg)),
        // Defer statement
        (go_identifier(), go_literal()).prop_map(|(f, arg)| format!("defer {}({})", f, arg)),
        // Send statement with literal
        (go_identifier(), go_literal()).prop_map(|(ch, val)| format!("{} <- {}", ch, val)),
        // Receive statement
        (go_identifier(), go_identifier())
            .prop_map(|(name, ch)| format!("{} := <-{}", name, ch)),
        // Increment/decrement (these are statements, not expressions)
        go_identifier().prop_map(|v| format!("{}++", v)),
        go_identifier().prop_map(|v| format!("{}--", v)),
    ]
}

/// Strategy for generating Go statements (including compound statements)
fn go_statement() -> impl Strategy<Value = String> {
    prop_oneof![
        10 => go_simple_statement(),
        // If statement (uses simple statement to avoid deep recursion)
        1 => (go_expression(1), go_simple_statement()).prop_map(|(cond, body)| format!(
            "if {} {{\n\t{}\n}}",
            cond, body
        )),
        // For statement
        1 => (go_identifier(), go_expression(1), go_simple_statement()).prop_map(|(i, n, body)| format!(
            "for {} := 0; {} < {}; {}++ {{\n\t{}\n}}",
            i, i, n, i, body
        )),
    ]
}

/// Strategy for generating valid-ish Go source code
fn go_source_strategy() -> impl Strategy<Value = String> {
    let package_name = prop::sample::select(vec!["main", "foo", "bar", "test", "pkg"]);

    (
        package_name,
        prop::collection::vec(go_statement(), 0..10),
        prop::collection::vec(
            (go_identifier(), go_type(), go_literal()),
            0..5,
        ),
        prop::collection::vec(
            (go_identifier(), go_type()),
            0..5,
        ),
    )
        .prop_map(|(pkg, stmts, consts, vars)| {
            let mut source = format!("package {}\n\n", pkg);

            // Add const declarations
            for (name, typ, val) in consts {
                source.push_str(&format!("const {} {} = {}\n", name, typ, val));
            }

            // Add var declarations
            for (name, typ) in vars {
                source.push_str(&format!("var {} {}\n", name, typ));
            }

            // Add main function with statements
            source.push_str("\nfunc main() {\n");
            for stmt in stmts {
                source.push_str(&format!("\t{}\n", stmt));
            }
            source.push_str("}\n");

            source
        })
}

/// Strategy for generating arbitrary UTF-8 strings of varying sizes
fn arbitrary_utf8() -> impl Strategy<Value = String> {
    prop_oneof![
        // Small inputs
        prop::string::string_regex(".{0,100}").unwrap(),
        // Medium inputs
        prop::string::string_regex(".{100,500}").unwrap(),
        // Large inputs
        prop::string::string_regex(".{500,2000}").unwrap(),
        // Very large inputs (stress test)
        prop::string::string_regex(".{2000,5000}").unwrap(),
    ]
}

/// Strategy for generating random bytes converted to lossy UTF-8
fn arbitrary_bytes_as_utf8() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::num::u8::ANY, 0..5000)
        .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: get_test_cases(),
        max_shrink_iters: 10000,
        ..ProptestConfig::default()
    })]

    /// Test that the scanner doesn't panic on arbitrary UTF-8 input
    #[test]
    fn scanner_no_panic(input in arbitrary_utf8()) {
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            // Just iterate through all tokens, ignoring errors
            let _ = result;
        }
    }

    /// Test that the scanner doesn't panic on arbitrary bytes
    #[test]
    fn scanner_no_panic_bytes(input in arbitrary_bytes_as_utf8()) {
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            let _ = result;
        }
    }

    /// Test that the parser doesn't panic on arbitrary UTF-8 input
    #[test]
    fn parser_no_panic(input in arbitrary_utf8()) {
        let _ = gors::parser::parse_file("test.go", &input);
    }

    /// Test that the parser doesn't panic on arbitrary bytes
    #[test]
    fn parser_no_panic_bytes(input in arbitrary_bytes_as_utf8()) {
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

    /// Test that parse -> print produces valid output without errors
    /// Note: fprint outputs an AST dump format, not Go source code,
    /// so we just verify it doesn't error, not that it roundtrips
    #[test]
    fn parse_print_no_error(source in go_source_strategy()) {
        // Parse
        let ast = gors::parser::parse_file("test.go", &source);
        prop_assert!(ast.is_ok(), "Parse failed: {:?}", ast.err());
        let ast = ast.unwrap();

        // Print AST dump
        let mut output = Vec::new();
        let print_result = gors::ast::fprint(&mut output, ast);
        prop_assert!(print_result.is_ok(), "Print failed: {:?}", print_result.err());

        // Verify valid UTF-8 output
        let printed = String::from_utf8(output);
        prop_assert!(printed.is_ok(), "Print produced invalid UTF-8");
        
        // Verify output is non-empty for valid input
        prop_assert!(!printed.unwrap().is_empty(), "Print produced empty output");
    }

    /// Test that expressions can be parsed without panic
    #[test]
    fn expression_no_panic(expr in go_expression(3)) {
        // Wrap in a minimal Go file
        let source = format!("package main\nvar x = {}\n", expr);
        let _ = gors::parser::parse_file("test.go", &source);
    }

    /// Test that statements can be parsed without panic
    #[test]
    fn statement_no_panic(stmt in go_statement()) {
        // Wrap in a minimal Go file
        let source = format!("package main\nfunc main() {{ {} }}\n", stmt);
        let _ = gors::parser::parse_file("test.go", &source);
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
        Just("   \t\n\r\n   ".to_string()),
        // Partial keywords
        Just("package".to_string()),
        Just("package ".to_string()),
        Just("func".to_string()),
        Just("func()".to_string()),
        Just("import".to_string()),
        Just("import (".to_string()),
        Just("type".to_string()),
        Just("type T".to_string()),
        Just("struct".to_string()),
        Just("interface".to_string()),
        Just("chan".to_string()),
        Just("map".to_string()),
        Just("go".to_string()),
        Just("defer".to_string()),
        Just("select".to_string()),
        Just("switch".to_string()),
        Just("case".to_string()),
        Just("default".to_string()),
        Just("fallthrough".to_string()),
        Just("break".to_string()),
        Just("continue".to_string()),
        Just("goto".to_string()),
        Just("return".to_string()),
        // Unterminated constructs
        Just("\"unterminated string".to_string()),
        Just("'".to_string()),
        Just("/*".to_string()),
        Just("/* unclosed".to_string()),
        Just("`raw".to_string()),
        Just("\"\\".to_string()),
        Just("'\\".to_string()),
        Just("`\\".to_string()),
        Just("\"\\x".to_string()),
        Just("\"\\u".to_string()),
        Just("\"\\U".to_string()),
        // Number edge cases
        Just("0x".to_string()),
        Just("0o".to_string()),
        Just("0b".to_string()),
        Just("1e".to_string()),
        Just("1.".to_string()),
        Just(".1".to_string()),
        Just("0x_".to_string()),
        Just("0o_".to_string()),
        Just("0b_".to_string()),
        Just("1__2".to_string()),
        Just("1_.2".to_string()),
        Just("1e+".to_string()),
        Just("1e-".to_string()),
        Just("1p10".to_string()),
        Just("0x1p".to_string()),
        Just("0x1.2p".to_string()),
        Just("1i".to_string()),
        Just("1.0i".to_string()),
        Just("1e10i".to_string()),
        // Unicode edge cases
        Just("// 日本語".to_string()),
        Just("var 世界 = 1".to_string()),
        Just("\"\\u0000\"".to_string()),
        Just("\"\\U00000000\"".to_string()),
        Just("\"\\uFFFF\"".to_string()),
        Just("\"\\U0010FFFF\"".to_string()),
        Just("var \u{200B} = 1".to_string()), // Zero-width space
        Just("var \u{FEFF} = 1".to_string()), // BOM
        Just("\u{2028}".to_string()),         // Line separator
        Just("\u{2029}".to_string()),         // Paragraph separator
        // Moderate nesting (parser has recursion limits)
        Just("(((((1)))))".to_string()),
        Just("[[[[[1]]]]]".to_string()),
        Just("{{{{{}}}}}".to_string()),
        Just("(((((".to_string()),
        Just("[[[[[".to_string()),
        Just("{{{{{".to_string()),
        Just(")))))".to_string()),
        Just("]]]]]".to_string()),
        Just("}}}}}".to_string()),
        // Operator edge cases
        Just("++".to_string()),
        Just("--".to_string()),
        Just("<<".to_string()),
        Just(">>".to_string()),
        Just("<-".to_string()),
        Just("->".to_string()),
        Just("...".to_string()),
        Just(":=".to_string()),
        Just("&&".to_string()),
        Just("||".to_string()),
        Just("&^".to_string()),
        Just("&^=".to_string()),
        Just("<<=".to_string()),
        Just(">>=".to_string()),
        // Comment edge cases
        Just("//".to_string()),
        Just("// \n //".to_string()),
        Just("/* */".to_string()),
        Just("/* /* */".to_string()),
        Just("/***/".to_string()),
        Just("/*/".to_string()),
        // String edge cases
        Just("\"\"".to_string()),
        Just("``".to_string()),
        Just("''".to_string()),
        Just("\"\\n\\r\\t\\\\\\\"\"".to_string()),
        Just("`\n\n\n`".to_string()),
        Just("\"\\000\"".to_string()),
        Just("\"\\xFF\"".to_string()),
        Just("'\\xFF'".to_string()),
        // Generics syntax
        Just("type T[P any] struct{}".to_string()),
        Just("func F[T any]() {}".to_string()),
        Just("T[int]".to_string()),
        Just("T[int, string]".to_string()),
        Just("T[~int]".to_string()),
        Just("T[int | string]".to_string()),
        // Channel types
        Just("chan int".to_string()),
        Just("chan<- int".to_string()),
        Just("<-chan int".to_string()),
        Just("chan chan int".to_string()),
        Just("<-chan <-chan int".to_string()),
        // Map and slice types
        Just("map[string]int".to_string()),
        Just("[]int".to_string()),
        Just("[...]int".to_string()),
        Just("[10]int".to_string()),
        Just("map[map[int]int]int".to_string()),
        // Function types
        Just("func()".to_string()),
        Just("func() int".to_string()),
        Just("func(int)".to_string()),
        Just("func(int) int".to_string()),
        Just("func(...int)".to_string()),
        Just("func() (int, error)".to_string()),
        // Interface and struct
        Just("interface{}".to_string()),
        Just("struct{}".to_string()),
        Just("interface{ M() }".to_string()),
        Just("struct{ F int }".to_string()),
    ]
}

/// Strategy for generating deeply nested expressions
/// Note: depth is kept reasonable to avoid stack overflow in the parser
fn deeply_nested_expressions(max_depth: u32) -> impl Strategy<Value = String> {
    (1..=max_depth).prop_flat_map(|depth| {
        let mut expr = "x".to_string();
        for _ in 0..depth {
            expr = format!("({})", expr);
        }
        Just(expr)
    })
}

/// Strategy for generating repeated patterns
fn repeated_patterns() -> impl Strategy<Value = String> {
    prop_oneof![
        (1..1000usize).prop_map(|n| "a".repeat(n)),
        (1..1000usize).prop_map(|n| " ".repeat(n)),
        (1..1000usize).prop_map(|n| "\n".repeat(n)),
        (1..500usize).prop_map(|n| "ab".repeat(n)),
        (1..200usize).prop_map(|n| "/**/".repeat(n)),
        (1..200usize).prop_map(|n| "//\n".repeat(n)),
        (1..100usize).prop_map(|n| "package main\n".repeat(n)),
        // Keep nesting depth low to avoid stack overflow in parser
        (1..20usize).prop_map(|n| format!("{}x", "(".repeat(n))),
        (1..20usize).prop_map(|n| format!("x{}", ")".repeat(n))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: get_edge_test_cases(),
        max_shrink_iters: 5000,
        ..ProptestConfig::default()
    })]

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

    /// Test moderately nested expressions (parser has recursion limits)
    #[test]
    fn nested_expressions_no_panic(expr in deeply_nested_expressions(20)) {
        let source = format!("package main\nvar x = {}\n", expr);
        let _ = gors::parser::parse_file("test.go", &source);
    }

    /// Test repeated patterns don't cause performance issues
    #[test]
    fn repeated_patterns_no_panic(input in repeated_patterns()) {
        // Scanner
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            let _ = result;
        }

        // Parser
        let _ = gors::parser::parse_file("test.go", &input);
    }

    /// Test that the scanner handles all possible byte values
    #[test]
    fn scanner_all_bytes(byte in prop::num::u8::ANY) {
        let input = String::from_utf8_lossy(&[byte]).into_owned();
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            let _ = result;
        }
    }

    /// Test scanner with two-byte sequences
    #[test]
    fn scanner_two_bytes(b1 in prop::num::u8::ANY, b2 in prop::num::u8::ANY) {
        let input = String::from_utf8_lossy(&[b1, b2]).into_owned();
        let scanner = gors::scanner::Scanner::new("test.go", &input);
        for result in scanner {
            let _ = result;
        }
    }
}
