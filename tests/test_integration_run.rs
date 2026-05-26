#![cfg(feature = "test_integration_run")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestConfig, discover_program_dirs, fixtures_dir, go_command};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const PROGRAM_TEST_STACK_SIZE: usize = 16 * 1024 * 1024;

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

#[derive(Default)]
struct RunMetrics {
    go: AtomicU64,
    parse: AtomicU64,
    compile: AtomicU64,
    print: AtomicU64,
    write: AtomicU64,
    rustc: AtomicU64,
    rust_run: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
}

impl RunMetrics {
    fn add_duration(cell: &AtomicU64, duration: Duration) {
        let nanos = duration.as_nanos().min(u128::from(u64::MAX)) as u64;
        cell.fetch_add(nanos, Ordering::Relaxed);
    }

    fn duration(cell: &AtomicU64) -> Duration {
        Duration::from_nanos(cell.load(Ordering::Relaxed))
    }

    fn print(&self) {
        eprintln!(
            "Timings: go={:?}, parse={:?}, compile={:?}, print={:?}, write={:?}, rustc={:?}, run={:?}, rustc-cache={} hits/{} misses",
            Self::duration(&self.go),
            Self::duration(&self.parse),
            Self::duration(&self.compile),
            Self::duration(&self.print),
            Self::duration(&self.write),
            Self::duration(&self.rustc),
            Self::duration(&self.rust_run),
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
        );
    }
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
    metrics: &RunMetrics,
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

    let go_out = run_go_program(dir, abort, metrics);
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

    let rust_out = match compile_and_run_generated_rust(dir, abort, metrics) {
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

fn run_go_program(
    dir: &Path,
    abort: &AtomicBool,
    metrics: &RunMetrics,
) -> Result<Option<Output>, String> {
    let mut go_cmd = go_command();
    go_cmd.args(["run", "."]).current_dir(dir);
    let before = Instant::now();
    let output = command_output_abortable(go_cmd, abort);
    RunMetrics::add_duration(&metrics.go, before.elapsed());
    output
}

fn compile_and_run_generated_rust(
    dir: &Path,
    abort: &AtomicBool,
    metrics: &RunMetrics,
) -> Result<Option<Output>, String> {
    let source_path = dir.to_string_lossy().into_owned();
    let before = Instant::now();
    let program = gors::parser::parse_program_files(&[source_path])
        .map_err(|e| format!("parse failed: {e}"))?;
    RunMetrics::add_duration(&metrics.parse, before.elapsed());

    let before = Instant::now();
    let compiled = gors::compiler::compile_program_multi(program)
        .map_err(|e| format!("compile failed: {e}"))?;
    RunMetrics::add_duration(&metrics.compile, before.elapsed());

    let before = Instant::now();
    let output =
        gors::printer::generate_multi(compiled).map_err(|e| format!("print failed: {e}"))?;
    RunMetrics::add_duration(&metrics.print, before.elapsed());

    let build_dir = cached_generated_output_dir(&program_name(dir), &output)?;
    let bin_path = build_dir.join("main");
    let cache_ok_path = build_dir.join(".rustc-ok");
    if bin_path.exists() && cache_ok_path.exists() {
        metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
        let before = Instant::now();
        let bin = Command::new(&bin_path);
        let output = command_output_abortable(bin, abort);
        RunMetrics::add_duration(&metrics.rust_run, before.elapsed());
        return output;
    }
    metrics.cache_misses.fetch_add(1, Ordering::Relaxed);

    let before = Instant::now();
    write_generated_output(&output, &build_dir)?;
    RunMetrics::add_duration(&metrics.write, before.elapsed());

    let src_path = build_dir.join("main.rs");
    if !src_path.exists() {
        return Err("generated output did not include main.rs".to_string());
    }

    let incremental_path = build_dir.join("rustc-incremental");
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

    let before = Instant::now();
    let Some(rustc_out) = command_output_abortable(rustc, abort)? else {
        return Ok(None);
    };
    RunMetrics::add_duration(&metrics.rustc, before.elapsed());
    if !rustc_out.status.success() {
        return Err(format!(
            "rustc failed:\n{}",
            String::from_utf8_lossy(&rustc_out.stderr)
        ));
    }
    fs::write(&cache_ok_path, b"ok").map_err(|e| e.to_string())?;

    let before = Instant::now();
    let bin = Command::new(&bin_path);
    let output = command_output_abortable(bin, abort);
    RunMetrics::add_duration(&metrics.rust_run, before.elapsed());
    output
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

fn cached_generated_output_dir(
    program_name: &str,
    output: &gors::printer::GeneratedOutput,
) -> Result<PathBuf, String> {
    let mut hasher = Sha256::new();
    hasher.update(program_name.as_bytes());
    hasher.update(b"\0");
    hasher.update(rustc_fingerprint().as_bytes());
    hasher.update(b"\0");
    hasher.update(gors::STDLIB_VERSION.as_bytes());
    hasher.update(b"\0rustc-flags:edition2024,deny-unused,overflow-checks-off");
    hasher.update(b"\0");
    for (filename, source) in &output.files {
        hasher.update(filename.as_bytes());
        hasher.update(b"\0");
        hasher.update(source.as_bytes());
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    let hash = hex_hash(&digest);
    let dir = workspace_root()
        .join("target")
        .join("gors-integration-run")
        .join(hash);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn rustc_fingerprint() -> &'static str {
    static RUSTC_FINGERPRINT: OnceLock<String> = OnceLock::new();
    RUSTC_FINGERPRINT.get_or_init(|| {
        Command::new("rustc")
            .arg("-vV")
            .output()
            .map(|output| {
                let mut hasher = Sha256::new();
                hasher.update(&output.stdout);
                hasher.update(&output.stderr);
                hasher.update([u8::from(output.status.success())]);
                let digest = hasher.finalize();
                hex_hash(&digest)
            })
            .unwrap_or_else(|error| format!("rustc-fingerprint-error:{error}"))
    })
}

fn hex_hash(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gors crate should live under workspace root")
        .to_path_buf()
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
    let metrics = Arc::new(RunMetrics::default());
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(PROGRAM_TEST_STACK_SIZE)
        .build()
        .expect("failed to build program test thread pool");
    let results: Vec<_> = pool.install(|| {
        dirs.par_iter()
            .map(|dir| run_generated_rust_program(dir, &config, &abort, &metrics))
            .collect()
    });

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
    metrics.print();
    if !failed.is_empty() {
        for (name, err) in &failed {
            eprintln!("  FAIL {name}: {}", err.lines().next().unwrap_or(""));
        }
    }
    assert!(failed.is_empty(), "{} tests failed", failed.len());
}
