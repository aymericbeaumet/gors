//! Unified codegen tests for all backends (Rust and WASM).
//!
//! These tests verify that Go code can be compiled through both the Rust and WASM
//! code generation backends.

use gors::codegen::{rust, wasm};

/// Test scenario that can be run against multiple backends.
pub struct CodegenScenario {
    /// Name of the test
    pub name: &'static str,
    /// Go source code
    pub go_source: &'static str,
    /// Whether this test should compile successfully
    pub should_compile: bool,
    /// WASM-specific checks (only run if should_compile is true)
    pub wasm_checks: Option<OutputChecks>,
    /// Rust-specific checks (only run if should_compile is true)
    pub rust_checks: Option<OutputChecks>,
    /// Which backends to test
    pub backends: Backends,
}

/// Which backends to test for a scenario.
#[derive(Clone, Copy)]
pub enum Backends {
    /// Test both WASM and Rust
    All,
    /// Test only Rust
    RustOnly,
    /// Test only WASM
    WasmOnly,
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
            wasm_checks: None,
            rust_checks: None,
            backends: Backends::All,
        }
    }

    /// Mark this test as expected to fail.
    pub fn should_fail(mut self) -> Self {
        self.should_compile = false;
        self
    }

    /// Only test Rust backend.
    pub fn rust_only(mut self) -> Self {
        self.backends = Backends::RustOnly;
        self
    }

    /// Only test WASM backend.
    pub fn wasm_only(mut self) -> Self {
        self.backends = Backends::WasmOnly;
        self
    }

    /// Add patterns that must be present in WASM output.
    pub fn wasm_contains(mut self, patterns: Vec<&'static str>) -> Self {
        let checks = self.wasm_checks.get_or_insert(OutputChecks {
            must_contain: vec![],
            must_not_contain: vec![],
        });
        checks.must_contain.extend(patterns);
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

    /// Run this test scenario against all configured backends.
    pub fn run(&self) {
        match self.backends {
            Backends::All => {
                self.run_rust();
                self.run_wasm();
            }
            Backends::RustOnly => {
                self.run_rust();
            }
            Backends::WasmOnly => {
                self.run_wasm();
            }
        }
    }

    /// Run against Rust backend only.
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

    /// Run against WASM backend only.
    pub fn run_wasm(&self) {
        let result = compile_go_to_wasm(self.go_source);

        if self.should_compile {
            let wat_output = result.unwrap_or_else(|e| {
                panic!("[{}] WASM compilation should succeed: {}", self.name, e)
            });

            // Validate WAT syntax
            validate_wat(&wat_output)
                .unwrap_or_else(|e| panic!("[{}] WAT should be valid: {}", self.name, e));

            if let Some(checks) = &self.wasm_checks {
                for pattern in &checks.must_contain {
                    assert!(
                        wat_output.contains(pattern),
                        "[{}] WASM output should contain '{}'\nOutput:\n{}",
                        self.name,
                        pattern,
                        wat_output
                    );
                }
                for pattern in &checks.must_not_contain {
                    assert!(
                        !wat_output.contains(pattern),
                        "[{}] WASM output should NOT contain '{}'\nOutput:\n{}",
                        self.name,
                        pattern,
                        wat_output
                    );
                }
            }
        } else {
            assert!(
                result.is_err(),
                "[{}] WASM compilation should fail",
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

    rust::generate(compiled).map_err(|e| format!("Rust codegen error: {:?}", e))
}

/// Compile Go code to WAT.
fn compile_go_to_wasm(go_source: &str) -> Result<String, String> {
    let ast = gors::parser::parse_file("test.go", go_source)
        .map_err(|e| format!("Parse error: {:?}", e))?;

    let compiled =
        gors::compiler::compile(ast).map_err(|e| format!("Compile error: {:?}", e))?;

    wasm::generate(compiled).map_err(|e| format!("WASM codegen error: {:?}", e))
}

/// Validate WAT text can be parsed.
fn validate_wat(wat: &str) -> Result<(), String> {
    wat::parse_str(wat).map_err(|e| format!("WAT validation error: {}", e))?;
    Ok(())
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
    .wasm_contains(vec!["(module", "(func", "(export \"main\""])
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
    .wasm_contains(vec![
        "(import \"env\" \"print\"",
        "Hello, World!",
        "(export \"memory\"",
    ])
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
    .wasm_contains(vec!["(param", "(result i32)", "i32.add"])
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
    .wasm_contains(vec!["(local", "i32.const 42", "local.set"])
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
    .wasm_contains(vec!["if", "i32.gt_s"])
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
    .wasm_contains(vec!["if", "else"])
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
    .wasm_contains(vec!["block", "loop", "br_if", "br "])
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
    .wasm_contains(vec!["block", "loop", "br_if"])
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
    .wasm_contains(vec!["(func"])
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
    .wasm_contains(vec!["(result i32)", "i32.const 42"])
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
    .wasm_contains(vec!["data"])
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
    .wasm_contains(vec![
        "i32.add", "i32.sub", "i32.mul", "i32.div_s", "i32.rem_s",
    ])
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
    .wasm_contains(vec![
        "i32.lt_s", "i32.gt_s", "i32.le_s", "i32.ge_s", "i32.eq", "i32.ne",
    ])
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
    .wasm_contains(vec!["i32.add", "i32.sub", "i32.mul"])
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
    .wasm_contains(vec!["loop"])
    .rust_contains(vec!["while"])
    .run();
}

#[test]
fn test_unsupported_function_error() {
    CodegenScenario::new(
        "unsupported_function",
        r#"package main

import "fmt"

func main() {
    s := fmt.Sprintf("test %d", 42)
}
"#,
    )
    .should_fail()
    .wasm_only()
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
    .wasm_contains(vec!["(param", "i32.mul", "(result i32)"])
    .rust_contains(vec!["fn multiply", "x: isize", "y: isize", "-> isize"])
    .run();
}

#[test]
fn test_logical_operators() {
    // Note: WASM backend doesn't support boolean literals yet, so only test Rust
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
    .rust_only()
    .run();
}

#[test]
fn test_unary_operators() {
    // Note: WASM backend doesn't support boolean literals yet, so only test Rust
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
    .rust_only()
    .run();
}

#[test]
fn test_multiple_variable_declaration() {
    // Note: WASM backend doesn't support tuple patterns yet, so only test Rust
    CodegenScenario::new(
        "multiple_variable_declaration",
        r#"package main

func main() {
    a, b := 1, 2
}
"#,
    )
    .rust_contains(vec!["let", "mut"])
    .rust_only()
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
    .wasm_contains(vec!["i32.add", "i32.sub"])
    .rust_contains(vec!["+=", "-="])
    .run();
}
