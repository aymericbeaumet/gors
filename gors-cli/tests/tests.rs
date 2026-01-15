use colored::*;
use console::{style, Style};
use crossbeam::thread;
use glob::glob;
use phf::{phf_set, Set};
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
    RUNNER.test("tokens", &["files", "programs"]);
}

#[test]
fn parser() {
    RUNNER.test("ast", &["files", "programs"]);
}

/// Test specific files passed via GORS_TEST_FILES environment variable.
/// Files should be separated by newlines or commas.
/// Example: GORS_TEST_FILES="tests/files/go/test/foo.go,tests/files/go/test/bar.go" cargo test specific_files
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
        let go_files: Vec<_> = prefixes
            .iter()
            .flat_map(|prefix| glob(&format!("tests/{}/{}", prefix, self.pattern)).unwrap())
            .filter_map(|entry| {
                let path = entry.unwrap();
                let path = path.to_str().unwrap();
                if IGNORE_FILES.contains(path) {
                    None
                } else {
                    Some(path.to_owned())
                }
            })
            .collect();
        self.test_files(command, go_files);
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
}

fn exec(bin: &str, args: &[&str]) -> Result<(Output, Duration), Box<dyn std::error::Error>> {
    let before = std::time::Instant::now();
    let output = Command::new(bin).args(args).output()?;
    let after = std::time::Instant::now();

    if !output.status.success() {
        eprintln!(
            "STATUS: {}",
            output
                .status
                .code()
                .unwrap_or_else(|| panic!("{:?} {:?}", bin, args))
        );
        eprintln!(
            "STDOUT: {}",
            std::str::from_utf8(&output.stdout).unwrap().blue(),
        );
        eprintln!(
            "STDERR: {}",
            std::str::from_utf8(&output.stderr).unwrap().blue(),
        );
        return Err(format!("{} {:?} failed", bin, args,).into());
    }

    Ok((output, after.checked_duration_since(before).unwrap()))
}

// Files that cannot be parsed by the Go compiler (intentionally invalid test data).
// These files are used to test error handling in the Go compiler.
static IGNORE_FILES: Set<&'static str> = phf_set! {
    // Go compiler test files with intentional syntax errors
    "tests/files/go/test/bombad.go",
    "tests/files/go/test/char_lit1.go",
    "tests/files/go/test/const2.go",
    "tests/files/go/test/switch2.go",
    "tests/files/go/test/fixedbugs/bug014.go",
    "tests/files/go/test/fixedbugs/bug068.go",
    "tests/files/go/test/fixedbugs/bug163.go",
    "tests/files/go/test/fixedbugs/bug169.go",
    "tests/files/go/test/fixedbugs/issue11359.go",
    "tests/files/go/test/fixedbugs/issue11610.go",
    "tests/files/go/test/fixedbugs/issue15611.go",
    "tests/files/go/test/fixedbugs/issue23587.go",
    "tests/files/go/test/fixedbugs/issue30722.go",
    "tests/files/go/test/fixedbugs/issue32133.go",
    "tests/files/go/test/fixedbugs/issue4405.go",
    "tests/files/go/test/fixedbugs/issue9036.go",
    "tests/files/go/test/slice3err.go",
    // Intentionally invalid syntax (channel without element type, ellipsis misuse, send as value, composite)
    "tests/files/go/test/syntax/chan.go",
    "tests/files/go/test/syntax/chan1.go",
    "tests/files/go/test/syntax/composite.go",
    "tests/files/go/test/syntax/ddd.go",
    "tests/files/go/test/ddd1.go",
    // Files using //line directives that affect position reporting (DWARF/debugging features)
    "tests/files/go/test/dwarf/linedirectives.go",
    "tests/files/go/test/dwarf/dwarf.dir/z1.go",
    "tests/files/go/test/dwarf/dwarf.dir/z2.go",
    "tests/files/go/test/dwarf/dwarf.dir/z3.go",
    "tests/files/go/test/dwarf/dwarf.dir/z4.go",
    "tests/files/go/test/dwarf/dwarf.dir/z5.go",
    "tests/files/go/test/dwarf/dwarf.dir/z6.go",
    "tests/files/go/test/dwarf/dwarf.dir/z7.go",
    "tests/files/go/test/dwarf/dwarf.dir/z8.go",
    "tests/files/go/test/dwarf/dwarf.dir/z9.go",
    "tests/files/go/test/dwarf/dwarf.dir/z10.go",
    "tests/files/go/test/dwarf/dwarf.dir/z11.go",
    "tests/files/go/test/dwarf/dwarf.dir/z12.go",
    "tests/files/go/test/dwarf/dwarf.dir/z13.go",
    "tests/files/go/test/dwarf/dwarf.dir/z14.go",
    "tests/files/go/test/dwarf/dwarf.dir/z15.go",
    "tests/files/go/test/dwarf/dwarf.dir/z16.go",
    "tests/files/go/test/dwarf/dwarf.dir/z17.go",
    "tests/files/go/test/dwarf/dwarf.dir/z18.go",
    "tests/files/go/test/dwarf/dwarf.dir/z19.go",
    "tests/files/go/test/dwarf/dwarf.dir/z20.go",
    // Intentionally missing file (referenced in Go parser testdata)
    "tests/files/go/src/go/parser/testdata/issue42951/not_a_file.go",
    // Invalid Go file in a path that looks like a file (Go parser testdata)
    "tests/files/go/src/go/parser/testdata/issue42951/not_a_file.go/invalid.go",
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
