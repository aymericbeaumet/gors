#![cfg(feature = "test_integration_run")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestConfig, discover_program_dirs, fixtures_dir, go_command};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

fn program_name(dir: &Path) -> String {
    let programs_dir = fixtures_dir().join("go_programs");
    dir.strip_prefix(&programs_dir)
        .ok()
        .and_then(|relative| relative.to_str())
        .or_else(|| dir.file_name().and_then(|name| name.to_str()))
        .unwrap_or("<unknown>")
        .to_string()
}

struct ProgramRunResult {
    name: String,
    passed: bool,
    skipped: bool,
    error: Option<String>,
}

fn command_output_abortable(
    mut command: Command,
    abort: &AtomicBool,
) -> Result<Option<Output>, String> {
    if abort.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let stdout_file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let stderr_file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    command
        .stdout(Stdio::from(
            stdout_file.reopen().map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(
            stderr_file.reopen().map_err(|e| e.to_string())?,
        ));
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    loop {
        if abort.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Some(Output {
                status,
                stdout: fs::read(stdout_file.path()).map_err(|e| e.to_string())?,
                stderr: fs::read(stderr_file.path()).map_err(|e| e.to_string())?,
            }));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn run_generated_rust_program(
    dir: &Path,
    config: &TestConfig,
    abort: &AtomicBool,
) -> ProgramRunResult {
    let name = program_name(dir);
    if config.fail_fast && abort.load(Ordering::SeqCst) {
        return ProgramRunResult {
            name,
            passed: false,
            skipped: true,
            error: None,
        };
    }
    if config.verbose {
        eprintln!("RUN  {name}");
    }

    let mut go_cmd = go_command();
    go_cmd.args(["run", "."]).current_dir(dir);
    let go_out = command_output_abortable(go_cmd, abort);
    let go_stdout = match go_out {
        Ok(Some(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(None) => {
            return ProgramRunResult {
                name,
                passed: false,
                skipped: true,
                error: None,
            };
        }
        _ => {
            eprintln!("Skipping {name} - go run failed");
            return ProgramRunResult {
                name,
                passed: false,
                skipped: true,
                error: None,
            };
        }
    };

    let rust_out = match compile_and_run_generated_rust(dir, abort) {
        Ok(Some(output)) => output,
        Ok(None) => {
            return ProgramRunResult {
                name,
                passed: false,
                skipped: true,
                error: None,
            };
        }
        Err(error) => {
            let result = ProgramRunResult {
                name,
                passed: false,
                skipped: false,
                error: Some(error),
            };
            if config.fail_fast {
                abort.store(true, Ordering::SeqCst);
            }
            return result;
        }
    };

    let rust_stdout = String::from_utf8_lossy(&rust_out.stdout);
    let result = if rust_out.status.success() && rust_stdout == go_stdout.as_str() {
        ProgramRunResult {
            name,
            passed: true,
            skipped: false,
            error: None,
        }
    } else if rust_out.status.success() {
        ProgramRunResult {
            name,
            passed: false,
            skipped: false,
            error: Some(format!(
                "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                go_stdout, rust_stdout
            )),
        }
    } else {
        ProgramRunResult {
            name,
            passed: false,
            skipped: false,
            error: Some(format!(
                "generated Rust program failed:\n{}",
                String::from_utf8_lossy(&rust_out.stderr)
            )),
        }
    };

    if config.verbose {
        if result.passed {
            eprintln!("PASS {}", result.name);
        } else if let Some(error) = &result.error {
            eprintln!(
                "FAIL {}: {}",
                result.name,
                error.lines().next().unwrap_or("")
            );
        }
    }
    if config.fail_fast && result.error.is_some() {
        abort.store(true, Ordering::SeqCst);
    }

    result
}

fn compile_and_run_generated_rust(
    dir: &Path,
    abort: &AtomicBool,
) -> Result<Option<Output>, String> {
    let source_path = dir.to_string_lossy().into_owned();
    let program = gors::parser::parse_program_files(&[source_path])
        .map_err(|e| format!("parse failed: {e}"))?;
    let compiled = gors::compiler::compile_program_multi(program)
        .map_err(|e| format!("compile failed: {e}"))?;
    let output =
        gors::printer::generate_multi(compiled).map_err(|e| format!("print failed: {e}"))?;

    let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    write_generated_output(&output, temp_dir.path())?;

    let src_path = temp_dir.path().join("main.rs");
    if !src_path.exists() {
        return Err("generated output did not include main.rs".to_string());
    }

    let bin_path = temp_dir.path().join("main");
    let incremental_path = temp_dir.path().join("rustc-incremental");
    fs::create_dir_all(&incremental_path).map_err(|e| e.to_string())?;
    let incremental_arg = format!("incremental={}", incremental_path.display());

    let mut rustc = Command::new("rustc");
    rustc
        .arg(&src_path)
        .args([
            "--edition=2024",
            "-D",
            "unused_imports",
            "-D",
            "unused_macros",
            "-C",
            "overflow-checks=off",
            "-o",
        ])
        .arg(&bin_path)
        .args(["-C", &incremental_arg]);

    let Some(rustc_out) = command_output_abortable(rustc, abort)? else {
        return Ok(None);
    };
    if !rustc_out.status.success() {
        return Err(format!(
            "rustc failed:\n{}",
            String::from_utf8_lossy(&rustc_out.stderr)
        ));
    }

    let bin = Command::new(&bin_path);
    command_output_abortable(bin, abort)
}

fn write_generated_output(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    for (filename, source) in &output.files {
        let path = output_dir.join(filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[test]
fn run_programs_generated_rust() {
    let config = TestConfig::from_env();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let abort = Arc::new(AtomicBool::new(false));
    let results: Vec<_> = dirs
        .par_iter()
        .map(|dir| run_generated_rust_program(dir, &config, &abort))
        .collect();

    let passed = results.iter().filter(|result| result.passed).count();
    let skipped = results.iter().filter(|result| result.skipped).count();
    let failed: Vec<(String, String)> = results
        .into_iter()
        .filter_map(|result| result.error.map(|error| (result.name, error)))
        .collect();

    eprintln!(
        "\nResults: {passed}/{} passed, {skipped} skipped",
        passed + failed.len()
    );
    if !failed.is_empty() {
        for (name, err) in &failed {
            eprintln!("  FAIL {name}: {}", err.lines().next().unwrap_or(""));
        }
    }
    assert!(failed.is_empty(), "{} tests failed", failed.len());
}
