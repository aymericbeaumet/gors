// Tests may use unwrap and panic for assertions
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use colored::*;
use console::{Style, style};
use crossbeam::thread;
use glob::glob;
use phf::{Set, phf_set};
use similar::{ChangeTag, TextDiff};
use std::fmt;
use std::process::{Command, Output};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref RUNNER: TestRunner<'static> = TestRunner::new();
}

#[test]
fn compiler() {
    RUNNER.test("run", &["programs"]);
}

#[test]
fn lexer() {
    RUNNER.test("tokens", &["files", "repositories", "programs"]);
}

#[test]
fn parser() {
    RUNNER.test("ast", &["files", "repositories", "programs"]);
}

/// Test source map generation for compilable programs.
/// Validates that source maps are generated in standard v3 format
/// and can be parsed back by the sourcemap crate.
#[test]
fn sourcemap() {
    // Test with the programs that can be compiled
    for entry in glob("tests/programs/**/*.go").unwrap() {
        let path = match entry {
            Ok(p) => p,
            Err(_) => continue,
        };
        let go_file = path.to_str().unwrap();

        // Read the Go source
        let go_source = std::fs::read_to_string(go_file).unwrap();

        // Parse
        let ast = match gors::parser::parse_file(go_file, &go_source) {
            Ok(ast) => ast,
            Err(_) => continue, // Skip files that don't parse
        };

        // Compile with source map tracking
        let compiled = match gors::compiler::compile_with_source_map(ast, go_file, &go_source) {
            Ok(compiled) => compiled,
            Err(_) => continue, // Skip files that don't compile
        };

        // Generate Rust code
        let rust_source = gors::codegen::generate(compiled).unwrap();

        // Build the source map
        let source_map = gors::compiler::build_source_map(&rust_source);

        // Validate: serialize and parse back (round-trip)
        let mut buf = Vec::new();
        source_map.to_writer(&mut buf).unwrap();
        let parsed = sourcemap::SourceMap::from_reader(&buf[..])
            .unwrap_or_else(|e| panic!("Invalid sourcemap for {}: {}", go_file, e));

        // Basic validation
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

        println!("| sourcemap OK: {} ({} tokens)", go_file, parsed.get_token_count());
    }
}

/// Test specific files passed via GORS_TEST_FILES environment variable.
/// Files should be separated by newlines or commas.
/// Example: GORS_TEST_FILES="tests/repositories/go/test/foo.go,tests/repositories/go/test/bar.go" cargo test specific_files
#[test]
fn specific_files() {
    if let Ok(files) = std::env::var("GORS_TEST_FILES") {
        let files: Vec<String> = files
            .split(|c| c == '\n' || c == ',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !files.is_empty() {
            // Use the command from GORS_TEST_CMD env var, defaulting to "ast"
            let command = std::env::var("GORS_TEST_CMD").unwrap_or_else(|_| "ast".to_string());
            RUNNER.test_files(&command, files);
        }
    }
}

#[derive(Debug)]
struct TestRunner<'a> {
    gors_bin: &'a str,
    gors_go_bin: &'a str,
    pattern: &'a str,
    thread_count: usize,
}

impl<'a> TestRunner<'a> {
    fn new() -> Self {
        let go_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
        let gors_go_bin = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/gors-go");

        print!("\n| building gors-go binary...");
        Command::new("go")
            .args(["build", "-o", gors_go_bin, "."])
            .current_dir(&go_dir)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();

        // Always test all .go files recursively (including submodules)
        // Run `make setup` first to initialize submodules
        let gors_bin = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/release/gors");
        let pattern = "**/*.go";
        let thread_count = num_cpus::get();

        Self {
            gors_bin,
            gors_go_bin,
            pattern,
            thread_count,
        }
    }

    fn test(&self, command: &str, prefixes: &[&str]) {
        // Collect all files, separating must-error files from normal files
        let mut go_files: Vec<String> = Vec::new();
        let mut must_error_files: Vec<String> = Vec::new();

        for prefix in prefixes {
            for entry in glob(&format!("tests/{}/{}", prefix, self.pattern)).unwrap() {
                // Skip glob errors (e.g., symlinks that create paths that are too long)
                let path = match entry {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let path_str = path.to_str().unwrap();

                // Skip files that don't exist (broken symlinks)
                if !std::path::Path::new(path_str).exists() {
                    continue;
                }

                // Skip paths with repeated directory components (recursive symlinks)
                if has_repeated_components(path_str) {
                    continue;
                }

                if MUST_ERROR_FILES.contains(path_str) {
                    must_error_files.push(path_str.to_owned());
                } else {
                    go_files.push(path_str.to_owned());
                }
            }
        }

        // Test normal files (should succeed and match)
        self.test_files(command, go_files);

        // Test must-error files only for the parser command (these have valid tokens but invalid syntax)
        // The lexer can scan them successfully, but the parser must reject them.
        if command == "ast" {
            self.test_must_error_files(command, must_error_files);
        }
    }

    fn test_files(&self, command: &str, go_files: Vec<String>) {
        println!("\n| found {} go files", go_files.len());
        if go_files.is_empty() {
            return;
        }

        let (go_elapsed, rust_elapsed) = thread::scope(|scope| {
            #[allow(clippy::needless_collect)] // We collect to start the threads in parallel
            let handles: Vec<_> = go_files
                .chunks((go_files.len() / self.thread_count) + 1)
                .enumerate()
                .map(|(i, chunk)| {
                    println!("| starting thread #{} (chunk_len={})", i, chunk.len());
                    scope.spawn(|_| {
                        chunk.iter().fold(
                            (Duration::new(0, 0), Duration::new(0, 0)),
                            |acc, go_file| {
                                let args = &[command, go_file.as_str()];
                                let (go_output, go_elapsed) = exec(self.gors_go_bin, args).unwrap();
                                let (rust_output, rust_elapsed) =
                                    exec(self.gors_bin, args).unwrap();

                                if go_output.stdout != rust_output.stdout {
                                    println!("| diff found: {}", go_file);
                                    print_diff(
                                        std::str::from_utf8(&go_output.stdout).unwrap(),
                                        std::str::from_utf8(&rust_output.stdout).unwrap(),
                                    );
                                    std::process::exit(1);
                                }

                                (acc.0 + go_elapsed, acc.1 + rust_elapsed)
                            },
                        )
                    })
                })
                .collect();

            handles
                .into_iter()
                .fold((Duration::new(0, 0), Duration::new(0, 0)), |acc, handle| {
                    let (g, r) = handle.join().unwrap();
                    (acc.0 + g, acc.1 + r)
                })
        })
        .unwrap();

        println!("| total elapsed time:");
        println!("|   go:   {:?}", go_elapsed);
        println!(
            "|   rust: {:?} (go {:+.2}%)",
            rust_elapsed,
            ((rust_elapsed.as_secs_f64() / go_elapsed.as_secs_f64()) - 1.0) * 100.0
        );
    }

    /// Test files that must error for both Go and gors parsers.
    /// These are intentionally invalid Go files used to test error handling.
    fn test_must_error_files(&self, command: &str, files: Vec<String>) {
        if files.is_empty() {
            return;
        }

        println!("\n| testing {} must-error files", files.len());

        thread::scope(|scope| {
            let handles: Vec<_> = files
                .chunks((files.len() / self.thread_count) + 1)
                .map(|chunk| {
                    scope.spawn(move |_| {
                        for file in chunk {
                            let args = &[command, file.as_str()];

                            // Run Go parser - should fail
                            let go_result = exec_allow_failure(self.gors_go_bin, args);
                            let go_failed = go_result.is_err()
                                || !go_result.as_ref().unwrap().0.status.success();

                            // Run gors parser - should also fail
                            let rust_result = exec_allow_failure(self.gors_bin, args);
                            let rust_failed = rust_result.is_err()
                                || !rust_result.as_ref().unwrap().0.status.success();

                            if !go_failed {
                                eprintln!(
                                    "| ERROR: Go parser should have failed on must-error file: {}",
                                    file
                                );
                                std::process::exit(1);
                            }

                            if !rust_failed {
                                eprintln!(
                                    "| ERROR: gors parser should have failed on must-error file: {}",
                                    file
                                );
                                std::process::exit(1);
                            }
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
        })
        .unwrap();

        println!("| all must-error files correctly rejected by both parsers");
    }
}

fn exec(bin: &str, args: &[&str]) -> Result<(Output, Duration), Box<dyn std::error::Error>> {
    let before = std::time::Instant::now();
    let output = Command::new(bin).args(args).output()?;
    let after = std::time::Instant::now();

    if !output.status.success() {
        // Only log details if there's an exit code (not killed by signal)
        if let Some(code) = output.status.code() {
            eprintln!("STATUS: {}", code);
            eprintln!(
                "STDOUT: {}",
                std::str::from_utf8(&output.stdout).unwrap().blue(),
            );
            eprintln!(
                "STDERR: {}",
                std::str::from_utf8(&output.stderr).unwrap().blue(),
            );
        }
        return Err(format!("{} {:?} failed", bin, args,).into());
    }

    Ok((output, after.checked_duration_since(before).unwrap()))
}

/// Check if a path has repeated directory components (sign of recursive symlinks).
/// For example: "foo/bar/v3/v3/v3/file.go" has repeated "v3" components.
fn has_repeated_components(path: &str) -> bool {
    use std::collections::HashMap;
    let components: Vec<&str> = path.split('/').collect();
    let mut seen: HashMap<&str, usize> = HashMap::new();

    for component in &components {
        // Skip empty components and common directory names
        if component.is_empty() || *component == "." || *component == ".." {
            continue;
        }
        let count = seen.entry(component).or_insert(0);
        *count += 1;
        // If we see the same directory 3+ times, it's likely a recursive symlink
        if *count >= 3 {
            return true;
        }
    }
    false
}

/// Execute a command and return the result without failing on non-zero exit codes.
fn exec_allow_failure(
    bin: &str,
    args: &[&str],
) -> Result<(Output, Duration), Box<dyn std::error::Error>> {
    let before = std::time::Instant::now();
    let output = Command::new(bin).args(args).output()?;
    let after = std::time::Instant::now();
    Ok((output, after.checked_duration_since(before).unwrap()))
}

// Files that must error for both Go and gors parsers (intentionally invalid test data).
// These files are used to test error handling - both parsers must reject them.
static MUST_ERROR_FILES: Set<&'static str> = phf_set! {
    // Go compiler test files with intentional syntax errors
    "tests/repositories/go/test/bombad.go",
    "tests/repositories/go/test/char_lit1.go",
    "tests/repositories/go/test/const2.go",
    "tests/repositories/go/test/switch2.go",
    "tests/repositories/go/test/fixedbugs/bug014.go",
    "tests/repositories/go/test/fixedbugs/bug050.go",
    "tests/repositories/go/test/fixedbugs/bug068.go",
    "tests/repositories/go/test/fixedbugs/bug088.go",
    "tests/repositories/go/test/fixedbugs/bug106.go",
    "tests/repositories/go/test/fixedbugs/bug121.go",
    "tests/repositories/go/test/fixedbugs/bug163.go",
    "tests/repositories/go/test/fixedbugs/bug169.go",
    "tests/repositories/go/test/fixedbugs/bug222.go",
    "tests/repositories/go/test/fixedbugs/bug228.go",
    "tests/repositories/go/test/fixedbugs/bug228a.go",
    "tests/repositories/go/test/fixedbugs/bug274.go",
    "tests/repositories/go/test/fixedbugs/bug280.go",
    "tests/repositories/go/test/fixedbugs/bug282.go",
    "tests/repositories/go/test/fixedbugs/bug287.go",
    "tests/repositories/go/test/fixedbugs/bug298.go",
    "tests/repositories/go/test/fixedbugs/issue11359.go",
    "tests/repositories/go/test/fixedbugs/issue11610.go",
    "tests/repositories/go/test/fixedbugs/issue15611.go",
    "tests/repositories/go/test/fixedbugs/issue23587.go",
    "tests/repositories/go/test/fixedbugs/issue30722.go",
    "tests/repositories/go/test/fixedbugs/issue32133.go",
    "tests/repositories/go/test/fixedbugs/issue4405.go",
    "tests/repositories/go/test/fixedbugs/issue9036.go",
    "tests/repositories/go/test/slice3err.go",
    // Intentionally invalid syntax tests in go/test/syntax/
    "tests/repositories/go/test/syntax/chan.go",
    "tests/repositories/go/test/syntax/chan1.go",
    "tests/repositories/go/test/syntax/composite.go",
    "tests/repositories/go/test/syntax/ddd.go",
    "tests/repositories/go/test/syntax/else.go",
    "tests/repositories/go/test/syntax/if.go",
    "tests/repositories/go/test/syntax/import.go",
    "tests/repositories/go/test/syntax/initvar.go",
    "tests/repositories/go/test/syntax/semi1.go",
    "tests/repositories/go/test/syntax/semi2.go",
    "tests/repositories/go/test/syntax/semi3.go",
    "tests/repositories/go/test/syntax/semi4.go",
    "tests/repositories/go/test/syntax/semi5.go",
    "tests/repositories/go/test/syntax/semi6.go",
    "tests/repositories/go/test/syntax/semi7.go",
    "tests/repositories/go/test/syntax/topexpr.go",
    "tests/repositories/go/test/syntax/typesw.go",
    "tests/repositories/go/test/syntax/vareq.go",
    "tests/repositories/go/test/syntax/vareq1.go",
    "tests/repositories/go/test/ddd1.go",
    // Files using //line directives that affect position reporting (DWARF/debugging features)
    "tests/repositories/go/test/dwarf/linedirectives.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z1.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z2.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z3.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z4.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z5.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z6.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z7.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z8.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z9.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z10.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z11.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z12.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z13.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z14.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z15.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z16.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z17.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z18.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z19.go",
    "tests/repositories/go/test/dwarf/dwarf.dir/z20.go",
    // Intentionally missing file (referenced in Go parser testdata)
    "tests/repositories/go/src/go/parser/testdata/issue42951/not_a_file.go",
    // Invalid Go file in a path that looks like a file (Go parser testdata)
    "tests/repositories/go/src/go/parser/testdata/issue42951/not_a_file.go/invalid.go",
    // Files with intentionally invalid characters (testing Go compiler error handling)
    "tests/repositories/go/src/cmd/compile/internal/types2/testdata/local/issue68183.go",
    "tests/repositories/go/src/internal/types/testdata/local/issue68183.go",
    // Go type checker test files with intentional syntax errors
    "tests/repositories/go/src/internal/types/testdata/fixedbugs/issue39634.go",
    "tests/repositories/go/src/cmd/compile/internal/types2/testdata/fixedbugs/issue39634.go",
    "tests/repositories/go/src/internal/types/testdata/examples/types.go",
    "tests/repositories/go/src/cmd/compile/internal/types2/testdata/examples/types.go",
    "tests/repositories/go/test/func3.go",
    "tests/repositories/go/test/import5.go",
    // Cgo test file using //line directive that redirects positions to /tmp/_cgo_.go
    "tests/repositories/go/src/cmd/cgo/internal/testerrors/testdata/err5.go",
    // Go type checker test files with intentional statement errors
    "tests/repositories/go/src/internal/types/testdata/check/stmt0.go",
    // Go type instantiation test files with intentional syntax errors
    "tests/repositories/go/src/internal/types/testdata/check/typeinst0.go",
    // Go type parameter test files with intentional syntax errors (method with type params)
    "tests/repositories/go/src/internal/types/testdata/check/typeparams.go",
    // Go var declaration test files with intentional syntax errors
    "tests/repositories/go/src/internal/types/testdata/check/vardecl.go",
    // Go function test files with intentional syntax errors (empty type param list)
    "tests/repositories/go/src/internal/types/testdata/examples/functions.go",
    // Go type param test files with intentional syntax errors (method with type params)
    "tests/repositories/go/test/typeparam/issue50317.go",
    // Go type checker test files with intentional short var declaration errors
    "tests/repositories/go/src/internal/types/testdata/fixedbugs/issue43087.go",
};

// https://github.com/mitsuhiko/similar/blob/main/examples/terminal-inline.rs

struct Line(Option<usize>);

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => write!(f, "    "),
            Some(idx) => write!(f, "{:<4}", idx + 1),
        }
    }
}

fn print_diff(expected: &str, got: &str) {
    let diff = TextDiff::from_lines(expected, got);

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            println!("{:-^1$}", "-", 80);
        }
        for op in group {
            for change in diff.iter_inline_changes(op) {
                let (sign, s) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::new().red()),
                    ChangeTag::Insert => ("+", Style::new().green()),
                    ChangeTag::Equal => (" ", Style::new().dim()),
                };
                print!(
                    "{}{} |{}",
                    style(Line(change.old_index())).dim(),
                    style(Line(change.new_index())).dim(),
                    s.apply_to(sign).bold(),
                );
                for (emphasized, value) in change.iter_strings_lossy() {
                    if emphasized {
                        print!("{}", s.apply_to(value).underlined().on_black());
                    } else {
                        print!("{}", s.apply_to(value));
                    }
                }
                if change.missing_newline() {
                    println!();
                }
            }
        }
    }
}
