//! Test runner infrastructure.
//!
//! This module provides utilities for running tests against Go files,
//! comparing outputs with the Go reference implementation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::Duration;

/// Get the path to the fixtures directory.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_suite/fixtures")
}

/// Get the path to the Go runner binary, building it if needed.
pub fn go_runner_bin() -> &'static PathBuf {
    static GO_RUNNER: OnceLock<PathBuf> = OnceLock::new();
    GO_RUNNER.get_or_init(|| {
        let runner_dir = fixtures_dir().join("go_runner");
        let bin_path = runner_dir.join("gors-go");

        // Build the Go binary
        let status = Command::new("go")
            .args(["build", "-o", bin_path.to_str().unwrap(), "."])
            .current_dir(&runner_dir)
            .status()
            .expect("Failed to build Go runner");

        if !status.success() {
            panic!("Failed to build Go runner binary");
        }

        bin_path
    })
}

/// Collect all `.go` files from a directory recursively.
pub fn collect_go_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_go_files_recursive(dir, &mut files);
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
        } else if path.extension().map_or(false, |ext| ext == "go") {
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
        // Go compiler test files with intentional syntax errors
        for file in [
            "fixtures/go_sources/repositories/go/test/bombad.go",
            "fixtures/go_sources/repositories/go/test/char_lit1.go",
            "fixtures/go_sources/repositories/go/test/const2.go",
            "fixtures/go_sources/repositories/go/test/switch2.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug014.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug050.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug068.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug088.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug106.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug121.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug163.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug169.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug222.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug228.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug228a.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug274.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug280.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug282.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug287.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/bug298.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue11359.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue11610.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue15611.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue23587.go",
            "fixtures/go_sources/repositories/go/test/fixedbugs/issue30722.go",
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
            "fixtures/go_sources/repositories/go/test/syntax/typesw.go",
            "fixtures/go_sources/repositories/go/test/syntax/vareq.go",
            "fixtures/go_sources/repositories/go/test/syntax/vareq1.go",
            "fixtures/go_sources/repositories/go/test/ddd1.go",
            // Files using //line directives
            "fixtures/go_sources/repositories/go/test/dwarf/linedirectives.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z1.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z2.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z3.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z4.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z5.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z6.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z7.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z8.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z9.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z10.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z11.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z12.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z13.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z14.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z15.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z16.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z17.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z18.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z19.go",
            "fixtures/go_sources/repositories/go/test/dwarf/dwarf.dir/z20.go",
            // Parser testdata
            "fixtures/go_sources/repositories/go/src/go/parser/testdata/issue42951/not_a_file.go",
            "fixtures/go_sources/repositories/go/src/go/parser/testdata/issue42951/not_a_file.go/invalid.go",
            // Invalid characters
            "fixtures/go_sources/repositories/go/src/cmd/compile/internal/types2/testdata/local/issue68183.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/local/issue68183.go",
            // Type checker test files
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/fixedbugs/issue39634.go",
            "fixtures/go_sources/repositories/go/src/cmd/compile/internal/types2/testdata/fixedbugs/issue39634.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/examples/types.go",
            "fixtures/go_sources/repositories/go/src/cmd/compile/internal/types2/testdata/examples/types.go",
            "fixtures/go_sources/repositories/go/test/func3.go",
            "fixtures/go_sources/repositories/go/test/import5.go",
            // Cgo test file
            "fixtures/go_sources/repositories/go/src/cmd/cgo/internal/testerrors/testdata/err5.go",
            // Type checker tests
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/check/stmt0.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/check/typeinst0.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/check/typeparams.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/check/vardecl.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/examples/functions.go",
            "fixtures/go_sources/repositories/go/test/typeparam/issue50317.go",
            "fixtures/go_sources/repositories/go/src/internal/types/testdata/fixedbugs/issue43087.go",
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
