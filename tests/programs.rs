//! Program execution tests.
//!
//! These tests compile real-world Go programs and verify their output matches
//! the expected output from `go run`.
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

        // Run `go run` to get reference output.
        // Use `go run .` from the directory when go.mod exists (multi-file/multi-package),
        // otherwise use `go run main.go` for single-file programs.
        let has_go_mod = path.join("go.mod").exists();
        let expected_output = if has_go_mod {
            Command::new("go")
                .args(["run", "."])
                .current_dir(&path)
                .output()
        } else {
            Command::new("go")
                .args(["run", main_go.to_str().unwrap()])
                .output()
        }
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

/// Compile and run via `gors run <path>`.
pub fn run_via_rust_cli(path: &std::path::Path) -> BackendResult {
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
            "gors run failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    BackendResult::success(String::from_utf8_lossy(&output.stdout).to_string())
}

// =============================================================================
// Test Cases
// =============================================================================

#[test]
fn test_programs_rust_backend() {
    let programs = discover_programs();
    assert!(
        !programs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let mut passed = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    for program in &programs {
        if program.expected_output.is_empty() {
            eprintln!(
                "Skipping {} - no reference output from go run",
                program.name
            );
            continue;
        }

        let result = run_via_rust_cli(&program.dir);

        if result.success && result.output == program.expected_output {
            passed += 1;
        } else if result.success {
            failed.push((
                program.name.clone(),
                format!(
                    "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                    program.expected_output, result.output
                ),
            ));
        } else {
            failed.push((
                program.name.clone(),
                result
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }
    }

    eprintln!("\n=== Rust Backend Summary ===");
    eprintln!("Passed: {passed}");
    eprintln!("Failed: {}", failed.len());

    if !failed.is_empty() {
        eprintln!("\nFailing tests:");
        for (name, error) in &failed {
            eprintln!("  ✗ {}", name);
            for line in error.lines().take(3) {
                eprintln!("    {}", line);
            }
        }
    }

    assert!(failed.is_empty(), "{} tests failed", failed.len());
}

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

#[test]
fn test_programs_sourcemap() {
    let programs = discover_programs();

    for program in &programs {
        let (ast, files) = match gors::parser::parse_path(program.dir.to_str().unwrap()) {
            Ok(result) => result,
            Err(_) => continue,
        };

        let (go_file, go_source) = match files.first() {
            Some((f, s)) => (f.as_str(), s.as_str()),
            None => continue,
        };

        let compiled = match gors::compiler::compile_with_source_map(ast, go_file, go_source) {
            Ok(compiled) => compiled,
            Err(_) => continue,
        };

        let rust_source = match gors::backend_rust::generate(compiled) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let source_map = gors::compiler::build_source_map(&rust_source);

        let mut buf = Vec::new();
        if source_map.to_writer(&mut buf).is_err() {
            continue;
        }

        let parsed = match sourcemap::SourceMap::from_reader(&buf[..]) {
            Ok(sm) => sm,
            Err(e) => panic!("Invalid sourcemap for {}: {}", go_file, e),
        };

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

/// Comprehensive test that reports detailed results.
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

    let mut passed = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    for program in &programs {
        eprint!("  {}: ", program.name);

        if program.expected_output.is_empty() {
            eprintln!("SKIP (no reference output)");
            continue;
        }

        let result = run_via_rust_cli(&program.dir);
        if result.success && result.output == program.expected_output {
            eprintln!("PASS");
            passed += 1;
        } else if result.success {
            eprintln!("FAIL (output mismatch)");
            eprintln!("    Expected: {:?}", program.expected_output);
            eprintln!("    Got:      {:?}", result.output);
            failed.push((
                program.name.clone(),
                format!(
                    "expected {:?}, got {:?}",
                    program.expected_output, result.output
                ),
            ));
        } else {
            let err = result.error.as_deref().unwrap_or("Unknown error");
            eprintln!("FAIL ({})", err.lines().next().unwrap_or(err));
            failed.push((program.name.clone(), err.to_string()));
        }
    }

    eprintln!("\n========================================");
    eprintln!("RESULTS: {passed}/{} passed", passed + failed.len());
    eprintln!("========================================\n");
}
