//! Test runner infrastructure.
//!
//! This module provides utilities for running tests against Go files,
//! comparing outputs with the Go reference implementation.
//!
//! ## Environment Variables
//!
//! - `GORS_TEST_LIMIT`: Maximum number of files to test (default: unlimited)
//! - `GORS_TEST_FILTER`: Only test files matching this substring
//! - `GORS_TEST_VERBOSE`: Show progress during testing (set to "1" to enable)
//! - `GORS_TEST_FAIL_FAST`: Cancel queued/running program tests after the first failure

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gors crate should live under workspace root")
        .to_path_buf()
}

/// Get the path to the Go runner binary, building it if needed.
pub fn go_runner_bin() -> &'static PathBuf {
    static GO_RUNNER: OnceLock<PathBuf> = OnceLock::new();
    GO_RUNNER.get_or_init(|| {
        let runner_dir = fixtures_dir().join("go_runner");
        let bin_path = runner_dir.join("gors-go");

        // Build the Go binary
        let status = Command::new("go")
            .args([
                "build",
                "-buildvcs=false",
                "-o",
                bin_path.to_str().expect("valid path"),
                ".",
            ])
            .current_dir(&runner_dir)
            .status()
            .expect("Failed to build Go runner");

        if !status.success() {
            panic!("Failed to build Go runner binary");
        }

        bin_path
    })
}

/// Get the gors binary path, building it if needed.
pub fn gors_bin() -> &'static PathBuf {
    static GORS_BIN: OnceLock<PathBuf> = OnceLock::new();
    GORS_BIN.get_or_init(|| {
        // Build in release mode for accurate timing comparisons
        let status = std::process::Command::new("cargo")
            .args(["build", "--release", "-p", "gors-cli", "--bin", "gors"])
            .current_dir(workspace_root())
            .status()
            .expect("Failed to build gors");

        if !status.success() {
            panic!("Failed to build gors");
        }

        workspace_root().join("target/release/gors")
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

/// Execute a command and return the output and elapsed time.
pub fn exec(bin: &Path, args: &[&str]) -> Result<(Output, Duration), String> {
    let before = std::time::Instant::now();
    let output = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute {:?}: {}", bin, e))?;
    let elapsed = before.elapsed();

    if !output.status.success() {
        if let Some(code) = output.status.code() {
            return Err(format!(
                "{:?} {:?} failed with code {}\nstdout: {}\nstderr: {}",
                bin,
                args,
                code,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        return Err(format!("{:?} {:?} killed by signal", bin, args));
    }

    Ok((output, elapsed))
}

/// Execute a command allowing failures (for testing error cases).
pub fn exec_allow_failure(bin: &Path, args: &[&str]) -> Result<(Output, Duration), String> {
    let before = std::time::Instant::now();
    let output = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute {:?}: {}", bin, e))?;
    let elapsed = before.elapsed();
    Ok((output, elapsed))
}

/// Files that must error for both Go and gors parsers (intentionally invalid test data).
pub fn must_error_files() -> &'static HashSet<&'static str> {
    static MUST_ERROR_FILES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    MUST_ERROR_FILES.get_or_init(|| {
        let mut set = HashSet::new();
        // Go compiler test files where both Go and gors parsers fail
        for file in [
            "fixtures/go_sources/repositories/go/test/switch2.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug014.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug050.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug068.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug088.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug106.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug121.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug163.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug222.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug228.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug228a.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug282.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug298.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue11610.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue15611.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue23587.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue32133.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue4405.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue9036.go",
            "fixtures/go_sources/repositories/go/test/slice3err.go",
            // Intentionally invalid syntax tests
            "fixtures/go_sources/repositories/go/test/syntax/chan.go",
            "fixtures/go_sources/repositories/go/test/syntax/chan1.go",
            "fixtures/go_sources/repositories/go/test/syntax/composite.go",
            "fixtures/go_sources/repositories/go/test/syntax/ddd.go",
            "fixtures/go_sources/repositories/go/test/syntax/else.go",
            "fixtures/go_sources/repositories/go/test/syntax/if.go",
            "fixtures/go_sources/repositories/go/test/syntax/import.go",
            "fixtures/go_sources/repositories/go/test/syntax/initvar.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi1.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi2.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi3.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi4.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi5.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi6.go",
            "fixtures/go_sources/repositories/go/test/syntax/semi7.go",
            "fixtures/go_sources/repositories/go/test/syntax/topexpr.go",
            "fixtures/go_sources/repositories/go/test/syntax/vareq.go",
            "fixtures/go_sources/repositories/go/test/syntax/vareq1.go",
            // Parser testdata
            "fixtures/go_sources/repositories/go/src/go/parser/testdata/issue42951/not_a_file.go",
            "fixtures/go_sources/repositories/go/src/go/parser/testdata/issue42951/not_a_file.go/invalid.go",
            // Invalid characters
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/local/issue68183.go",
            // Type checker test files
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/fixedbugs/issue39634.go",
            "fixtures/go_sources/repositories/go/src/cmd/compile/internal/types2/testdata/fixedbugs/issue39634.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/examples/types.go",
            "fixtures/go_sources/repositories/go/src/cmd/compile/internal/types2/testdata/examples/types.go",
            "fixtures/go_sources/repositories/go/test/func3.go",
            "fixtures/go_sources/repositories/go/test/import5.go",
            "fixtures/go_sources/repositories/go/test/char_lit1.go",
        ] {
            set.insert(file);
        }
        set
    })
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
fn test_single_file(command: &str, file: &Path, go_bin: &Path, gors_bin: &Path) -> FileTestResult {
    let file_str = file.to_str().expect("valid path");
    let args = [command, file_str];

    // Run Go reference — skip if Go's parser can't handle the file
    let (go_output, go_duration) = match exec(go_bin, &args) {
        Ok(result) => result,
        Err(_) => {
            return FileTestResult {
                path: file.to_path_buf(),
                passed: false,
                skipped: true,
                error: None,
                go_duration: Duration::ZERO,
                gors_duration: Duration::ZERO,
            };
        }
    };

    // Run gors
    let (gors_output, gors_duration) = match exec(gors_bin, &args) {
        Ok(result) => result,
        Err(e) => {
            return FileTestResult {
                path: file.to_path_buf(),
                passed: false,
                skipped: false,
                error: Some(format!("gors failed: {}", e)),
                go_duration,
                gors_duration: Duration::ZERO,
            };
        }
    };

    // Compare outputs
    if go_output.stdout != gors_output.stdout {
        let go_str = String::from_utf8_lossy(&go_output.stdout);
        let gors_str = String::from_utf8_lossy(&gors_output.stdout);
        let diff_info = find_first_diff(&go_str, &gors_str);

        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some(format!(
                "Output mismatch (command: {})\n{}",
                command, diff_info
            )),
            go_duration,
            gors_duration,
        };
    }

    FileTestResult {
        path: file.to_path_buf(),
        passed: true,
        skipped: false,
        error: None,
        go_duration,
        gors_duration,
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
fn test_must_error_file(file: &Path, go_bin: &Path, gors_bin: &Path) -> FileTestResult {
    let file_str = file.to_str().expect("valid path");
    let args = ["ast", file_str];

    let go_result = exec_allow_failure(go_bin, &args);
    let go_failed = go_result.is_err()
        || !go_result
            .as_ref()
            .map(|r| r.0.status.success())
            .unwrap_or(false);

    let gors_result = exec_allow_failure(gors_bin, &args);
    let gors_failed = gors_result.is_err()
        || !gors_result
            .as_ref()
            .map(|r| r.0.status.success())
            .unwrap_or(false);

    let go_duration = go_result.as_ref().map(|r| r.1).unwrap_or(Duration::ZERO);
    let gors_duration = gors_result.as_ref().map(|r| r.1).unwrap_or(Duration::ZERO);

    if !go_failed {
        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some("Go parser should have failed on must-error file".to_string()),
            go_duration,
            gors_duration,
        };
    }

    if !gors_failed {
        return FileTestResult {
            path: file.to_path_buf(),
            passed: false,
            skipped: false,
            error: Some("gors parser should have failed on must-error file".to_string()),
            go_duration,
            gors_duration,
        };
    }

    FileTestResult {
        path: file.to_path_buf(),
        passed: true,
        skipped: false,
        error: None,
        go_duration,
        gors_duration,
    }
}

/// Test files with the given command, running in parallel.
/// Returns a summary with all results.
pub fn test_files_parallel(command: &str, files: &[PathBuf], config: &TestConfig) -> TestSummary {
    let go_bin = go_runner_bin();
    let gors = gors_bin();

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

    if config.verbose {
        eprintln!(
            "Testing {} files with command '{}'...",
            total_files, command
        );
    }

    // Test normal files in parallel
    let normal_results: Vec<FileTestResult> = normal_files
        .par_iter()
        .map(|file| {
            let result = test_single_file(command, file, go_bin, gors);
            if config.verbose {
                let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
                if count.is_multiple_of(100) {
                    eprintln!("  Progress: {}/{}", count, total_files);
                }
            }
            result
        })
        .collect();

    // Test must-error files in parallel (only for parser)
    let must_error_results: Vec<FileTestResult> = if command == "ast" {
        must_error_files
            .par_iter()
            .map(|file| {
                let result = test_must_error_file(file, go_bin, gors);
                if config.verbose {
                    let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
                    if count.is_multiple_of(100) {
                        eprintln!("  Progress: {}/{}", count, total_files);
                    }
                }
                result
            })
            .collect()
    } else {
        Vec::new()
    };

    // Build summary
    let mut summary = TestSummary::new();

    for result in normal_results {
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

    for result in must_error_results {
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

    // Count skipped must-error files for lexer
    if command != "ast" {
        summary.skipped += must_error_files.len();
    }

    summary
}
