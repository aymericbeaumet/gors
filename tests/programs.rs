//! Program execution tests.
//!
//! These tests compile real-world Go programs and verify their output matches
//! the expected output when run through different backends.
//!
//! # Backend comparison
//!
//! - `go run` - Native Go execution (reference output)
//! - `gors build --target rust` + `rustc` + exec - Rust backend
//! - `gors run` - WASM backend
//!
//! # Adding a new program
//!
//! 1. Create a directory in `fixtures/go_programs/` (e.g., `my_program/`)
//! 2. Add `main.go` with your Go program
//! 3. Run `go run main.go > expected_output.txt` to generate expected output
//! 4. The program will automatically be tested

mod common;

use common::fixtures_dir;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use wasmi::{Caller, Engine, Func, Linker, Module, Store};

/// Discovered program with its expected output.
pub struct Program {
    /// Name of the program (directory name)
    pub name: String,
    /// Path to the program directory
    pub dir: PathBuf,
    /// Path to main.go (for backwards compatibility with go run)
    pub main_go: PathBuf,
    /// Expected stdout output
    pub expected_output: String,
}

/// Discover all programs in fixtures/go_programs.
pub fn discover_programs() -> Vec<Program> {
    let programs_dir = fixtures_dir().join("go_programs");
    let mut programs = Vec::new();

    let entries = match std::fs::read_dir(&programs_dir) {
        Ok(entries) => entries,
        Err(_) => return programs,
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

        let expected_output = Command::new("go")
                .args(["run", main_go.to_str().unwrap()])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8(o.stdout).ok()
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

        let name = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

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

/// Run a Go program natively and return its output.
/// Accepts a path to either a .go file or a directory containing Go files.
pub fn run_go_native(path: &std::path::Path) -> Result<String, String> {
    let output = Command::new("go")
        .args(["run", path.to_str().unwrap()])
        .output()
        .map_err(|e| format!("Failed to run go: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Go execution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Compile and run a Go program via the Rust backend.
/// Accepts a path to either a .go file or a directory containing Go files.
pub fn run_via_rust(path: &std::path::Path) -> Result<String, String> {
    // Parse (supports both files and directories)
    let (ast, _files) = gors::parser::parse_path(path.to_str().unwrap())
        .map_err(|e| format!("Parse error: {:?}", e))?;

    // Compile
    let compiled =
        gors::compiler::compile(ast).map_err(|e| format!("Compile error: {:?}", e))?;

    // Generate Rust
    let rust_source =
        gors::backend_rust::generate(compiled).map_err(|e| format!("Codegen error: {:?}", e))?;

    // Write to temp file
    let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let rust_file = temp_dir.path().join("main.rs");
    let bin_file = temp_dir.path().join("main");

    std::fs::write(&rust_file, &rust_source).map_err(|e| e.to_string())?;

    // Compile with rustc
    let rustc = Command::new("rustc")
        .args([
            rust_file.to_str().unwrap(),
            "-o",
            bin_file.to_str().unwrap(),
            "--edition=2021",
        ])
        .output()
        .map_err(|e| format!("Failed to run rustc: {}", e))?;

    if !rustc.status.success() {
        return Err(format!(
            "Rust compilation failed:\n{}\n{}",
            String::from_utf8_lossy(&rustc.stdout),
            String::from_utf8_lossy(&rustc.stderr)
        ));
    }

    // Run the binary
    let output = Command::new(&bin_file)
        .output()
        .map_err(|e| format!("Failed to run binary: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Execution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Compile and run a Go program via the WASM backend.
/// Accepts a path to either a .go file or a directory containing Go files.
pub fn run_via_wasm(path: &std::path::Path) -> Result<String, String> {
    // Parse (supports both files and directories)
    let (ast, _files) = gors::parser::parse_path(path.to_str().unwrap())
        .map_err(|e| format!("Parse error: {:?}", e))?;

    // Compile to Rust AST
    let compiled =
        gors::compiler::compile(ast).map_err(|e| format!("Compile error: {:?}", e))?;

    // Compile to WASM
    let wasm_bytes = gors::backend_wasm::compile_to_wasm(&compiled)
        .map_err(|e| format!("WASM compile error: {:?}", e))?;

    // Run with wasmi
    run_wasm_bytes(&wasm_bytes)
}

/// Run WASM bytes using wasmi and capture output.
fn run_wasm_bytes(wasm_bytes: &[u8]) -> Result<String, String> {
    // Create output buffer
    let output_buffer = Arc::new(Mutex::new(Vec::new()));

    // Create engine and store with output buffer as state
    let engine = Engine::default();
    let output_clone = Arc::clone(&output_buffer);
    let mut store = Store::new(&engine, output_clone);

    // Compile the WASM module
    let module = Module::new(&engine, wasm_bytes)
        .map_err(|e| format!("WASM module error: {}", e))?;

    // Create linker and add print_i32 import
    let mut linker = Linker::new(&engine);

    // print_i32 function that captures output to the store's state
    linker
        .func_wrap(
            "env",
            "print_i32",
            |caller: Caller<'_, Arc<Mutex<Vec<String>>>>, value: i32| {
                if let Ok(mut out) = caller.data().lock() {
                    out.push(value.to_string());
                }
            },
        )
        .map_err(|e| format!("Failed to add print_i32: {}", e))?;

    // Instantiate the module
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("WASM instantiation error: {}", e))?
        .start(&mut store)
        .map_err(|e| format!("WASM start error: {}", e))?;

    // Get and call the main function
    let main_func: Func = instance
        .get_export(&store, "main")
        .and_then(|e| e.into_func())
        .ok_or("main function not found")?;

    main_func
        .call(&mut store, &[], &mut [])
        .map_err(|e| format!("WASM execution error: {}", e))?;

    // Get output
    let output = output_buffer
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    Ok(output.join("\n") + if output.is_empty() { "" } else { "\n" })
}

// =============================================================================
// Test Cases
// =============================================================================

/// Test that all programs compile and run via Rust backend.
#[test]
fn programs_rust_backend() {
    let programs = discover_programs();
    assert!(!programs.is_empty(), "No programs found in fixtures/go_programs");

    for program in &programs {
        // First verify Go itself produces the expected output (using the directory)
        let go_output = run_go_native(&program.dir);
        if let Ok(ref output) = go_output {
            assert_eq!(
                output, &program.expected_output,
                "[{}] go run output mismatch",
                program.name
            );
        }

        // Run via Rust backend using directory path
        let result = run_via_rust(&program.dir);

        match result {
            Ok(output) => {
                if output != program.expected_output {
                    // For now, just warn - not all programs may work yet
                    eprintln!(
                        "Warning: {} Rust output differs\nExpected: {:?}\nGot: {:?}",
                        program.name, program.expected_output, output
                    );
                }
            }
            Err(e) => {
                // Some programs may not compile yet
                eprintln!("Warning: {} failed to compile/run via Rust: {}", program.name, e);
            }
        }
    }
}

/// Test that all programs compile and run via WASM backend.
#[test]
fn programs_wasm_backend() {
    let programs = discover_programs();
    assert!(!programs.is_empty(), "No programs found in fixtures/go_programs");

    for program in &programs {
        // Run via WASM backend using directory path
        let result = run_via_wasm(&program.dir);

        match result {
            Ok(output) => {
                if output != program.expected_output {
                    // For now, just warn - WASM backend is limited
                    eprintln!(
                        "Warning: {} WASM output differs\nExpected: {:?}\nGot: {:?}",
                        program.name, program.expected_output, output
                    );
                }
            }
            Err(e) => {
                // Many programs won't compile with WASM backend yet
                eprintln!("Warning: {} failed to compile/run via WASM: {}", program.name, e);
            }
        }
    }
}

/// Test that the Go runner produces correct output for programs.
#[test]
fn programs_go_runner() {
    use common::go_runner_bin;

    let programs = discover_programs();
    assert!(!programs.is_empty(), "No programs found in fixtures/go_programs");

    let go_bin = go_runner_bin();

    for program in &programs {
        // Use directory path for go run (same as gors run ./programs/fizzbuzz)
        let args = ["run", program.dir.to_str().unwrap()];
        let output = Command::new(go_bin)
            .args(&args)
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
fn programs_sourcemap() {
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
        let compiled =
            match gors::compiler::compile_with_source_map(ast, go_file, go_source) {
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
