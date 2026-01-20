//! Codegen tests for the Rust backend.
//!
//! These tests verify that Go code can be compiled through the Rust
//! code generation backend.

use gors::codegen;

/// Test scenario that runs against the Rust backend.
pub struct CodegenScenario {
    /// Name of the test
    pub name: &'static str,
    /// Go source code
    pub go_source: &'static str,
    /// Whether this test should compile successfully
    pub should_compile: bool,
    /// Rust-specific checks (only run if should_compile is true)
    pub rust_checks: Option<OutputChecks>,
}

/// Checks for output content.
pub struct OutputChecks {
    /// Strings that must be present in the output
    pub must_contain: Vec<&'static str>,
    /// Strings that must NOT be present in the output
    pub must_not_contain: Vec<&'static str>,
}

impl CodegenScenario {
    /// Create a new test scenario.
    pub fn new(name: &'static str, go_source: &'static str) -> Self {
        Self {
            name,
            go_source,
            should_compile: true,
            rust_checks: None,
        }
    }

    /// Mark this test as expected to fail.
    pub fn should_fail(mut self) -> Self {
        self.should_compile = false;
        self
    }

    /// Add patterns that must be present in Rust output.
    pub fn rust_contains(mut self, patterns: Vec<&'static str>) -> Self {
        let checks = self.rust_checks.get_or_insert(OutputChecks {
            must_contain: vec![],
            must_not_contain: vec![],
        });
        checks.must_contain.extend(patterns);
        self
    }

    /// Run this test scenario.
    pub fn run(&self) {
        self.run_rust();
    }

    /// Run against Rust backend.
    pub fn run_rust(&self) {
        let result = compile_go_to_rust(self.go_source);

        if self.should_compile {
            let rust_output = result.unwrap_or_else(|e| {
                panic!("[{}] Rust compilation should succeed: {}", self.name, e)
            });

            if let Some(checks) = &self.rust_checks {
                for pattern in &checks.must_contain {
                    assert!(
                        rust_output.contains(pattern),
                        "[{}] Rust output should contain '{}'\nOutput:\n{}",
                        self.name,
                        pattern,
                        rust_output
                    );
                }
                for pattern in &checks.must_not_contain {
                    assert!(
                        !rust_output.contains(pattern),
                        "[{}] Rust output should NOT contain '{}'\nOutput:\n{}",
                        self.name,
                        pattern,
                        rust_output
                    );
                }
            }
        } else {
            assert!(
                result.is_err(),
                "[{}] Rust compilation should fail",
                self.name
            );
        }
    }
}

/// Compile Go code to Rust source.
fn compile_go_to_rust(go_source: &str) -> Result<String, String> {
    let ast = gors::parser::parse_file("test.go", go_source)
        .map_err(|e| format!("Parse error: {:?}", e))?;

    let compiled =
        gors::compiler::compile(ast).map_err(|e| format!("Compile error: {:?}", e))?;

    codegen::generate(compiled).map_err(|e| format!("Rust codegen error: {:?}", e))
}

// =============================================================================
// Test Cases
// =============================================================================

#[test]
fn test_empty_main() {
    CodegenScenario::new(
        "empty_main",
        r#"package main

func main() {
}
"#,
    )
    .rust_contains(vec!["fn main()", "pub"])
    .run();
}

#[test]
fn test_println_hello() {
    CodegenScenario::new(
        "println_hello",
        r#"package main

import "fmt"

func main() {
    fmt.Println("Hello, World!")
}
"#,
    )
    .rust_contains(vec!["println!", "Hello, World!"])
    .run();
}

#[test]
fn test_simple_arithmetic() {
    CodegenScenario::new(
        "simple_arithmetic",
        r#"package main

func add(a int, b int) int {
    return a + b
}
"#,
    )
    .rust_contains(vec!["fn add", "a + b", "-> isize"])
    .run();
}

#[test]
fn test_local_variable() {
    CodegenScenario::new(
        "local_variable",
        r#"package main

func main() {
    x := 42
}
"#,
    )
    .rust_contains(vec!["let mut x", "42"])
    .run();
}

#[test]
fn test_if_statement() {
    CodegenScenario::new(
        "if_statement",
        r#"package main

func main() {
    x := 1
    if x > 0 {
        y := 2
    }
}
"#,
    )
    .rust_contains(vec!["if", ">"])
    .run();
}

#[test]
fn test_if_else_statement() {
    CodegenScenario::new(
        "if_else_statement",
        r#"package main

func main() {
    x := 1
    if x > 0 {
        y := 2
    } else {
        y := 3
    }
}
"#,
    )
    .rust_contains(vec!["if", "else"])
    .run();
}

#[test]
fn test_while_loop() {
    CodegenScenario::new(
        "while_loop",
        r#"package main

func main() {
    i := 0
    for i < 10 {
        i = i + 1
    }
}
"#,
    )
    .rust_contains(vec!["while", "<"])
    .run();
}

#[test]
fn test_for_loop_with_init_cond_post() {
    CodegenScenario::new(
        "for_loop_with_init_cond_post",
        r#"package main

import "fmt"

func main() {
    for i := 0; i < 5; i++ {
        fmt.Println("loop")
    }
}
"#,
    )
    .rust_contains(vec!["let mut i", "while", "+="])
    .run();
}

#[test]
fn test_multiple_functions() {
    CodegenScenario::new(
        "multiple_functions",
        r#"package main

func helper() int {
    return 42
}

func main() {
    x := helper()
}
"#,
    )
    .rust_contains(vec!["fn helper", "fn main"])
    .run();
}

#[test]
fn test_return_value() {
    CodegenScenario::new(
        "return_value",
        r#"package main

func answer() int {
    return 42
}
"#,
    )
    .rust_contains(vec!["-> isize", "42"])
    .run();
}

#[test]
fn test_unicode_string() {
    CodegenScenario::new(
        "unicode_string",
        r#"package main

import "fmt"

func main() {
    fmt.Println("Hello, 世界")
}
"#,
    )
    .rust_contains(vec!["世界"])
    .run();
}

#[test]
fn test_binary_operators() {
    CodegenScenario::new(
        "binary_operators",
        r#"package main

func main() {
    a := 10
    b := 3
    c := a + b
    d := a - b
    e := a * b
    f := a / b
    g := a % b
}
"#,
    )
    .rust_contains(vec!["+", "-", "*", "/", "%"])
    .run();
}

#[test]
fn test_comparison_operators() {
    CodegenScenario::new(
        "comparison_operators",
        r#"package main

func main() {
    a := 5
    b := 10
    if a < b {
    }
    if a > b {
    }
    if a <= b {
    }
    if a >= b {
    }
    if a == b {
    }
    if a != b {
    }
}
"#,
    )
    .rust_contains(vec!["<", ">", "<=", ">=", "==", "!="])
    .run();
}

#[test]
fn test_compound_assignment() {
    CodegenScenario::new(
        "compound_assignment",
        r#"package main

func main() {
    x := 5
    x += 3
    x -= 2
    x *= 4
}
"#,
    )
    .rust_contains(vec!["+=", "-=", "*="])
    .run();
}

#[test]
fn test_nested_for_loops() {
    CodegenScenario::new(
        "nested_for_loops",
        r#"package main

import "fmt"

func main() {
    for i := 0; i < 2; i++ {
        for j := 0; j < 3; j++ {
            fmt.Println("nested")
        }
    }
}
"#,
    )
    .rust_contains(vec!["while"])
    .run();
}

#[test]
fn test_function_with_parameters() {
    CodegenScenario::new(
        "function_with_parameters",
        r#"package main

func multiply(x int, y int) int {
    return x * y
}
"#,
    )
    .rust_contains(vec!["fn multiply", "x: isize", "y: isize", "-> isize"])
    .run();
}

#[test]
fn test_logical_operators() {
    CodegenScenario::new(
        "logical_operators",
        r#"package main

func main() {
    a := true
    b := false
    if a && b {
    }
    if a || b {
    }
}
"#,
    )
    .rust_contains(vec!["&&", "||"])
    .run();
}

#[test]
fn test_unary_operators() {
    CodegenScenario::new(
        "unary_operators",
        r#"package main

func main() {
    x := 5
    y := -x
    b := true
    c := !b
}
"#,
    )
    .rust_contains(vec!["-x", "!b"])
    .run();
}

#[test]
fn test_multiple_variable_declaration() {
    CodegenScenario::new(
        "multiple_variable_declaration",
        r#"package main

func main() {
    a, b := 1, 2
}
"#,
    )
    .rust_contains(vec!["let", "mut"])
    .run();
}

#[test]
fn test_increment_decrement() {
    CodegenScenario::new(
        "increment_decrement",
        r#"package main

func main() {
    i := 0
    i++
    i--
}
"#,
    )
    .rust_contains(vec!["+=", "-="])
    .run();
}
