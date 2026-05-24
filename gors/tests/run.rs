#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestConfig, discover_program_dirs, fixtures_dir, go_runner_bin, gors_bin};
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

#[test]
fn run_single_file() {
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

#[test]
fn run_multiple_files() {
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

#[test]
fn run_current_directory() {
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

#[test]
fn run_specific_package() {
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

#[test]
fn build_removes_manifest_stale_output_files() {
    let gors = gors_bin();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("go.mod"), "module example\n").unwrap();
    std::fs::write(
        src.join("main.go"),
        r#"
package main

func main() {}
"#,
    )
    .unwrap();

    let first = Command::new(gors.as_path())
        .args([
            "build",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors build: {e}"));
    assert!(
        first.status.success(),
        "initial gors build failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );

    let stale_file = out.join("stale.rs");
    std::fs::write(&stale_file, "stale").unwrap();
    let mut manifest = gors::compiler::manifest::BuildManifest::load(&out).unwrap();
    manifest.modules.insert(
        "stale.rs".to_string(),
        gors::compiler::manifest::ModuleEntry {
            content_hash: "stale".to_string(),
            output_file: "stale.rs".to_string(),
        },
    );
    manifest.save(&out).unwrap();

    let second = Command::new(gors.as_path())
        .args([
            "build",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to rerun gors build: {e}"));
    assert!(
        second.status.success(),
        "second gors build failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        !stale_file.exists(),
        "stale manifest output should be removed"
    );
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
    gors: &Path,
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

    let mut go_cmd = Command::new("go");
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

    let mut gors_cmd = Command::new(gors);
    gors_cmd.args(["run", dir.to_str().unwrap()]);
    let gors_out = command_output_abortable(gors_cmd, abort)
        .unwrap_or_else(|e| panic!("failed to run gors on {name}: {e}"));
    let Some(gors_out) = gors_out else {
        return ProgramRunResult {
            name,
            passed: false,
            skipped: true,
            error: None,
        };
    };

    let gors_stdout = String::from_utf8_lossy(&gors_out.stdout);
    let result = if gors_out.status.success() && gors_stdout == go_stdout.as_str() {
        ProgramRunResult {
            name,
            passed: true,
            skipped: false,
            error: None,
        }
    } else if gors_out.status.success() {
        ProgramRunResult {
            name,
            passed: false,
            skipped: false,
            error: Some(format!(
                "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                go_stdout, gors_stdout
            )),
        }
    } else {
        ProgramRunResult {
            name,
            passed: false,
            skipped: false,
            error: Some(format!(
                "gors run failed:\n{}",
                String::from_utf8_lossy(&gors_out.stderr)
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

#[test]
fn run_programs_generated_rust() {
    let gors = gors_bin();
    let config = TestConfig::from_env();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let abort = Arc::new(AtomicBool::new(false));
    let results: Vec<_> = dirs
        .par_iter()
        .map(|dir| run_generated_rust_program(gors.as_path(), dir, &config, &abort))
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

#[test]
fn run_programs_go_runner() {
    let go_bin = go_runner_bin();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    dirs.par_iter().for_each(|dir| {
        let name = program_name(dir);

        let go_out = Command::new("go")
            .args(["run", "."])
            .current_dir(dir)
            .output();
        let go_stdout = match go_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => return,
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
    });
}
