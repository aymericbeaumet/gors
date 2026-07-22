#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cache;
mod catalog;
mod process;

use crate::common::{TestConfig, fixtures_dir, go_command};
use cache::{
    RUST_EDITION, RUST_TOOLCHAIN, cached_fixture_output_dir, prune_integration_cache,
    write_generated_output,
};
use catalog::discover_program_dirs;
use process::{
    RunningCommand, command_output_abortable, spawn_command_abortable,
    wait_command_output_abortable,
};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const PROGRAM_TEST_STACK_SIZE: usize = 16 * 1024 * 1024;
const DEFAULT_GO_RUN_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_GENERATED_RUN_TIMEOUT: Duration = Duration::from_secs(10);

pub fn command_output_with_timeout(command: Command, timeout: Duration) -> Result<Output, String> {
    process::command_output_with_timeout(command, timeout)
}

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
    let before = go_run.started();
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
    let compiled =
        gors::compiler::compile_program(program).map_err(|e| format!("compile failed: {e}"))?;
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

pub fn default_run_workers_for_cpus(cpus: usize) -> usize {
    default_run_test_thread_count_for_cpus(cpus)
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

#[cfg(test)]
mod tests;
