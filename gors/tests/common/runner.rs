#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::common::{TestConfig, fixtures_dir, go_command};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const PROGRAM_TEST_STACK_SIZE: usize = 16 * 1024 * 1024;
const DEFAULT_GO_RUN_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_GENERATED_RUN_TIMEOUT: Duration = Duration::from_secs(10);
const RUST_TOOLCHAIN: &str = "1.96.0";
const RUST_EDITION: &str = "2024";

fn program_name(fixture_root: &Path, dir: &Path) -> String {
    dir.strip_prefix(fixture_root)
        .ok()
        .and_then(|relative| relative.to_str())
        .or_else(|| dir.file_name().and_then(|name| name.to_str()))
        .unwrap_or("<unknown>")
        .to_string()
}

fn run_test_thread_count() -> usize {
    configured_thread_count("GORS_TEST_RUN_THREADS")
        .or_else(|| configured_thread_count("GORS_TEST_THREADS"))
        .unwrap_or_else(default_run_test_thread_count)
}

fn configured_thread_count(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|threads| *threads > 0)
}

fn default_run_test_thread_count() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    default_run_test_thread_count_for_cpus(cpus)
}

fn default_run_test_thread_count_for_cpus(cpus: usize) -> usize {
    cpus.max(1).saturating_mul(2)
}

struct ProgramRunResult {
    name: String,
    passed: bool,
    skipped: bool,
    error: Option<String>,
}

pub struct ProgramFixtureRun {
    pub attempted_fixture_names: Vec<String>,
    pub passed_fixture_names: Vec<String>,
    pub retain_unattempted_fixture_names: bool,
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

struct RunningCommand {
    child: Child,
    stdout_file: tempfile::NamedTempFile,
    stderr_file: tempfile::NamedTempFile,
    started: Instant,
}

fn spawn_command_abortable(
    mut command: Command,
    abort: &AtomicBool,
) -> Result<Option<RunningCommand>, String> {
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
    let child = command.spawn().map_err(|e| e.to_string())?;
    Ok(Some(RunningCommand {
        child,
        stdout_file,
        stderr_file,
        started: Instant::now(),
    }))
}

fn wait_command_output_abortable(
    mut running: RunningCommand,
    abort: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<Option<Output>, String> {
    loop {
        if abort.load(Ordering::SeqCst) {
            let _ = running.child.kill();
            let _ = running.child.wait();
            return Ok(None);
        }
        if let Some(status) = running.child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Some(Output {
                status,
                stdout: fs::read(running.stdout_file.path()).map_err(|e| e.to_string())?,
                stderr: fs::read(running.stderr_file.path()).map_err(|e| e.to_string())?,
            }));
        }
        if let Some(timeout) = timeout
            && running.started.elapsed() >= timeout
        {
            let _ = running.child.kill();
            let _ = running.child.wait();
            return Err(format!("command timed out after {timeout:?}"));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn command_output_abortable(
    command: Command,
    abort: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<Option<Output>, String> {
    let Some(running) = spawn_command_abortable(command, abort)? else {
        return Ok(None);
    };
    wait_command_output_abortable(running, abort, timeout)
}

fn run_generated_rust_program(
    fixture_root: &Path,
    dir: &Path,
    config: &TestConfig,
    abort: &AtomicBool,
    metrics: &RunMetrics,
) -> ProgramRunResult {
    let name = program_name(fixture_root, dir);
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

    let go_run = match spawn_go_program(dir, abort) {
        Ok(Some(go_run)) => Some(go_run),
        Ok(None) => {
            return ProgramRunResult {
                name,
                passed: false,
                skipped: true,
                error: None,
            };
        }
        Err(_) => None,
    };

    let rust_out = match compile_and_run_generated_rust(fixture_root, dir, abort, metrics) {
        Ok(Some(output)) => output,
        Ok(None) => {
            let _ = finish_go_reference_stdout(go_run, abort, metrics, &name);
            return ProgramRunResult {
                name,
                passed: false,
                skipped: true,
                error: None,
            };
        }
        Err(error) => {
            if finish_go_reference_stdout(go_run, abort, metrics, &name).is_none() {
                return ProgramRunResult {
                    name,
                    passed: false,
                    skipped: true,
                    error: None,
                };
            }
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

    let Some(go_stdout) = finish_go_reference_stdout(go_run, abort, metrics, &name) else {
        return ProgramRunResult {
            name,
            passed: false,
            skipped: true,
            error: None,
        };
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

fn spawn_go_program(dir: &Path, abort: &AtomicBool) -> Result<Option<RunningCommand>, String> {
    let mut go_cmd = go_command();
    go_cmd.args(["run", "."]).current_dir(dir);
    spawn_command_abortable(go_cmd, abort)
}

fn finish_go_reference_stdout(
    go_run: Option<RunningCommand>,
    abort: &AtomicBool,
    metrics: &RunMetrics,
    name: &str,
) -> Option<String> {
    let Some(go_run) = go_run else {
        eprintln!("Skipping {name} - go run failed");
        return None;
    };
    let before = go_run.started;
    let output = wait_command_output_abortable(go_run, abort, Some(go_run_timeout()));
    RunMetrics::add_duration(&metrics.go, before.elapsed());
    match output {
        Ok(Some(o)) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(None) => None,
        _ => {
            eprintln!("Skipping {name} - go run failed");
            None
        }
    }
}

fn compile_and_run_generated_rust(
    fixture_root: &Path,
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

    let build_dir = cached_generated_output_dir(&program_name(fixture_root, dir), &output)?;
    let bin_path = build_dir.join("main");
    let cache_ok_path = build_dir.join(".rustc-ok");
    if bin_path.exists() && cache_ok_path.exists() {
        metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
        let before = Instant::now();
        let bin = Command::new(&bin_path);
        let output = command_output_abortable(bin, abort, Some(generated_run_timeout()));
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
    let edition_arg = format!("--edition={RUST_EDITION}");

    let mut rustc = Command::new("rustup");
    rustc
        .args(["run", RUST_TOOLCHAIN, "rustc"])
        .arg(&src_path)
        .args([
            edition_arg.as_str(),
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
    let Some(rustc_out) = command_output_abortable(rustc, abort, None)? else {
        return Ok(None);
    };
    RunMetrics::add_duration(&metrics.rustc, before.elapsed());
    if !rustc_out.status.success() {
        return Err(format!(
            "rustc failed for {} with {}:\n{}",
            src_path.display(),
            rustc_out.status,
            String::from_utf8_lossy(&rustc_out.stderr)
        ));
    }
    fs::write(&cache_ok_path, b"ok").map_err(|e| e.to_string())?;

    let before = Instant::now();
    let bin = Command::new(&bin_path);
    let output = command_output_abortable(bin, abort, Some(generated_run_timeout()));
    RunMetrics::add_duration(&metrics.rust_run, before.elapsed());
    output
}

fn go_run_timeout() -> Duration {
    duration_from_env("GORS_TEST_GO_RUN_TIMEOUT_SECS", DEFAULT_GO_RUN_TIMEOUT)
}

fn generated_run_timeout() -> Duration {
    duration_from_env(
        "GORS_TEST_GENERATED_RUN_TIMEOUT_SECS",
        DEFAULT_GENERATED_RUN_TIMEOUT,
    )
}

fn duration_from_env(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(default)
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
    hasher.update(
        format!(
            "\0rustc-toolchain:{RUST_TOOLCHAIN},rustc-flags:edition{RUST_EDITION},deny-unused,overflow-checks-off"
        )
        .as_bytes(),
    );
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
        Command::new("rustup")
            .args(["run", RUST_TOOLCHAIN, "rustc", "-vV"])
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

pub fn default_run_workers_for_cpus(cpus: usize) -> usize {
    default_run_test_thread_count_for_cpus(cpus)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gors crate should live under workspace root")
        .to_path_buf()
}

pub fn run_generated_program_fixture_set(fixture_set: &str) -> ProgramFixtureRun {
    let config = TestConfig::from_env();
    let fixture_root = fixtures_dir().join(fixture_set);
    let dirs = discover_program_dirs(&fixture_root, &config);
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/{fixture_set}"
    );

    let abort = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(RunMetrics::default());
    let worker_count = run_test_thread_count();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(worker_count)
        .stack_size(PROGRAM_TEST_STACK_SIZE)
        .build()
        .expect("failed to build program test thread pool");
    let results: Vec<_> = pool.install(|| {
        if config.verbose {
            eprintln!(
                "Testing {} {} programs on {} workers...",
                dirs.len(),
                fixture_set,
                rayon::current_num_threads()
            );
        }
        dirs.par_iter()
            .map(|dir| run_generated_rust_program(&fixture_root, dir, &config, &abort, &metrics))
            .collect()
    });

    let attempted_fixture_names = results
        .iter()
        .map(|result| result.name.clone())
        .collect::<Vec<_>>();
    let passed_fixture_names = results
        .iter()
        .filter(|result| result.passed)
        .map(|result| result.name.clone())
        .collect::<Vec<_>>();
    let passed = passed_fixture_names.len();
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
    ProgramFixtureRun {
        attempted_fixture_names,
        passed_fixture_names,
        retain_unattempted_fixture_names: config.filter.is_some() || config.limit.is_some(),
    }
}

fn discover_program_dirs(fixture_root: &Path, config: &TestConfig) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_program_dirs_recursive(fixture_root, &mut dirs);
    dirs.retain(|path| program_matches_filter(fixture_root, path, config.filter.as_deref()));
    dirs.sort();
    if let Some(limit) = config.limit {
        dirs.truncate(limit);
    }
    dirs
}

fn collect_program_dirs_recursive(dir: &Path, dirs: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('_'))
        {
            continue;
        }
        if path.join("main.go").exists() {
            dirs.push(path.clone());
        }
        collect_program_dirs_recursive(&path, dirs);
    }
}

fn program_matches_filter(fixture_root: &Path, path: &Path, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| {
        path.strip_prefix(fixture_root)
            .ok()
            .and_then(|relative| relative.to_str())
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .is_some_and(|name| name.contains(filter))
    })
}
