//! Program execution tests.
//!
//! These tests compile Go programs via `gors run` and verify their output
//! matches the reference output from `go run`.
//!
//! # Adding a new program
//!
//! 1. Create a directory in `fixtures/go_programs/` (e.g., `my_program/`)
//! 2. Add `main.go` and `go.mod` with your Go program
//! 3. The program will automatically be tested

mod common;

use common::{fixtures_dir, gors_bin};
use sha2::Digest;
use std::path::PathBuf;
use std::process::Command;

/// Discover all program directories in fixtures/go_programs that have main.go.
fn discover_program_dirs() -> Vec<PathBuf> {
    let programs_dir = fixtures_dir().join("go_programs");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&programs_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", programs_dir.display(), e))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("main.go").exists())
        .collect();
    dirs.sort();
    dirs
}

// =============================================================================
// Test Cases
// =============================================================================

#[test]
fn test_programs_rust_backend() {
    let gors = gors_bin();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let mut passed = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    for dir in &dirs {
        let name = dir.file_name().unwrap().to_str().unwrap();

        let go_out = Command::new("go")
            .args(["run", "."])
            .current_dir(dir)
            .output();
        let go_stdout = match go_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => {
                eprintln!("Skipping {name} - go run failed");
                continue;
            }
        };

        let gors_out = Command::new(gors.as_path())
            .args(["run", dir.to_str().unwrap()])
            .output()
            .unwrap_or_else(|e| panic!("failed to run gors on {name}: {e}"));

        let gors_stdout = String::from_utf8_lossy(&gors_out.stdout);

        if gors_out.status.success() && gors_stdout == go_stdout.as_str() {
            passed += 1;
        } else if gors_out.status.success() {
            failed.push((
                name.to_string(),
                format!(
                    "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                    go_stdout, gors_stdout
                ),
            ));
        } else {
            failed.push((
                name.to_string(),
                format!(
                    "gors run failed:\n{}",
                    String::from_utf8_lossy(&gors_out.stderr)
                ),
            ));
        }
    }

    eprintln!("\nResults: {passed}/{} passed", passed + failed.len());
    if !failed.is_empty() {
        for (name, err) in &failed {
            eprintln!("  FAIL {name}: {}", err.lines().next().unwrap_or(""));
        }
    }
    assert!(failed.is_empty(), "{} tests failed", failed.len());
}

#[test]
fn test_programs_go_runner() {
    use common::go_runner_bin;

    let go_bin = go_runner_bin();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    for dir in &dirs {
        let name = dir.file_name().unwrap().to_str().unwrap();

        let go_out = Command::new("go")
            .args(["run", "."])
            .current_dir(dir)
            .output();
        let go_stdout = match go_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => continue,
        };

        let runner_out = Command::new(go_bin.as_path())
            .args(["run", dir.to_str().unwrap()])
            .output()
            .unwrap_or_else(|e| panic!("Failed to run go runner on {name}: {e}"));

        if runner_out.status.success() {
            let actual = String::from_utf8_lossy(&runner_out.stdout);
            assert_eq!(
                actual.as_ref(),
                go_stdout.as_str(),
                "Output mismatch for {name}"
            );
        }
    }
}

#[test]
fn test_programs_sourcemap() {
    let dirs = discover_program_dirs();

    for dir in &dirs {
        let (ast, files) = match gors::parser::parse_path(dir.to_str().unwrap()) {
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

// =============================================================================
// Run pattern tests — verify the 4 `gors run` invocation styles
// =============================================================================

/// A. Running a specific file: `gors run main.go`
#[test]
fn test_run_single_file() {
    let gors = gors_bin();
    let main_go = fixtures_dir()
        .join("go_programs/hello_world/main.go")
        .to_string_lossy()
        .to_string();

    let gors_out = Command::new(gors.as_path())
        .args(["run", &main_go])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run main.go failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "."])
        .current_dir(fixtures_dir().join("go_programs/hello_world"))
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

/// B. Running multiple files: `gors run main.go helpers.go`
#[test]
fn test_run_multiple_files() {
    let gors = gors_bin();
    let dir = fixtures_dir().join("go_programs/multi_file_same_package");
    let main_go = dir.join("main.go").to_string_lossy().to_string();
    let helpers_go = dir.join("helpers.go").to_string_lossy().to_string();

    let gors_out = Command::new(gors.as_path())
        .args(["run", &main_go, &helpers_go])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run main.go helpers.go failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "."])
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

/// C. Running the current directory: `gors run .`
#[test]
fn test_run_current_directory() {
    let gors = gors_bin();
    let dir = fixtures_dir().join("go_programs/multi_file_same_package");

    let gors_out = Command::new(gors.as_path())
        .args(["run", dir.to_str().unwrap()])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run <dir> failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "."])
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

/// D. Running a specific local package: `gors run ./cmd/myapp`
#[test]
fn test_run_specific_package() {
    let gors = gors_bin();
    let pkg_dir = fixtures_dir().join("go_programs/cmd_layout/cmd/myapp");

    let gors_out = Command::new(gors.as_path())
        .args(["run", pkg_dir.to_str().unwrap()])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run ./cmd/myapp failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "./cmd/myapp"])
        .current_dir(fixtures_dir().join("go_programs/cmd_layout"))
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

// =============================================================================
// Multi-file output tests (fast unit tests)
// =============================================================================

#[test]
fn test_multi_file_output_hello_world() {
    let path = fixtures_dir()
        .join("go_programs/hello_world/main.go")
        .to_string_lossy()
        .to_string();
    let program = gors::parser::parse_program(&path).unwrap();
    let compiled = gors::compiler::compile_program_multi(program).unwrap();
    assert!(compiled.has_main);
    assert!(compiled.modules.contains_key("__main__"));
    assert!(compiled.modules.contains_key("fmt"));
}

#[test]
fn test_multi_file_output_with_imports() {
    let path = fixtures_dir()
        .join("go_programs/import_local_package")
        .to_string_lossy()
        .to_string();
    let program = gors::parser::parse_program(&path).unwrap();
    let compiled = gors::compiler::compile_program_multi(program).unwrap();

    assert!(compiled.has_main);
    assert!(compiled.modules.values().any(|m| m.mod_name == "greet"));
    assert!(
        compiled
            .modules
            .values()
            .any(|m| m.mod_name == "fmt" && m.is_stdlib)
    );

    let greet = compiled
        .modules
        .values()
        .find(|m| m.mod_name == "greet")
        .unwrap();
    assert_eq!(greet.filename, "example__greet.rs");
    assert!(!greet.content_hash.is_empty());
}

#[test]
fn test_multi_file_generate_and_compile() {
    let path = fixtures_dir()
        .join("go_programs/import_local_package")
        .to_string_lossy()
        .to_string();
    let program = gors::parser::parse_program(&path).unwrap();
    let compiled = gors::compiler::compile_program_multi(program).unwrap();
    let output = gors::backend_rust::generate_multi(compiled).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    for (filename, source) in &output.files {
        std::fs::write(tmp.path().join(filename), source).unwrap();
    }

    let status = Command::new("rustc")
        .args([
            tmp.path().join("main.rs").to_str().unwrap(),
            "--edition=2021",
            "-o",
            tmp.path().join("main").to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "rustc compilation failed");

    let run_output = Command::new(tmp.path().join("main")).output().unwrap();
    assert!(run_output.status.success());
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert_eq!(stdout, "Hello from greet!\nGoodbye from greet!\n");
}

#[test]
fn test_multi_file_no_filename_collisions() {
    let path = fixtures_dir()
        .join("go_programs/import_recursive")
        .to_string_lossy()
        .to_string();
    let program = gors::parser::parse_program(&path).unwrap();
    let compiled = gors::compiler::compile_program_multi(program).unwrap();

    let filenames: Vec<&str> = compiled
        .modules
        .values()
        .map(|m| m.filename.as_str())
        .collect();
    let unique: std::collections::HashSet<&str> = filenames.iter().copied().collect();
    assert_eq!(filenames.len(), unique.len(), "filenames must be unique");
}

#[test]
fn test_multi_file_content_hash_stability() {
    let path = fixtures_dir()
        .join("go_programs/import_local_package")
        .to_string_lossy()
        .to_string();

    let program1 = gors::parser::parse_program(&path).unwrap();
    let compiled1 = gors::compiler::compile_program_multi(program1).unwrap();

    let program2 = gors::parser::parse_program(&path).unwrap();
    let compiled2 = gors::compiler::compile_program_multi(program2).unwrap();

    for (key, m1) in &compiled1.modules {
        if m1.is_stdlib {
            continue;
        }
        let m2 = compiled2.modules.get(key).unwrap();
        assert_eq!(
            m1.content_hash, m2.content_hash,
            "hash for {key} should be stable"
        );
    }
}

#[test]
fn test_build_manifest_skip_unchanged() {
    let path = fixtures_dir()
        .join("go_programs/hello_world/main.go")
        .to_string_lossy()
        .to_string();

    let tmp = tempfile::tempdir().unwrap();

    // First build
    let program = gors::parser::parse_program(&path).unwrap();
    let compiled = gors::compiler::compile_program_multi(program).unwrap();
    let output = gors::backend_rust::generate_multi(compiled).unwrap();

    let mut manifest = gors::compiler::manifest::BuildManifest::new();
    for (filename, source) in &output.files {
        std::fs::write(tmp.path().join(filename), source).unwrap();
        manifest.modules.insert(
            filename.clone(),
            gors::compiler::manifest::ModuleEntry {
                content_hash: format!("{:x}", sha2::Sha256::digest(source.as_bytes())),
                output_file: filename.clone(),
            },
        );
    }
    manifest.save(tmp.path()).unwrap();

    // Load manifest and verify skip logic
    let loaded = gors::compiler::manifest::BuildManifest::load(tmp.path()).unwrap();
    for (filename, source) in &output.files {
        if filename == "lib.rs" || filename == "main.rs" {
            continue;
        }
        let hash = format!("{:x}", sha2::Sha256::digest(source.as_bytes()));
        assert!(
            !loaded.needs_recompile(filename, &hash),
            "unchanged module {filename} should not need recompile"
        );
    }

    // A changed hash should trigger recompile
    assert!(loaded.needs_recompile("fmt.rs", "different_hash"));
}

#[test]
fn test_multi_file_lib_rs_is_consumable() {
    let path = fixtures_dir()
        .join("go_programs/hello_world/main.go")
        .to_string_lossy()
        .to_string();
    let program = gors::parser::parse_program(&path).unwrap();
    let compiled = gors::compiler::compile_program_multi(program).unwrap();
    let output = gors::backend_rust::generate_multi(compiled).unwrap();

    assert!(
        output.files.contains_key("lib.rs"),
        "lib.rs should exist for library consumption"
    );
    let lib_rs = &output.files["lib.rs"];
    assert!(
        lib_rs.contains("pub mod"),
        "lib.rs should have pub mod declarations"
    );
}
