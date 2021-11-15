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

#[test]
fn test_lexer() {
    run("tokens");
}

#[test]
fn test_parser() {
    run("ast");
}

fn run(command: &str) {
    let (is_dev, go_patterns, rust_build_flags, rust_bin, go_bin, thread_count) =
        if std::option_env!("DEV").unwrap_or("false") == "true" {
            (
                true,
                vec!["tests/files/**/*.go"],
                vec!["build"],
                "target/debug/gors",
                "tests/go-cli/go-cli",
                1,
            )
        } else {
            (
                false,
                vec!["tests/files/**/*.go", ".repositories/**/*.go"],
                vec!["build", "--release"],
                "target/release/gors",
                "tests/go-cli/go-cli",
                2 * num_cpus::get(),
            )
        };

    println!("| dev mode? {}", is_dev);

    let root = env::var("CARGO_MANIFEST_DIR").unwrap();
    env::set_current_dir(Path::new(&root)).unwrap();

    println!("| updating git submodules...");
    exec("git", &["submodule", "update", "--init"]).unwrap();

    println!("| building the Rust binary... ({:?})", rust_build_flags);
    exec("cargo", &rust_build_flags).unwrap();

    println!("| finding go files...");
    let go_files: Vec<_> = go_patterns
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
    let chunk_size = (total / thread_count) + 1;
    println!(
        "| starting {} thread(s) to test {} go files in chunks of {}",
        thread_count, total, chunk_size,
    );

    let (go_elapsed, rust_elapsed) = thread::scope(|scope| {
        go_files
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(|_| {
                    chunk.iter().map(|go_file| {
                        if is_dev {
                            println!("> {}", go_file);
                        }

                        let (go_output, go_elapsed) = exec(go_bin, &[command, go_file]).unwrap();
                        let (rust_output, rust_elapsed) =
                            exec(rust_bin, &[command, go_file]).unwrap();

                        if go_output.stdout != rust_output.stdout {
                            print_diff(
                                std::str::from_utf8(&go_output.stdout).unwrap(),
                                std::str::from_utf8(&rust_output.stdout).unwrap(),
                            );
                            std::process::exit(1);
                        }

                        (go_elapsed, rust_elapsed)
                    })
                })
            })
            .flat_map(|handle| handle.join().unwrap())
            .fold((Duration::new(0, 0), Duration::new(0, 0)), |acc, (g, r)| {
                (acc.0 + g, acc.1 + r)
            })
    })
    .unwrap();

    println!("");
    println!("Total Elapsed Time:");
    println!("- Go: {:?}", go_elapsed);
    println!(
        "- Rust: {:?} ({:+.2}%)",
        rust_elapsed,
        ((rust_elapsed.as_secs_f64() / go_elapsed.as_secs_f64()) - 1.0) * 100.0
    );
    println!("");
}

fn exec(bin: &str, args: &[&str]) -> Result<(Output, Duration), Box<dyn std::error::Error>> {
    let before = std::time::Instant::now();
    let output = Command::new(bin).args(args).output()?;
    let after = std::time::Instant::now();

    if !output.status.success() {
        return Err(format!("{} {:?} exited with status {:?}", bin, args, output.status).into());
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
