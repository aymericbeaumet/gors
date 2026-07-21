#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::common::{TestConfig, fixtures_dir, go_command};
use rayon::prelude::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime};

const PROGRAM_TEST_STACK_SIZE: usize = 16 * 1024 * 1024;
const DEFAULT_GO_RUN_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_GENERATED_RUN_TIMEOUT: Duration = Duration::from_secs(10);
const RUST_TOOLCHAIN: &str = "1.96.0";
const RUST_EDITION: &str = "2024";
const INTEGRATION_CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const INTEGRATION_CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const INCOMPLETE_CACHE_GRACE_PERIOD: Duration = Duration::from_secs(60 * 60);

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
    cpus.max(1)
}

struct ProgramRunResult {
    name: String,
    passed: bool,
    cancelled: bool,
    error: Option<String>,
}

pub struct ProgramFixtureRun {
    pub attempted_fixture_names: Vec<String>,
    pub passed_fixture_names: Vec<String>,
    pub complete: bool,
}

impl ProgramFixtureRun {
    pub fn add_passing_evidence<I>(&mut self, fixture_names: I)
    where
        I: IntoIterator<Item = String>,
    {
        for fixture in fixture_names {
            self.attempted_fixture_names.push(fixture.clone());
            self.passed_fixture_names.push(fixture);
        }
        self.attempted_fixture_names.sort();
        self.attempted_fixture_names.dedup();
        self.passed_fixture_names.sort();
        self.passed_fixture_names.dedup();
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureManifest {
    #[serde(default)]
    expected_program_count: Option<usize>,
    #[serde(default)]
    fixtures: BTreeMap<String, FixtureDirective>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureDirective {
    status: FixtureStatus,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    reason_file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureStatus {
    Run,
    Unsupported,
    CompileError,
}

#[derive(Debug)]
struct FixtureCatalog {
    runnable_dirs: Vec<PathBuf>,
    all_program_names: BTreeSet<String>,
    excluded_names: BTreeSet<String>,
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
            "Timings: go={:?}, parse={:?}, compile={:?}, print={:?}, write={:?}, rustc={:?}, run={:?}, fixture-cache={} hits/{} misses",
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

pub fn command_output_with_timeout(command: Command, timeout: Duration) -> Result<Output, String> {
    let abort = AtomicBool::new(false);
    command_output_abortable(command, &abort, Some(timeout))?
        .ok_or_else(|| "command was cancelled unexpectedly".to_string())
}

pub fn configured_go_run_timeout() -> Duration {
    go_run_timeout()
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
            cancelled: true,
            error: None,
        };
    }
    if config.verbose {
        eprintln!("RUN  {name}");
    }

    let go_run = match spawn_go_program(dir, abort) {
        Ok(Some(go_run)) => go_run,
        Ok(None) => {
            return ProgramRunResult {
                name,
                passed: false,
                cancelled: true,
                error: None,
            };
        }
        Err(error) => {
            return failed_program_result(
                name,
                format!("Go oracle could not be started: {error}"),
                config,
                abort,
            );
        }
    };

    let rust_result = compile_and_run_generated_rust(fixture_root, dir, abort, metrics);
    let go_out = match finish_go_reference(go_run, abort, metrics, &name) {
        Ok(Some(output)) => output,
        Ok(None) => {
            return ProgramRunResult {
                name,
                passed: false,
                cancelled: true,
                error: None,
            };
        }
        Err(error) => return failed_program_result(name, error, config, abort),
    };

    let rust_out = match rust_result {
        Ok(Some(output)) => output,
        Ok(None) => {
            return ProgramRunResult {
                name,
                passed: false,
                cancelled: true,
                error: None,
            };
        }
        Err(error) => return failed_program_result(name, error, config, abort),
    };

    let result = if rust_out.status.success()
        && rust_out.stdout == go_out.stdout
        && rust_out.stderr == go_out.stderr
    {
        ProgramRunResult {
            name,
            passed: true,
            cancelled: false,
            error: None,
        }
    } else if !rust_out.status.success() {
        ProgramRunResult {
            name,
            passed: false,
            cancelled: false,
            error: Some(format!(
                "generated Rust program exited with {}:\nstdout: {}\nstderr: {}",
                rust_out.status,
                String::from_utf8_lossy(&rust_out.stdout),
                String::from_utf8_lossy(&rust_out.stderr),
            )),
        }
    } else {
        ProgramRunResult {
            name,
            passed: false,
            cancelled: false,
            error: Some(format!(
                "observable behavior mismatch:\nGo exit: {}\nRust exit: {}\nGo stdout: {:?}\nRust stdout: {:?}\nGo stderr: {:?}\nRust stderr: {:?}",
                go_out.status,
                rust_out.status,
                String::from_utf8_lossy(&go_out.stdout),
                String::from_utf8_lossy(&rust_out.stdout),
                String::from_utf8_lossy(&go_out.stderr),
                String::from_utf8_lossy(&rust_out.stderr),
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

fn failed_program_result(
    name: String,
    error: String,
    config: &TestConfig,
    abort: &AtomicBool,
) -> ProgramRunResult {
    if config.fail_fast {
        abort.store(true, Ordering::SeqCst);
    }
    ProgramRunResult {
        name,
        passed: false,
        cancelled: false,
        error: Some(error),
    }
}

fn spawn_go_program(dir: &Path, abort: &AtomicBool) -> Result<Option<RunningCommand>, String> {
    let mut go_cmd = go_command();
    go_cmd
        .args(["run", "."])
        .current_dir(dir)
        .stdin(Stdio::null());
    spawn_command_abortable(go_cmd, abort)
}

fn finish_go_reference(
    go_run: RunningCommand,
    abort: &AtomicBool,
    metrics: &RunMetrics,
    name: &str,
) -> Result<Option<Output>, String> {
    let before = go_run.started;
    let output = wait_command_output_abortable(go_run, abort, Some(go_run_timeout()));
    RunMetrics::add_duration(&metrics.go, before.elapsed());
    match output {
        Ok(Some(output)) if output.status.success() => Ok(Some(output)),
        Ok(Some(output)) => Err(format!(
            "Go oracle for {name} exited with {}:\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )),
        Ok(None) => Ok(None),
        Err(error) => Err(format!("Go oracle for {name} failed: {error}")),
    }
}

fn compile_and_run_generated_rust(
    fixture_root: &Path,
    dir: &Path,
    abort: &AtomicBool,
    metrics: &RunMetrics,
) -> Result<Option<Output>, String> {
    let build_dir = cached_fixture_output_dir(fixture_root, dir)?;
    let bin_path = build_dir.join("main");
    let cache_ok_path = build_dir.join(".rustc-ok");
    if bin_path.exists() && cache_ok_path.exists() {
        metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
        // Refresh the marker so cache pruning can retain fixtures that remain in use.
        fs::write(&cache_ok_path, b"ok").map_err(|e| e.to_string())?;
        let before = Instant::now();
        let mut bin = Command::new(&bin_path);
        bin.current_dir(dir).stdin(Stdio::null());
        let output = command_output_abortable(bin, abort, Some(generated_run_timeout()));
        RunMetrics::add_duration(&metrics.rust_run, before.elapsed());
        return output;
    }
    metrics.cache_misses.fetch_add(1, Ordering::Relaxed);

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

    let before = Instant::now();
    write_generated_output(&output, &build_dir)?;
    RunMetrics::add_duration(&metrics.write, before.elapsed());

    let src_path = build_dir.join("main.rs");
    if !src_path.exists() {
        return Err("generated output did not include main.rs".to_string());
    }

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
        .arg(&bin_path);

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
    let mut bin = Command::new(&bin_path);
    bin.current_dir(dir).stdin(Stdio::null());
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

fn cached_fixture_output_dir(fixture_root: &Path, fixture_dir: &Path) -> Result<PathBuf, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"gors-integration-fixture-v2");
    hasher.update(b"\0");
    hasher.update(program_name(fixture_root, fixture_dir).as_bytes());
    hasher.update(b"\0");
    hasher.update(test_binary_fingerprint().as_bytes());
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
    let mut input_files = Vec::new();
    collect_regular_files_recursive(fixture_dir, &mut input_files)?;
    input_files.sort();
    for path in input_files {
        let relative = path
            .strip_prefix(fixture_dir)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?);
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    let hash = hex_hash(&digest);
    let dir = integration_cache_root().join(hash);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn integration_cache_root() -> PathBuf {
    workspace_root()
        .join("target")
        .join("gors-integration-run")
        .join("v2")
}

fn prune_integration_cache() -> Result<(), String> {
    let root = integration_cache_root();
    if !root.exists() {
        return Ok(());
    }
    let now = SystemTime::now();
    let mut successful = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| format!("{}: {error}", root.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let marker = path.join(".rustc-ok");
        if !marker.exists() {
            let age = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or(Duration::ZERO);
            if age >= INCOMPLETE_CACHE_GRACE_PERIOD {
                let _ = fs::remove_dir_all(path);
            }
            continue;
        }
        let modified = marker
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age >= INTEGRATION_CACHE_MAX_AGE {
            let _ = fs::remove_dir_all(path);
            continue;
        }
        successful.push((modified, directory_size(&path)?, path));
    }

    successful.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let mut retained_bytes = 0_u64;
    for (_, size, path) in successful {
        if retained_bytes.saturating_add(size) <= INTEGRATION_CACHE_MAX_BYTES {
            retained_bytes = retained_bytes.saturating_add(size);
        } else {
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(())
}

fn directory_size(dir: &Path) -> Result<u64, String> {
    let mut size = 0_u64;
    for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            size = size.saturating_add(directory_size(&path)?);
        } else {
            size = size.saturating_add(
                entry
                    .metadata()
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .len(),
            );
        }
    }
    Ok(size)
}

fn collect_regular_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_regular_files_recursive(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn test_binary_fingerprint() -> &'static str {
    static TEST_BINARY_FINGERPRINT: OnceLock<String> = OnceLock::new();
    TEST_BINARY_FINGERPRINT.get_or_init(|| {
        std::env::current_exe()
            .and_then(fs::read)
            .map(|binary| {
                let mut hasher = Sha256::new();
                hasher.update(binary);
                hex_hash(&hasher.finalize())
            })
            .unwrap_or_else(|error| format!("test-binary-fingerprint-error:{error}"))
    })
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
    run_generated_program_fixture_set_impl(fixture_set, false)
}

pub fn run_generated_program_fixture_set_allow_empty(fixture_set: &str) -> ProgramFixtureRun {
    run_generated_program_fixture_set_impl(fixture_set, true)
}

fn run_generated_program_fixture_set_impl(
    fixture_set: &str,
    allow_empty_filtered_run: bool,
) -> ProgramFixtureRun {
    let config = TestConfig::from_env();
    let fixture_root = fixtures_dir().join(fixture_set);
    let catalog = discover_program_dirs(&fixture_root, &config)
        .unwrap_or_else(|error| panic!("failed to discover fixtures/{fixture_set}: {error}"));
    let dirs = catalog.runnable_dirs;
    if dirs.is_empty() && allow_empty_filtered_run && config.filter.is_some() {
        return ProgramFixtureRun {
            attempted_fixture_names: Vec::new(),
            passed_fixture_names: Vec::new(),
            complete: false,
        };
    }
    assert!(
        !dirs.is_empty(),
        "No runnable programs found in fixtures/{fixture_set}; filter={:?}",
        config.filter
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
        .filter(|result| !result.cancelled)
        .map(|result| result.name.clone())
        .collect::<Vec<_>>();
    let passed_fixture_names = results
        .iter()
        .filter(|result| result.passed)
        .map(|result| result.name.clone())
        .collect::<Vec<_>>();
    let passed = passed_fixture_names.len();
    let cancelled = results.iter().filter(|result| result.cancelled).count();
    let failed: Vec<(String, String)> = results
        .into_iter()
        .filter_map(|result| result.error.map(|error| (result.name, error)))
        .collect();

    eprintln!(
        "\nResults: {passed}/{} passed, {cancelled} cancelled",
        passed + failed.len()
    );
    metrics.print();
    if !failed.is_empty() {
        for (name, err) in &failed {
            eprintln!("  FAIL {name}:\n{err}");
        }
    }
    if let Err(error) = prune_integration_cache() {
        eprintln!("Warning: could not prune the generated-program cache: {error}");
    }
    assert!(failed.is_empty(), "{} tests failed", failed.len());
    let complete = config.filter.is_none()
        && config.limit.is_none()
        && !config.include_unsupported
        && cancelled == 0
        && attempted_fixture_names.len()
            == catalog
                .all_program_names
                .len()
                .saturating_sub(catalog.excluded_names.len());
    ProgramFixtureRun {
        attempted_fixture_names,
        passed_fixture_names,
        complete,
    }
}

fn discover_program_dirs(
    fixture_root: &Path,
    config: &TestConfig,
) -> Result<FixtureCatalog, String> {
    let mut dirs = Vec::new();
    collect_program_dirs_recursive(fixture_root, &mut dirs)?;
    let manifest = load_fixture_manifest(fixture_root)?;
    let all_program_names = dirs
        .iter()
        .map(|path| program_name(fixture_root, path))
        .collect::<BTreeSet<_>>();
    validate_fixture_manifest(fixture_root, &manifest, &all_program_names)?;
    let excluded_names = manifest
        .fixtures
        .iter()
        .filter(|(name, directive)| {
            directive.status != FixtureStatus::Run && all_program_names.contains(*name)
        })
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    dirs.retain(|path| {
        let name = program_name(fixture_root, path);
        match manifest
            .fixtures
            .get(&name)
            .map(|directive| directive.status)
        {
            None => true,
            Some(FixtureStatus::Run) => true,
            Some(FixtureStatus::Unsupported) => config.include_unsupported,
            Some(FixtureStatus::CompileError) => false,
        }
    });
    dirs.retain(|path| program_matches_filter(fixture_root, path, config.filter.as_deref()));
    dirs.sort();
    if let Some(limit) = config.limit {
        dirs.truncate(limit);
    }
    Ok(FixtureCatalog {
        runnable_dirs: dirs,
        all_program_names,
        excluded_names,
    })
}

fn collect_program_dirs_recursive(dir: &Path, dirs: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("main.go").exists() {
            dirs.push(path.clone());
        }
        collect_program_dirs_recursive(&path, dirs)?;
    }
    Ok(())
}

fn load_fixture_manifest(fixture_root: &Path) -> Result<FixtureManifest, String> {
    let path = fixture_root.join("fixtures.json");
    if !path.exists() {
        return Ok(FixtureManifest::default());
    }
    serde_json::from_str(
        &fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}

fn validate_fixture_manifest(
    fixture_root: &Path,
    manifest: &FixtureManifest,
    program_names: &BTreeSet<String>,
) -> Result<(), String> {
    if let Some(expected) = manifest.expected_program_count
        && program_names.len() != expected
    {
        return Err(format!(
            "fixtures.json expected {expected} program directories containing main.go, discovered {}; update the inventory only when adding or removing an intentional fixture",
            program_names.len()
        ));
    }
    for name in program_names {
        let has_private_component = Path::new(name).components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|part| part.starts_with('_'))
        });
        if has_private_component && !manifest.fixtures.contains_key(name) {
            return Err(format!(
                "{name}: underscore-prefixed fixtures require an explicit fixtures.json status"
            ));
        }
    }
    for (name, directive) in &manifest.fixtures {
        if !program_names.contains(name) {
            return Err(format!(
                "{name}: fixtures.json entry does not reference a directory containing main.go"
            ));
        }
        if directive.status != FixtureStatus::Run {
            let reason = if let Some(reason_file) = &directive.reason_file {
                let path = fixture_root.join(reason_file);
                fs::read_to_string(&path).map_err(|error| {
                    format!("cannot read reason file {}: {error}", path.display())
                })?
            } else {
                directive.reason.clone()
            };
            if reason.trim().is_empty() {
                return Err(format!(
                    "{name}: non-running fixture status {:?} requires a non-empty reason or reasonFile",
                    directive.status
                ));
            }
        }
    }
    Ok(())
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_config() -> TestConfig {
        TestConfig {
            limit: None,
            filter: None,
            verbose: false,
            fail_fast: false,
            include_unsupported: false,
        }
    }

    #[test]
    fn underscore_fixture_requires_explicit_status() {
        let fixture_root = tempfile::tempdir().unwrap();
        let fixture = fixture_root.path().join("_known_failure");
        fs::create_dir_all(&fixture).unwrap();
        fs::write(fixture.join("main.go"), "package main\nfunc main() {}\n").unwrap();

        let error = discover_program_dirs(fixture_root.path(), &test_config()).unwrap_err();

        assert!(error.contains("underscore-prefixed fixtures require an explicit"));
    }

    #[test]
    fn explicit_unsupported_fixture_is_discovered_but_not_run() {
        let fixture_root = tempfile::tempdir().unwrap();
        let fixture = fixture_root.path().join("_known_failure");
        fs::create_dir_all(&fixture).unwrap();
        fs::write(fixture.join("main.go"), "package main\nfunc main() {}\n").unwrap();
        fs::write(
            fixture_root.path().join("fixtures.json"),
            r#"{
  "fixtures": {
    "_known_failure": {
      "status": "unsupported",
      "reason": "compiler issue recorded by the fixture owner"
    }
  }
}"#,
        )
        .unwrap();

        let catalog = discover_program_dirs(fixture_root.path(), &test_config()).unwrap();

        assert!(catalog.runnable_dirs.is_empty());
        assert_eq!(
            catalog.all_program_names,
            BTreeSet::from(["_known_failure".to_string()])
        );
        assert_eq!(
            catalog.excluded_names,
            BTreeSet::from(["_known_failure".to_string()])
        );
    }

    #[test]
    fn manifest_rejects_missing_fixture_and_empty_reason() {
        let fixture_root = tempfile::tempdir().unwrap();
        fs::write(
            fixture_root.path().join("fixtures.json"),
            r#"{
  "fixtures": {
    "_missing": {
      "status": "unsupported",
      "reason": ""
    }
  }
}"#,
        )
        .unwrap();

        let error = discover_program_dirs(fixture_root.path(), &test_config()).unwrap_err();

        assert!(error.contains("does not reference a directory containing main.go"));
    }

    #[test]
    fn manifest_program_count_makes_missing_fixtures_a_hard_error() {
        let fixture_root = tempfile::tempdir().unwrap();
        fs::write(
            fixture_root.path().join("fixtures.json"),
            r#"{
  "expectedProgramCount": 1,
  "fixtures": {}
}"#,
        )
        .unwrap();

        let error = discover_program_dirs(fixture_root.path(), &test_config()).unwrap_err();

        assert!(error.contains("expected 1 program directories"));
        assert!(error.contains("discovered 0"));
    }

    #[test]
    fn repository_fixture_manifests_classify_every_private_program() {
        for fixture_set in ["go_spec", "go_stdlib", "go_programs"] {
            discover_program_dirs(&fixtures_dir().join(fixture_set), &test_config())
                .unwrap_or_else(|error| panic!("fixtures/{fixture_set}: {error}"));
        }
    }
}
