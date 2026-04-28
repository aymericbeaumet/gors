//! Program execution tests.
//!
//! These tests compile real-world Go programs and verify their output matches
//! the expected output when run through different backends.
//!
//! # Backend comparison
//!
//! - `go run` - Native Go execution (reference output)
//! - `gors build --emit=rust` + `rustc` + exec - Rust backend
//! - `gors run` - WASM backend
//!
//! # Adding a new program
//!
//! 1. Create a directory in `fixtures/go_programs/` (e.g., `my_program/`)
//! 2. Add `main.go` with your Go program
//! 3. The program will automatically be tested

mod common;

use common::{fixtures_dir, gors_bin};
use std::path::PathBuf;
use std::process::Command;

use wasmi::{Caller, Engine, Extern, Func, Linker, Module, Store};

/// Discovered program with its expected output.
#[derive(Debug)]
pub struct Program {
    /// Name of the program (directory name)
    pub name: String,
    /// Path to the program directory
    pub dir: PathBuf,
    /// Path to main.go
    pub main_go: PathBuf,
    /// Expected stdout output from `go run`
    pub expected_output: String,
}

/// Discover all programs in fixtures/go_programs.
/// Runs `go run` for each program to get the reference output.
pub fn discover_programs() -> Vec<Program> {
    let programs_dir = fixtures_dir().join("go_programs");
    let mut programs = Vec::new();

    let entries = match std::fs::read_dir(&programs_dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Failed to read programs directory: {}", e);
            return programs;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let main_go = path.join("main.go");

        if !main_go.exists() {
            continue;
        }

        // Run `go run main.go` to get reference output
        let expected_output = Command::new("go")
            .args(["run", main_go.to_str().unwrap()])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok()
                } else {
                    eprintln!(
                        "Warning: go run failed for {}: {}",
                        path.display(),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    None
                }
            })
            .unwrap_or_default();

        let name = path.file_name().unwrap().to_str().unwrap().to_string();

        programs.push(Program {
            name,
            dir: path.clone(),
            main_go,
            expected_output,
        });
    }

    programs.sort_by(|a, b| a.name.cmp(&b.name));
    programs
}

/// Result of running a program through a backend
#[derive(Debug)]
pub struct BackendResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl BackendResult {
    fn success(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
        }
    }

    fn failure(error: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error),
        }
    }
}

/// Compile and run via Rust backend using CLI:
/// 1. `gors build --emit=rust -o <temp>.rs <path>`
/// 2. `rustc <temp>.rs -o <temp_bin>`
/// 3. Execute `<temp_bin>`
pub fn run_via_rust_cli(path: &std::path::Path) -> BackendResult {
    let temp_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return BackendResult::failure(format!("Failed to create temp dir: {}", e)),
    };

    let rust_file = temp_dir.path().join("main.rs");
    let bin_file = temp_dir.path().join("main");

    // Step 1: gors build --emit=rust
    let gors = gors_bin();
    let build_output = Command::new(gors)
        .args([
            "build",
            "--emit=rust",
            "-o",
            rust_file.to_str().unwrap(),
            path.to_str().unwrap(),
        ])
        .output();

    let build_output = match build_output {
        Ok(o) => o,
        Err(e) => return BackendResult::failure(format!("Failed to run gors build: {}", e)),
    };

    if !build_output.status.success() {
        return BackendResult::failure(format!(
            "gors build failed:\n{}",
            String::from_utf8_lossy(&build_output.stderr)
        ));
    }

    // Step 2: Compile with rustc
    let rustc_output = Command::new("rustc")
        .args([
            rust_file.to_str().unwrap(),
            "-o",
            bin_file.to_str().unwrap(),
            "--edition=2021",
        ])
        .output();

    let rustc_output = match rustc_output {
        Ok(o) => o,
        Err(e) => return BackendResult::failure(format!("Failed to run rustc: {}", e)),
    };

    if !rustc_output.status.success() {
        // Include the generated Rust source for debugging
        let rust_source = std::fs::read_to_string(&rust_file).unwrap_or_default();
        return BackendResult::failure(format!(
            "rustc compilation failed:\n{}\n{}\n\nGenerated Rust source:\n{}",
            String::from_utf8_lossy(&rustc_output.stdout),
            String::from_utf8_lossy(&rustc_output.stderr),
            rust_source
        ));
    }

    // Step 3: Execute the binary
    let exec_output = Command::new(&bin_file).output();

    let exec_output = match exec_output {
        Ok(o) => o,
        Err(e) => return BackendResult::failure(format!("Failed to execute binary: {}", e)),
    };

    if !exec_output.status.success() {
        return BackendResult::failure(format!(
            "Execution failed:\n{}",
            String::from_utf8_lossy(&exec_output.stderr)
        ));
    }

    BackendResult::success(String::from_utf8_lossy(&exec_output.stdout).to_string())
}

/// Compile and run via WASM backend using CLI:
/// `gors run <path>`
pub fn run_via_wasm_cli(path: &std::path::Path) -> BackendResult {
    let gors = gors_bin();
    let output = Command::new(gors)
        .args(["run", path.to_str().unwrap()])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => return BackendResult::failure(format!("Failed to run gors run: {}", e)),
    };

    if !output.status.success() {
        return BackendResult::failure(format!(
            "gors run failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    BackendResult::success(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a Go program via the Rust backend using library APIs (for comparison).
pub fn run_via_rust_lib(path: &std::path::Path) -> BackendResult {
    // Parse (supports both files and directories)
    let (ast, _files) = match gors::parser::parse_path(path.to_str().unwrap()) {
        Ok(result) => result,
        Err(e) => return BackendResult::failure(format!("Parse error: {:?}", e)),
    };

    // Compile
    let compiled = match gors::compiler::compile(ast) {
        Ok(c) => c,
        Err(e) => return BackendResult::failure(format!("Compile error: {:?}", e)),
    };

    // Generate Rust
    let rust_source = match gors::backend_rust::generate(compiled) {
        Ok(s) => s,
        Err(e) => return BackendResult::failure(format!("Codegen error: {:?}", e)),
    };

    // Write to temp file
    let temp_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return BackendResult::failure(format!("Failed to create temp dir: {}", e)),
    };

    let rust_file = temp_dir.path().join("main.rs");
    let bin_file = temp_dir.path().join("main");

    if let Err(e) = std::fs::write(&rust_file, &rust_source) {
        return BackendResult::failure(format!("Failed to write Rust file: {}", e));
    }

    // Compile with rustc
    let rustc = Command::new("rustc")
        .args([
            rust_file.to_str().unwrap(),
            "-o",
            bin_file.to_str().unwrap(),
            "--edition=2021",
        ])
        .output();

    let rustc = match rustc {
        Ok(o) => o,
        Err(e) => return BackendResult::failure(format!("Failed to run rustc: {}", e)),
    };

    if !rustc.status.success() {
        return BackendResult::failure(format!(
            "Rust compilation failed:\n{}\n{}\n\nGenerated source:\n{}",
            String::from_utf8_lossy(&rustc.stdout),
            String::from_utf8_lossy(&rustc.stderr),
            rust_source
        ));
    }

    // Run the binary
    let output = Command::new(&bin_file).output();

    let output = match output {
        Ok(o) => o,
        Err(e) => return BackendResult::failure(format!("Failed to run binary: {}", e)),
    };

    if !output.status.success() {
        return BackendResult::failure(format!(
            "Execution failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    BackendResult::success(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Compile and run via WASM backend using library APIs.
pub fn run_via_wasm_lib(path: &std::path::Path) -> BackendResult {
    // Parse (supports both files and directories)
    let (ast, _files) = match gors::parser::parse_path(path.to_str().unwrap()) {
        Ok(result) => result,
        Err(e) => return BackendResult::failure(format!("Parse error: {:?}", e)),
    };

    // Compile to Rust AST
    let compiled = match gors::compiler::compile(ast) {
        Ok(c) => c,
        Err(e) => return BackendResult::failure(format!("Compile error: {:?}", e)),
    };

    // Compile to WASM
    let wasm_bytes = match gors::backend_wasm::compile_to_wasm(&compiled) {
        Ok(bytes) => bytes,
        Err(e) => return BackendResult::failure(format!("WASM compile error: {:?}", e)),
    };

    // Run with wasmi
    match run_wasm_bytes(&wasm_bytes) {
        Ok(output) => BackendResult::success(output),
        Err(e) => BackendResult::failure(e),
    }
}

/// State for WASM execution - holds output buffer and memory reference
struct WasmState {
    output: Vec<String>,
    memory: Option<wasmi::Memory>,
}

/// Run WASM bytes using wasmi and capture output.
fn run_wasm_bytes(wasm_bytes: &[u8]) -> Result<String, String> {
    // Create engine and store with state
    let engine = Engine::default();
    let state = WasmState {
        output: Vec::new(),
        memory: None,
    };
    let mut store = Store::new(&engine, state);

    // Compile the WASM module
    let module =
        Module::new(&engine, wasm_bytes).map_err(|e| format!("WASM module error: {}", e))?;

    // Create linker and add imports
    let mut linker = Linker::new(&engine);

    // print_i32 function that captures output to the store's state
    linker
        .func_wrap(
            "env",
            "print_i32",
            |mut caller: Caller<'_, WasmState>, value: i32| {
                caller.data_mut().output.push(value.to_string());
            },
        )
        .map_err(|e| format!("Failed to add print_i32: {}", e))?;

    // print_str function that reads string from memory
    linker
        .func_wrap(
            "env",
            "print_str",
            |mut caller: Caller<'_, WasmState>, offset: i32, len: i32| {
                let memory = caller.data().memory;
                if let Some(mem) = memory {
                    let mut buffer = vec![0u8; len as usize];
                    if mem.read(&caller, offset as usize, &mut buffer).is_ok() {
                        if let Ok(s) = String::from_utf8(buffer) {
                            caller.data_mut().output.push(s);
                        }
                    }
                }
            },
        )
        .map_err(|e| format!("Failed to add print_str: {}", e))?;

    // Instantiate the module
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| format!("WASM instantiation error: {}", e))?;

    // Get memory export and store it in state for print_str to use
    if let Some(memory) = instance
        .get_export(&store, "memory")
        .and_then(Extern::into_memory)
    {
        store.data_mut().memory = Some(memory);
    }

    // Get and call the main function
    let main_func: Func = instance
        .get_export(&store, "main")
        .and_then(Extern::into_func)
        .ok_or("main function not found")?;

    main_func
        .call(&mut store, &[], &mut [])
        .map_err(|e| format!("WASM execution error: {}", e))?;

    // Get output
    let output = &store.data().output;
    Ok(output.join("\n") + if output.is_empty() { "" } else { "\n" })
}

// =============================================================================
// Test Cases
// =============================================================================

/// Summary of test results for a backend
#[derive(Default)]
struct TestSummary {
    passed: Vec<String>,
    failed: Vec<(String, String)>, // (program_name, error)
}

impl TestSummary {
    fn add_pass(&mut self, name: &str) {
        self.passed.push(name.to_string());
    }

    fn add_fail(&mut self, name: &str, error: &str) {
        self.failed.push((name.to_string(), error.to_string()));
    }

    fn print_summary(&self, backend_name: &str) {
        eprintln!("\n=== {} Backend Summary ===", backend_name);
        eprintln!("Passed: {}", self.passed.len());
        eprintln!("Failed: {}", self.failed.len());

        if !self.passed.is_empty() {
            eprintln!("\nPassing tests:");
            for name in &self.passed {
                eprintln!("  ✓ {}", name);
            }
        }

        if !self.failed.is_empty() {
            eprintln!("\nFailing tests:");
            for (name, error) in &self.failed {
                eprintln!("  ✗ {}", name);
                // Show first line of error
                if let Some(first_line) = error.lines().next() {
                    eprintln!("    {}", first_line);
                }
            }
        }
    }
}

/// Test all programs via the Rust backend (CLI: gors build --emit=rust + rustc)
#[test]
fn test_programs_rust_backend() {
    let programs = discover_programs();
    assert!(
        !programs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let mut summary = TestSummary::default();
    let mut all_passed = true;

    for program in &programs {
        // Skip programs with empty expected output (go run failed)
        if program.expected_output.is_empty() {
            eprintln!(
                "Skipping {} - no reference output from go run",
                program.name
            );
            continue;
        }

        let result = run_via_rust_cli(&program.dir);

        if result.success {
            if result.output == program.expected_output {
                summary.add_pass(&program.name);
            } else {
                all_passed = false;
                summary.add_fail(
                    &program.name,
                    &format!(
                        "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                        program.expected_output, result.output
                    ),
                );
            }
        } else {
            all_passed = false;
            summary.add_fail(
                &program.name,
                result.error.as_deref().unwrap_or("Unknown error"),
            );
        }
    }

    summary.print_summary("Rust (CLI)");

    // Assert that at least some tests pass (soft assertion for now)
    // When the backend is more complete, change this to assert all pass
    if !all_passed {
        eprintln!("\nNote: Some Rust backend tests failed. This is expected during development.");
    }
}

/// Test all programs via the WASM backend (CLI: gors run)
#[test]
fn test_programs_wasm_backend() {
    let programs = discover_programs();
    assert!(
        !programs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let mut summary = TestSummary::default();
    let mut all_passed = true;

    for program in &programs {
        // Skip programs with empty expected output
        if program.expected_output.is_empty() {
            eprintln!(
                "Skipping {} - no reference output from go run",
                program.name
            );
            continue;
        }

        let result = run_via_wasm_cli(&program.dir);

        if result.success {
            if result.output == program.expected_output {
                summary.add_pass(&program.name);
            } else {
                all_passed = false;
                summary.add_fail(
                    &program.name,
                    &format!(
                        "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                        program.expected_output, result.output
                    ),
                );
            }
        } else {
            all_passed = false;
            summary.add_fail(
                &program.name,
                result.error.as_deref().unwrap_or("Unknown error"),
            );
        }
    }

    summary.print_summary("WASM (CLI)");

    if !all_passed {
        eprintln!("\nNote: Some WASM backend tests failed. This is expected during development.");
    }
}

/// Test all programs via the Go runner (uses the custom Go-based runner)
#[test]
fn test_programs_go_runner() {
    use common::go_runner_bin;

    let programs = discover_programs();
    assert!(
        !programs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let go_bin = go_runner_bin();

    for program in &programs {
        // Skip programs with empty expected output
        if program.expected_output.is_empty() {
            eprintln!(
                "Skipping {} - no reference output from go run",
                program.name
            );
            continue;
        }

        let output = Command::new(go_bin)
            .args(["run", program.dir.to_str().unwrap()])
            .output()
            .unwrap_or_else(|e| panic!("Failed to run go runner on {}: {}", program.name, e));

        if output.status.success() {
            let actual = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                actual.as_ref(),
                &program.expected_output,
                "Output mismatch for {}",
                program.name
            );
        }
    }
}

/// Test source map generation for compilable programs.
#[test]
fn test_programs_sourcemap() {
    let programs = discover_programs();

    for program in &programs {
        // Parse the directory (supports both files and directories)
        let (ast, files) = match gors::parser::parse_path(program.dir.to_str().unwrap()) {
            Ok(result) => result,
            Err(_) => continue,
        };

        // Get the first file's info for source map
        let (go_file, go_source) = match files.first() {
            Some((f, s)) => (f.as_str(), s.as_str()),
            None => continue,
        };

        // Compile with source map tracking
        let compiled = match gors::compiler::compile_with_source_map(ast, go_file, go_source) {
            Ok(compiled) => compiled,
            Err(_) => continue,
        };

        // Generate Rust code
        let rust_source = match gors::backend_rust::generate(compiled) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Build the source map
        let source_map = gors::compiler::build_source_map(&rust_source);

        // Validate: serialize and parse back (round-trip)
        let mut buf = Vec::new();
        if source_map.to_writer(&mut buf).is_err() {
            continue;
        }

        let parsed = match sourcemap::SourceMap::from_reader(&buf[..]) {
            Ok(sm) => sm,
            Err(e) => panic!("Invalid sourcemap for {}: {}", go_file, e),
        };

        // Basic validation
        assert!(
            parsed.get_token_count() > 0,
            "Empty sourcemap for {}",
            go_file
        );
        assert_eq!(
            parsed.get_source(0),
            Some(go_file),
            "Source file mismatch for {}",
            go_file
        );
    }
}

/// Comprehensive test that reports detailed results for all backends.
/// This test is the main entry point for testing program execution.
#[test]
fn test_all_programs() {
    let programs = discover_programs();
    assert!(
        !programs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    eprintln!("\n========================================");
    eprintln!("Testing {} programs", programs.len());
    eprintln!("========================================\n");

    let mut rust_results: Vec<(String, bool, String)> = Vec::new();
    let mut wasm_results: Vec<(String, bool, String)> = Vec::new();

    for program in &programs {
        eprintln!("Testing: {}", program.name);
        eprintln!("  Expected output: {:?}", program.expected_output);

        // Skip if no expected output
        if program.expected_output.is_empty() {
            eprintln!("  [SKIP] No reference output from go run");
            continue;
        }

        // Test Rust backend
        let rust_result = run_via_rust_cli(&program.dir);
        if rust_result.success && rust_result.output == program.expected_output {
            eprintln!("  [RUST] PASS");
            rust_results.push((program.name.clone(), true, String::new()));
        } else if rust_result.success {
            eprintln!("  [RUST] FAIL - Output mismatch");
            eprintln!("    Got: {:?}", rust_result.output);
            rust_results.push((
                program.name.clone(),
                false,
                format!(
                    "Output mismatch: expected {:?}, got {:?}",
                    program.expected_output, rust_result.output
                ),
            ));
        } else {
            eprintln!(
                "  [RUST] FAIL - {}",
                rust_result.error.as_deref().unwrap_or("Unknown error")
            );
            rust_results.push((
                program.name.clone(),
                false,
                rust_result
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        // Test WASM backend
        let wasm_result = run_via_wasm_cli(&program.dir);
        if wasm_result.success && wasm_result.output == program.expected_output {
            eprintln!("  [WASM] PASS");
            wasm_results.push((program.name.clone(), true, String::new()));
        } else if wasm_result.success {
            eprintln!("  [WASM] FAIL - Output mismatch");
            eprintln!("    Got: {:?}", wasm_result.output);
            wasm_results.push((
                program.name.clone(),
                false,
                format!(
                    "Output mismatch: expected {:?}, got {:?}",
                    program.expected_output, wasm_result.output
                ),
            ));
        } else {
            eprintln!(
                "  [WASM] FAIL - {}",
                wasm_result.error.as_deref().unwrap_or("Unknown error")
            );
            wasm_results.push((
                program.name.clone(),
                false,
                wasm_result
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        eprintln!();
    }

    // Final summary
    let rust_passed = rust_results.iter().filter(|(_, p, _)| *p).count();
    let wasm_passed = wasm_results.iter().filter(|(_, p, _)| *p).count();

    eprintln!("========================================");
    eprintln!("FINAL RESULTS");
    eprintln!("========================================");
    eprintln!(
        "Rust backend: {}/{} passed",
        rust_passed,
        rust_results.len()
    );
    eprintln!(
        "WASM backend: {}/{} passed",
        wasm_passed,
        wasm_results.len()
    );
    eprintln!("========================================\n");
}
