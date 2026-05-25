//! Test runner infrastructure.
//!
//! This module provides utilities for running tests against Go files,
//! comparing outputs with the Go oracle implementation.
//!
//! ## Environment Variables
//!
//! - `GORS_TEST_LIMIT`: Maximum number of files to test (default: unlimited)
//! - `GORS_TEST_FILTER`: Only test files matching this substring
//! - `GORS_TEST_VERBOSE`: Show progress during testing (set to "1" to enable)
//! - `GORS_TEST_FAIL_FAST`: Cancel queued/running tests after the first failure where supported

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// Test configuration from environment variables.
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Maximum number of files to test (None = unlimited)
    pub limit: Option<usize>,
    /// Only test files containing this substring
    pub filter: Option<String>,
    /// Show verbose progress output
    pub verbose: bool,
    /// Stop after the first failure
    pub fail_fast: bool,
}

impl TestConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            limit: std::env::var("GORS_TEST_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok()),
            filter: std::env::var("GORS_TEST_FILTER").ok(),
            verbose: std::env::var("GORS_TEST_VERBOSE")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
            fail_fast: std::env::var("GORS_TEST_FAIL_FAST")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
        }
    }
}

/// Result of testing a single file.
#[derive(Debug)]
pub struct FileTestResult {
    pub path: PathBuf,
    pub passed: bool,
    pub skipped: bool,
    pub error: Option<String>,
    pub go_duration: Duration,
    pub gors_duration: Duration,
}

/// Summary of test results.
#[derive(Debug)]
pub struct TestSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failures: Vec<FileTestResult>,
    pub total_go_time: Duration,
    pub total_gors_time: Duration,
}

impl TestSummary {
    /// Create a new empty summary.
    pub fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            skipped: 0,
            failures: Vec::new(),
            total_go_time: Duration::ZERO,
            total_gors_time: Duration::ZERO,
        }
    }

    /// Merge another summary into this one.
    pub fn merge(&mut self, other: TestSummary) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.failures.extend(other.failures);
        self.total_go_time += other.total_go_time;
        self.total_gors_time += other.total_gors_time;
    }

    /// Check if all tests passed and panic with details if not.
    pub fn assert_all_passed(&self) {
        if self.failed == 0 {
            eprintln!(
                "\n{} tests passed, {} skipped (Go: {:?}, gors: {:?})",
                self.passed, self.skipped, self.total_go_time, self.total_gors_time
            );
            return;
        }

        let mut msg = format!(
            "\n{} tests FAILED, {} passed, {} skipped\n\nFailures:\n",
            self.failed, self.passed, self.skipped
        );

        // Show up to 10 failure details
        for (i, failure) in self.failures.iter().take(10).enumerate() {
            msg.push_str(&format!("\n{}. {}\n", i + 1, failure.path.display()));
            if let Some(ref error) = failure.error {
                // Truncate long error messages
                let error_preview: String = error.chars().take(500).collect();
                msg.push_str(&error_preview);
                if error.len() > 500 {
                    msg.push_str("\n... (truncated)");
                }
                msg.push('\n');
            }
        }

        if self.failures.len() > 10 {
            msg.push_str(&format!(
                "\n... and {} more failures\n",
                self.failures.len() - 10
            ));
        }

        panic!("{}", msg);
    }
}

impl Default for TestSummary {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the path to the fixtures directory.
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("tests/fixtures")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gors crate should live under workspace root")
        .to_path_buf()
}

pub fn go_command() -> Command {
    let mut command = Command::new(go_binary());
    command
        .env("GOROOT", gors::GO_SDK_PATH)
        .env("GOTOOLCHAIN", "local");
    command
}

fn go_binary() -> &'static PathBuf {
    static GO_BINARY: OnceLock<PathBuf> = OnceLock::new();
    GO_BINARY.get_or_init(|| {
        let binary_name = if cfg!(windows) { "go.exe" } else { "go" };
        let binary = Path::new(gors::GO_SDK_PATH).join("bin").join(binary_name);
        assert_pinned_go_version(&binary);
        binary
    })
}

fn assert_pinned_go_version(binary: &Path) {
    let actual = detected_go_version(binary)
        .unwrap_or_else(|error| panic!("failed to determine Go version from {binary:?}: {error}"));
    if actual != gors::GO_VERSION {
        panic!(
            "Go version mismatch: expected go{} from .go-version, got go{} from {}.",
            gors::GO_VERSION,
            actual,
            binary.display()
        );
    }
}

fn detected_go_version(binary: &Path) -> Result<String, String> {
    let output = Command::new(binary)
        .arg("version")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    parse_go_version(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_string)
        .ok_or_else(|| format!("unexpected `go version` output: {:?}", output.stdout))
}

fn parse_go_version(output: &str) -> Option<&str> {
    output.split_whitespace().find_map(|part| {
        part.strip_prefix("go")
            .filter(|version| version.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
    })
}

/// Get the path to the Go oracle helper binary, building it if needed.
pub fn go_oracle_bin() -> &'static PathBuf {
    static GO_ORACLE: OnceLock<PathBuf> = OnceLock::new();
    GO_ORACLE.get_or_init(|| {
        let runner_dir = workspace_root().join("tests/tools/go_oracle");
        let bin_path = runner_dir.join(format!(
            "go-oracle-go{}",
            gors::GO_VERSION.replace('.', "_")
        ));

        // Build the Go binary
        let status = go_command()
            .args([
                "build",
                "-buildvcs=false",
                "-o",
                bin_path.to_str().expect("valid path"),
                ".",
            ])
            .current_dir(&runner_dir)
            .status()
            .expect("Failed to build go_oracle");

        if !status.success() {
            panic!("Failed to build go_oracle binary");
        }

        bin_path
    })
}

/// Discover all program directories in fixtures/go_programs that have main.go.
pub fn discover_program_dirs() -> Vec<PathBuf> {
    let config = TestConfig::from_env();
    let programs_dir = fixtures_dir().join("go_programs");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&programs_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", programs_dir.display(), e))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("main.go").exists())
        .collect();

    collect_stdlib_program_dirs(&programs_dir.join("stdlib"), &mut dirs);
    dirs.retain(|p| program_matches_filter(&programs_dir, p, config.filter.as_deref()));
    dirs.sort();
    if let Some(limit) = config.limit {
        dirs.truncate(limit);
    }
    dirs
}

fn collect_stdlib_program_dirs(dir: &Path, dirs: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("main.go").exists() {
            dirs.push(path);
        } else {
            collect_stdlib_program_dirs(&path, dirs);
        }
    }
}

fn program_matches_filter(programs_dir: &Path, path: &Path, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| {
        path.strip_prefix(programs_dir)
            .ok()
            .and_then(|relative| relative.to_str())
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .is_some_and(|name| name.contains(filter))
    })
}

/// Collect all `.go` files from a directory recursively.
pub fn collect_go_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_go_files_recursive(dir, &mut files);
    files.sort(); // Deterministic ordering
    files
}

fn collect_go_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip if path doesn't exist (broken symlinks)
        if !path.exists() {
            continue;
        }

        // Skip paths with repeated components (recursive symlinks)
        if has_repeated_components(&path) {
            continue;
        }

        if path.is_dir() {
            collect_go_files_recursive(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "go") {
            files.push(path);
        }
    }
}

/// Check if a path has repeated directory components (sign of recursive symlinks).
fn has_repeated_components(path: &Path) -> bool {
    let mut seen: std::collections::HashMap<&std::ffi::OsStr, usize> =
        std::collections::HashMap::new();

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let count = seen.entry(name).or_insert(0);
            *count += 1;
            if *count >= 3 {
                return true;
            }
        }
    }
    false
}

/// Files that must error for both Go and gors parsers (intentionally invalid test data).
pub fn must_error_files() -> &'static HashSet<&'static str> {
    static MUST_ERROR_FILES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    MUST_ERROR_FILES.get_or_init(|| {
        let mut set = HashSet::new();
        // Go compiler test files where both Go and gors parsers fail
        for file in [
            "fixtures/go_repositories/go/test/switch2.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug014.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug050.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug068.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug088.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug106.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug121.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug163.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug222.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug228.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug228a.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug282.go",
            "fixtures/go_repositories/go/test/fixedbugs/bug298.go",
            "fixtures/go_repositories/go/test/fixedbugs/issue11610.go",
            "fixtures/go_repositories/go/test/fixedbugs/issue15611.go",
            "fixtures/go_repositories/go/test/fixedbugs/issue23587.go",
            "fixtures/go_repositories/go/test/fixedbugs/issue32133.go",
            "fixtures/go_repositories/go/test/fixedbugs/issue4405.go",
            "fixtures/go_repositories/go/test/fixedbugs/issue9036.go",
            "fixtures/go_repositories/go/test/slice3err.go",
            // Intentionally invalid syntax tests
            "fixtures/go_repositories/go/test/syntax/chan.go",
            "fixtures/go_repositories/go/test/syntax/chan1.go",
            "fixtures/go_repositories/go/test/syntax/composite.go",
            "fixtures/go_repositories/go/test/syntax/ddd.go",
            "fixtures/go_repositories/go/test/syntax/else.go",
            "fixtures/go_repositories/go/test/syntax/if.go",
            "fixtures/go_repositories/go/test/syntax/import.go",
            "fixtures/go_repositories/go/test/syntax/initvar.go",
            "fixtures/go_repositories/go/test/syntax/semi1.go",
            "fixtures/go_repositories/go/test/syntax/semi2.go",
            "fixtures/go_repositories/go/test/syntax/semi3.go",
            "fixtures/go_repositories/go/test/syntax/semi4.go",
            "fixtures/go_repositories/go/test/syntax/semi5.go",
            "fixtures/go_repositories/go/test/syntax/semi6.go",
            "fixtures/go_repositories/go/test/syntax/semi7.go",
            "fixtures/go_repositories/go/test/syntax/topexpr.go",
            "fixtures/go_repositories/go/test/syntax/vareq.go",
            "fixtures/go_repositories/go/test/syntax/vareq1.go",
            // Parser testdata
            "fixtures/go_repositories/go/src/go/parser/testdata/issue42951/not_a_file.go",
            "fixtures/go_repositories/go/src/go/parser/testdata/issue42951/not_a_file.go/invalid.go",
            // Invalid characters
            "fixtures/go_repositories/go/src/internal/types/testdata/local/issue68183.go",
            // Type checker test files
            "fixtures/go_repositories/go/src/internal/types/testdata/fixedbugs/issue39634.go",
            "fixtures/go_repositories/go/src/cmd/compile/internal/types2/testdata/fixedbugs/issue39634.go",
            "fixtures/go_repositories/go/src/internal/types/testdata/examples/types.go",
            "fixtures/go_repositories/go/src/cmd/compile/internal/types2/testdata/examples/types.go",
            "fixtures/go_repositories/go/test/func3.go",
            "fixtures/go_repositories/go/test/import5.go",
            "fixtures/go_repositories/go/test/char_lit1.go",
        ] {
            set.insert(file);
        }
        set
    })
}

#[derive(Debug, Deserialize)]
struct GoFileResult {
    path: String,
    ok: bool,
    #[serde(default)]
    stdout: String,
}

#[derive(Debug)]
struct GoOracleOutput {
    ok: bool,
    stdout: Vec<u8>,
    duration: Duration,
}

const FILE_TEST_BATCH_SIZE: usize = 256;
const FILE_TEST_STACK_SIZE: usize = 16 * 1024 * 1024;

fn go_oracle_chunk(
    go_bin: &Path,
    command: &str,
    files: &[&PathBuf],
) -> Result<BTreeMap<PathBuf, GoOracleOutput>, String> {
    if files.is_empty() {
        return Ok(BTreeMap::new());
    }
    if let [file] = files {
        return go_oracle_single(go_bin, command, file);
    }

    let mut args = Vec::with_capacity(files.len() + 1);
    args.push(command.to_string());
    for file in files {
        args.push(file.to_string_lossy().into_owned());
    }

    let before = std::time::Instant::now();
    let output = Command::new(go_bin)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to execute {:?}: {}", go_bin, e))?;
    let elapsed = before.elapsed();

    if !output.status.success() {
        return Err(format!(
            "{:?} {command} failed\nstdout: {}\nstderr: {}",
            go_bin,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let per_file = if files.is_empty() {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(elapsed.as_secs_f64() / files.len() as f64)
    };
    let mut results = BTreeMap::new();
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let result: GoFileResult = serde_json::from_slice(line)
            .map_err(|e| format!("failed to parse Go file output: {e}"))?;
        results.insert(
            PathBuf::from(result.path),
            GoOracleOutput {
                ok: result.ok,
                stdout: result.stdout.into_bytes(),
                duration: per_file,
            },
        );
    }
    Ok(results)
}

fn go_oracle_single(
    go_bin: &Path,
    command: &str,
    file: &Path,
) -> Result<BTreeMap<PathBuf, GoOracleOutput>, String> {
    let before = std::time::Instant::now();
    let output = Command::new(go_bin)
        .args([
            command,
            file.to_str().ok_or_else(|| "non-utf8 path".to_string())?,
        ])
        .output()
        .map_err(|e| format!("failed to execute {:?}: {}", go_bin, e))?;
    let elapsed = before.elapsed();

    let mut results = BTreeMap::new();
    results.insert(
        file.to_path_buf(),
        GoOracleOutput {
            ok: output.status.success(),
            stdout: output.stdout,
            duration: elapsed,
        },
    );
    Ok(results)
}

fn run_gors_in_memory(command: &str, file: &Path) -> Result<(Vec<u8>, Duration), String> {
    let before = std::time::Instant::now();
    let output = match command {
        "ast" => gors_ast_output(file),
        "tokens" => gors_tokens_output(file),
        other => Err(format!("unsupported in-memory gors command {other:?}")),
    }?;
    Ok((output, before.elapsed()))
}

fn gors_ast_output(file: &Path) -> Result<Vec<u8>, String> {
    let filename = file.to_str().ok_or_else(|| "non-utf8 path".to_string())?;
    let buffer = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let ast = gors::parser::parse_file(filename, &buffer).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    gors::ast::fprint(&mut output, ast).map_err(|e| e.to_string())?;
    Ok(output)
}

fn gors_tokens_output(file: &Path) -> Result<Vec<u8>, String> {
    let filename = file.to_str().ok_or_else(|| "non-utf8 path".to_string())?;
    let buffer = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    for step in gors::scanner::Scanner::new(filename, &buffer) {
        let token = step.map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut output, &token).map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    Ok(output)
}

/// Check if a file is in the must-error list.
pub fn is_must_error_file(path: &Path) -> bool {
    let fixtures_dir = fixtures_dir();
    if let Ok(relative) = path.strip_prefix(&fixtures_dir) {
        let relative_str = format!("fixtures/{}", relative.display());
        must_error_files().contains(relative_str.as_str())
    } else {
        false
    }
}

/// Test a single file with the given command.
fn test_single_file(
    command: &str,
    file: &Path,
    go_oracle: Option<&GoOracleOutput>,
) -> FileTestResult {
    let go_oracle = match go_oracle {
        Some(reference) => reference,
        None => {
            return FileTestResult {
                path: file.to_path_buf(),
                passed: false,
                skipped: false,
                error: Some("missing Go oracle output".to_string()),
                go_duration: Duration::ZERO,
                gors_duration: Duration::ZERO,
            };
        }
    };

    if !go_oracle.ok {
        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: true,
            error: None,
            go_duration: go_oracle.duration,
            gors_duration: Duration::ZERO,
        };
    }

    let (gors_stdout, gors_duration) = match run_gors_in_memory(command, file) {
        Ok(result) => result,
        Err(e) => {
            return FileTestResult {
                path: file.to_path_buf(),
                passed: false,
                skipped: false,
                error: Some(format!("gors failed: {e}")),
                go_duration: go_oracle.duration,
                gors_duration: Duration::ZERO,
            };
        }
    };

    if go_oracle.stdout != gors_stdout {
        let go_str = String::from_utf8_lossy(&go_oracle.stdout);
        let gors_str = String::from_utf8_lossy(&gors_stdout);
        let diff_info = find_first_diff(&go_str, &gors_str);

        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some(format!(
                "Output mismatch (command: {})\n{}",
                command, diff_info
            )),
            go_duration: go_oracle.duration,
            gors_duration,
        };
    }

    FileTestResult {
        path: file.to_path_buf(),
        passed: true,
        skipped: false,
        error: None,
        go_duration: go_oracle.duration,
        gors_duration,
    }
}

fn skipped_file_result(file: &Path) -> FileTestResult {
    FileTestResult {
        path: file.to_path_buf(),
        passed: false,
        skipped: true,
        error: None,
        go_duration: Duration::ZERO,
        gors_duration: Duration::ZERO,
    }
}

/// Find the first difference between two strings and return a helpful message.
fn find_first_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    for (i, (e, a)) in expected_lines.iter().zip(actual_lines.iter()).enumerate() {
        if e != a {
            return format!(
                "First difference at line {}:\n  expected: {}\n  actual:   {}",
                i + 1,
                e,
                a
            );
        }
    }

    if expected_lines.len() != actual_lines.len() {
        return format!(
            "Line count differs: expected {} lines, got {} lines",
            expected_lines.len(),
            actual_lines.len()
        );
    }

    "Unknown difference".to_string()
}

/// Test a must-error file (both parsers should fail).
fn test_must_error_file(file: &Path, go_oracle: Option<&GoOracleOutput>) -> FileTestResult {
    let Some(go_oracle) = go_oracle else {
        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some("missing Go oracle output".to_string()),
            go_duration: Duration::ZERO,
            gors_duration: Duration::ZERO,
        };
    };

    let go_failed = !go_oracle.ok;
    let gors_result = run_gors_in_memory("ast", file);
    let gors_failed = gors_result.is_err();
    let gors_duration = gors_result
        .as_ref()
        .map(|(_, duration)| *duration)
        .unwrap_or(Duration::ZERO);

    if !go_failed {
        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some("Go parser should have failed on must-error file".to_string()),
            go_duration: go_oracle.duration,
            gors_duration,
        };
    }

    if !gors_failed {
        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some("gors parser should have failed on must-error file".to_string()),
            go_duration: go_oracle.duration,
            gors_duration,
        };
    }

    FileTestResult {
        path: file.to_path_buf(),
        passed: true,
        skipped: false,
        error: None,
        go_duration: go_oracle.duration,
        gors_duration,
    }
}

fn summarize_file_results(results: Vec<FileTestResult>) -> TestSummary {
    let mut summary = TestSummary::new();

    for result in results {
        record_file_result(&mut summary, result);
    }

    summary
}

fn record_file_result(summary: &mut TestSummary, result: FileTestResult) {
    summary.total_go_time += result.go_duration;
    summary.total_gors_time += result.gors_duration;
    if result.skipped {
        summary.skipped += 1;
    } else if result.passed {
        summary.passed += 1;
    } else {
        summary.failed += 1;
        summary.failures.push(result);
    }
}

fn skipped_file_summary(files: &[&PathBuf]) -> TestSummary {
    let mut summary = TestSummary::new();
    summary.skipped = files.len();
    summary
}

fn record_progress(config: &TestConfig, processed: &AtomicUsize, total_files: usize) {
    if config.verbose {
        let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(100) {
            eprintln!("  Progress: {}/{}", count, total_files);
        }
    }
}

fn test_normal_file_batch(
    go_bin: &Path,
    command: &str,
    files: &[&PathBuf],
    config: &TestConfig,
    processed: &AtomicUsize,
    abort: &AtomicBool,
    total_files: usize,
) -> TestSummary {
    if config.fail_fast && abort.load(Ordering::SeqCst) {
        return skipped_file_summary(files);
    }

    let go_oracles = go_oracle_chunk(go_bin, command, files)
        .unwrap_or_else(|e| panic!("failed to collect Go oracle output: {e}"));
    let results = files
        .par_iter()
        .map(|file| {
            if config.fail_fast && abort.load(Ordering::SeqCst) {
                return skipped_file_result(file);
            }
            let result = test_single_file(command, file, go_oracles.get(file.as_path()));
            if config.fail_fast && !result.passed && !result.skipped {
                abort.store(true, Ordering::SeqCst);
            }
            record_progress(config, processed, total_files);
            result
        })
        .collect();

    summarize_file_results(results)
}

fn test_must_error_file_batch(
    go_bin: &Path,
    files: &[&PathBuf],
    config: &TestConfig,
    processed: &AtomicUsize,
    abort: &AtomicBool,
    total_files: usize,
) -> TestSummary {
    if config.fail_fast && abort.load(Ordering::SeqCst) {
        return skipped_file_summary(files);
    }

    let go_oracles = go_oracle_chunk(go_bin, "ast", files)
        .unwrap_or_else(|e| panic!("failed to collect Go must-error output: {e}"));
    let results = files
        .par_iter()
        .map(|file| {
            if config.fail_fast && abort.load(Ordering::SeqCst) {
                return skipped_file_result(file);
            }
            let result = test_must_error_file(file, go_oracles.get(file.as_path()));
            if config.fail_fast && !result.passed && !result.skipped {
                abort.store(true, Ordering::SeqCst);
            }
            record_progress(config, processed, total_files);
            result
        })
        .collect();

    summarize_file_results(results)
}

/// Test files with the given command, running in parallel.
/// Returns a summary with all results.
pub fn test_files_parallel(command: &str, files: &[PathBuf], config: &TestConfig) -> TestSummary {
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(FILE_TEST_STACK_SIZE)
        .build()
        .expect("failed to build file test thread pool");

    pool.install(|| test_files_parallel_in_pool(command, files, config))
}

fn test_files_parallel_in_pool(
    command: &str,
    files: &[PathBuf],
    config: &TestConfig,
) -> TestSummary {
    // Apply filters
    let mut files_to_test: Vec<_> = files
        .iter()
        .filter(|f| {
            if let Some(ref filter) = config.filter {
                f.to_str().is_some_and(|s| s.contains(filter))
            } else {
                true
            }
        })
        .collect();

    // Apply limit
    if let Some(limit) = config.limit {
        files_to_test.truncate(limit);
    }

    // Partition into normal and must-error files
    let (normal_files, must_error_files): (Vec<_>, Vec<_>) = files_to_test
        .into_iter()
        .partition(|f| !is_must_error_file(f));

    let total_files = normal_files.len() + must_error_files.len();
    let processed = AtomicUsize::new(0);
    let abort = AtomicBool::new(false);

    if config.verbose {
        eprintln!(
            "Testing {} files with command '{}'...",
            total_files, command
        );
    }

    let mut summary = TestSummary::new();
    let go_bin = go_oracle_bin();

    // Keep oracle AST output bounded; storing the full repository corpus can
    // exhaust hosted CI memory before any gors-side progress is reported.
    let normal_summaries: Vec<TestSummary> = normal_files
        .par_chunks(FILE_TEST_BATCH_SIZE)
        .map(|chunk| {
            test_normal_file_batch(
                go_bin,
                command,
                chunk,
                config,
                &processed,
                &abort,
                total_files,
            )
        })
        .collect();
    for batch_summary in normal_summaries {
        summary.merge(batch_summary);
    }

    if command == "ast" {
        let must_error_summaries: Vec<TestSummary> = must_error_files
            .par_chunks(FILE_TEST_BATCH_SIZE)
            .map(|chunk| {
                test_must_error_file_batch(go_bin, chunk, config, &processed, &abort, total_files)
            })
            .collect();
        for batch_summary in must_error_summaries {
            summary.merge(batch_summary);
        }
    }

    // Count skipped must-error files for lexer
    if command != "ast" {
        summary.skipped += must_error_files.len();
    }

    summary
}
