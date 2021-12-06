use colored::*;
use console::{style, Style};
use crossbeam::thread;
use glob::glob;
use phf::{phf_set, Set};
use similar::{ChangeTag, TextDiff};
use std::env;
use std::fmt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref RUNNER: TestRunner<'static> = TestRunner::new();
}

#[test]
fn test_lexer() {
    RUNNER.test("tokens");
}

#[test]
fn test_parser() {
    RUNNER.test("ast");
}

#[derive(Debug)]
struct TestRunner<'a> {
    go_bin: &'a str,
    go_files: Vec<String>,
    print_files: bool,
    rust_bin: &'a str,
    thread_count: usize,
}

impl TestRunner<'_> {
    fn new() -> Self {
        println!("\n# initializing test runner...");

        let go_bin = "tests/go-cli/go-cli";
        let input_patterns: &[&str] = match std::option_env!("LOCAL_FILES_ONLY") {
            Some("true") => &["tests/files/**/*.go"],
            _ => &["tests/files/**/*.go", ".repositories/**/*.go"],
        };
        let print_files = match std::option_env!("PRINT_FILES") {
            Some("true") => true,
            _ => false,
        };
        let rust_bin = match std::option_env!("RELEASE_BUILD") {
            Some("false") => "target/debug/gors",
            _ => "target/release/gors",
        };
        let rust_build_flags: &[&str] = match std::option_env!("RELEASE_BUILD") {
            Some("false") => &["build"],
            _ => &["build", "--release"],
        };
        let thread_count = match std::option_env!("LOCAL_FILES_ONLY") {
            Some("true") => 1,
            _ => num_cpus::get(),
        };

        let root = env::var("CARGO_MANIFEST_DIR").unwrap();
        env::set_current_dir(Path::new(&root)).unwrap();

        println!("# updating git submodules...");
        exec("git", &["submodule", "update", "--init"]).unwrap();

        println!("# building the Rust binary...");
        exec("cargo", rust_build_flags).unwrap();

        println!("# finding go files...");
        let go_files: Vec<_> = input_patterns
            .iter()
            .flat_map(|pattern| {
                glob(pattern).unwrap().filter_map(|entry| {
                    let path = entry.unwrap();
                    let path = path.to_str().unwrap();
                    if IGNORE_FILES.contains(path) {
                        None
                    } else {
                        Some(path.to_owned())
                    }
                })
            })
            .collect();
        let total = go_files.len();
        println!("# found {} go files", total);

        print!("# test runner initialized");

        Self {
            go_bin,
            go_files,
            print_files,
            rust_bin,
            thread_count,
        }
    }

    fn test(&self, command: &str) {
        let (go_elapsed, rust_elapsed) = thread::scope(|scope| {
            let handles: Vec<_> = self
                .go_files
                .chunks((self.go_files.len() / self.thread_count) + 1)
                .enumerate()
                .map(|(i, chunk)| {
                    println!("\n| starting thread #{} (chunk_len={})", i, chunk.len());
                    scope.spawn(|_| {
                        chunk.iter().fold(
                            (Duration::new(0, 0), Duration::new(0, 0)),
                            |acc, go_file| {
                                if self.print_files {
                                    println!("> {}", go_file);
                                }

                                let args = &[command, go_file];
                                let (go_output, go_elapsed) = exec(self.go_bin, args).unwrap();
                                let (rust_output, rust_elapsed) =
                                    exec(self.rust_bin, args).unwrap();

                                if go_output.stdout != rust_output.stdout {
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
        eprintln!("STATUS: {}", output.status.code().unwrap());
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

// Some files cannot successfully be parsed by the Go compiler. So we exclude them from the
// testing/benchmarking for now.
static IGNORE_FILES: Set<&'static str> = phf_set! {
    ".repositories/github.com/golang/go/src/cmd/api/testdata/src/pkg/p4/p4.go",
    ".repositories/github.com/golang/go/src/constraints/constraints.go",
    ".repositories/github.com/golang/go/src/go/doc/testdata/generics.go",
    ".repositories/github.com/golang/go/src/go/parser/testdata/issue42951/not_a_file.go",
    ".repositories/github.com/golang/go/test/bombad.go",
    ".repositories/github.com/golang/go/test/char_lit1.go",
    ".repositories/github.com/golang/go/test/fixedbugs/bug014.go",
    ".repositories/github.com/golang/go/test/fixedbugs/bug068.go",
    ".repositories/github.com/golang/go/test/fixedbugs/bug163.go",
    ".repositories/github.com/golang/go/test/fixedbugs/bug169.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue11359.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue11610.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue15611.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue23587.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue30722.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue32133.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue4405.go",
    ".repositories/github.com/golang/go/test/fixedbugs/issue9036.go",
    ".repositories/github.com/golang/go/test/typeparam/absdiff.go",
    ".repositories/github.com/golang/go/test/typeparam/absdiffimp.dir/a.go",
    ".repositories/github.com/golang/go/test/typeparam/append.go",
    ".repositories/github.com/golang/go/test/typeparam/boundmethod.go",
    ".repositories/github.com/golang/go/test/typeparam/builtins.go",
    ".repositories/github.com/golang/go/test/typeparam/double.go",
    ".repositories/github.com/golang/go/test/typeparam/fact.go",
    ".repositories/github.com/golang/go/test/typeparam/issue39755.go",
    ".repositories/github.com/golang/go/test/typeparam/issue48137.go",
    ".repositories/github.com/golang/go/test/typeparam/issue48424.go",
    ".repositories/github.com/golang/go/test/typeparam/issue48453.go",
    ".repositories/github.com/golang/go/test/typeparam/issue48538.go",
    ".repositories/github.com/golang/go/test/typeparam/issue48609.go",
    ".repositories/github.com/golang/go/test/typeparam/issue48711.go",
    ".repositories/github.com/golang/go/test/typeparam/issue49295.go",
    ".repositories/github.com/golang/go/test/typeparam/list.go",
    ".repositories/github.com/golang/go/test/typeparam/listimp.dir/a.go",
    ".repositories/github.com/golang/go/test/typeparam/min.go",
    ".repositories/github.com/golang/go/test/typeparam/minimp.dir/a.go",
    ".repositories/github.com/golang/go/test/typeparam/nested.go",
    ".repositories/github.com/golang/go/test/typeparam/ordered.go",
    ".repositories/github.com/golang/go/test/typeparam/orderedmap.go",
    ".repositories/github.com/golang/go/test/typeparam/orderedmapsimp.dir/a.go",
    ".repositories/github.com/golang/go/test/typeparam/settable.go",
    ".repositories/github.com/golang/go/test/typeparam/sliceimp.dir/a.go",
    ".repositories/github.com/golang/go/test/typeparam/sliceimp.dir/main.go",
    ".repositories/github.com/golang/go/test/typeparam/slices.go",
    ".repositories/github.com/golang/go/test/typeparam/smallest.go",
    ".repositories/github.com/golang/go/test/typeparam/typelist.go",
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

fn print_diff(old: &str, new: &str) {
    let diff = TextDiff::from_lines(old, new);

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
