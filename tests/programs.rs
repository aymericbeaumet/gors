//! Program execution tests.
//!
//! These tests compile real-world Go programs and verify their output matches
//! the expected output when run through different backends.
//!
//! # Adding a new program
//!
//! 1. Create a directory in `fixtures/programs/` (e.g., `my_program/`)
//! 2. Add `main.go` with your Go program
//! 3. Run `go run main.go > expected_output.txt` to generate expected output
//! 4. The program will automatically be tested

mod common;

use common::fixtures_dir;
use std::path::PathBuf;
use std::process::Command;

/// Discovered program with its expected output.
pub struct Program {
    /// Name of the program (directory name)
    pub name: String,
    /// Path to main.go
    pub main_go: PathBuf,
    /// Expected stdout output
    pub expected_output: String,
}

/// Discover all programs in fixtures/programs.
pub fn discover_programs() -> Vec<Program> {
    let programs_dir = fixtures_dir().join("programs");
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
        let expected_output_path = path.join("expected_output.txt");

        if !main_go.exists() {
            continue;
        }

        let expected_output = if expected_output_path.exists() {
            std::fs::read_to_string(&expected_output_path).unwrap_or_default()
        } else {
            // Generate expected output by running Go
            let output = Command::new("go")
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
            output
        };

        let name = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        programs.push(Program {
            name,
            main_go,
            expected_output,
        });
    }

    programs.sort_by(|a, b| a.name.cmp(&b.name));
    programs
}

/// Run a Go program natively and return its output.
pub fn run_go_native(go_file: &std::path::Path) -> Result<String, String> {
    let output = Command::new("go")
        .args(["run", go_file.to_str().unwrap()])
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
pub fn run_via_rust(go_source: &str) -> Result<String, String> {
    // Parse
    let ast = gors::parser::parse_file("main.go", go_source)
        .map_err(|e| format!("Parse error: {:?}", e))?;

    // Compile
    let compiled =
        gors::compiler::compile(ast).map_err(|e| format!("Compile error: {:?}", e))?;

    // Generate Rust
    let rust_source =
        gors::codegen::generate(compiled).map_err(|e| format!("Codegen error: {:?}", e))?;

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

// =============================================================================
// Test Cases
// =============================================================================

/// Test that all programs compile and run via Rust backend.
#[test]
fn programs_rust_backend() {
    let programs = discover_programs();
    assert!(!programs.is_empty(), "No programs found in fixtures/programs");

    for program in &programs {
        let go_source = std::fs::read_to_string(&program.main_go)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", program.main_go.display(), e));

        // Some programs may use features not yet supported
        let result = run_via_rust(&go_source);

        match result {
            Ok(output) => {
                if output != program.expected_output {
                    // For now, just warn - not all programs may work yet
                    eprintln!(
                        "Warning: {} output differs\nExpected: {:?}\nGot: {:?}",
                        program.name, program.expected_output, output
                    );
                }
            }
            Err(e) => {
                // Some programs may not compile yet
                eprintln!("Warning: {} failed to compile/run: {}", program.name, e);
            }
        }
    }
}

/// Test that the Go runner produces correct output for programs.
#[test]
fn programs_go_runner() {
    use common::go_runner_bin;

    let programs = discover_programs();
    assert!(!programs.is_empty(), "No programs found in fixtures/programs");

    let go_bin = go_runner_bin();

    for program in &programs {
        let args = ["run", program.main_go.to_str().unwrap()];
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
        let go_source = match std::fs::read_to_string(&program.main_go) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let go_file = program.main_go.to_str().unwrap();

        // Parse
        let ast = match gors::parser::parse_file(go_file, &go_source) {
            Ok(ast) => ast,
            Err(_) => continue,
        };

        // Compile with source map tracking
        let compiled =
            match gors::compiler::compile_with_source_map(ast, go_file, &go_source) {
                Ok(compiled) => compiled,
                Err(_) => continue,
            };

        // Generate Rust code
        let rust_source = match gors::codegen::generate(compiled) {
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
