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

        let (gors_bin, pattern, thread_count) = match std::option_env!("CI") {
            Some("true") => {
                println!("\n| building release gors binary...");
                Command::new("cargo")
                    .args(["build", "--release"])
                    .current_dir(&go_dir)
                    .spawn()
                    .unwrap()
                    .wait()
                    .unwrap();

                println!("| initializing submodules...");
                Command::new("git")
                    .args(["submodule", "update", "--init", "--depth=1"])
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                    .spawn()
                    .unwrap()
                    .wait()
                    .unwrap();

                (
                    concat!(env!("CARGO_MANIFEST_DIR"), "/../target/release/gors"),
                    "**/*.go",
                    num_cpus::get(),
                )
            }
            _ => (
                concat!(env!("CARGO_MANIFEST_DIR"), "/../target/debug/gors"),
                "*.go",
                1,
            ),
        };

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
        println!("\n| found {} go files", go_files.len());

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
                                let args = &[command, go_file];
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

// Some files cannot successfully be parsed by the Go compiler. So we exclude them from the
// testing/benchmarking for now.
static IGNORE_FILES: Set<&'static str> = phf_set! {
    "tests/files/go/src/cmd/api/testdata/src/pkg/p4/p4.go",
    "tests/files/go/src/constraints/constraints.go",
    "tests/files/go/src/go/doc/testdata/generics.go",
    "tests/files/go/src/go/parser/testdata/issue42951/not_a_file.go",
    "tests/files/go/test/bombad.go",
    "tests/files/go/test/char_lit1.go",
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
    "tests/files/go/test/typeparam/absdiff.go",
    "tests/files/go/test/typeparam/absdiffimp.dir/a.go",
    "tests/files/go/test/typeparam/append.go",
    "tests/files/go/test/typeparam/boundmethod.go",
    "tests/files/go/test/typeparam/builtins.go",
    "tests/files/go/test/typeparam/double.go",
    "tests/files/go/test/typeparam/fact.go",
    "tests/files/go/test/typeparam/issue39755.go",
    "tests/files/go/test/typeparam/issue48137.go",
    "tests/files/go/test/typeparam/issue48424.go",
    "tests/files/go/test/typeparam/issue48453.go",
    "tests/files/go/test/typeparam/issue48538.go",
    "tests/files/go/test/typeparam/issue48609.go",
    "tests/files/go/test/typeparam/issue48711.go",
    "tests/files/go/test/typeparam/issue49295.go",
    "tests/files/go/test/typeparam/list.go",
    "tests/files/go/test/typeparam/listimp.dir/a.go",
    "tests/files/go/test/typeparam/min.go",
    "tests/files/go/test/typeparam/minimp.dir/a.go",
    "tests/files/go/test/typeparam/nested.go",
    "tests/files/go/test/typeparam/ordered.go",
    "tests/files/go/test/typeparam/orderedmap.go",
    "tests/files/go/test/typeparam/orderedmapsimp.dir/a.go",
    "tests/files/go/test/typeparam/settable.go",
    "tests/files/go/test/typeparam/sliceimp.dir/a.go",
    "tests/files/go/test/typeparam/sliceimp.dir/main.go",
    "tests/files/go/test/typeparam/slices.go",
    "tests/files/go/test/typeparam/smallest.go",
    "tests/files/go/test/typeparam/typelist.go",
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
