use crossbeam::thread;
use glob::glob;
use phf::{phf_set, Set};
use std::env;
use std::path::Path;
use std::process::{Command, Output};

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
        if env::var("DEV").unwrap_or(String::from("dev")) == "true" {
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
                8 * num_cpus::get(),
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

    thread::scope(|scope| {
        go_files.chunks(chunk_size).for_each(|go_files| {
            scope.spawn(move |_| {
                for go_file in go_files {
                    if is_dev {
                        println!("> {}", go_file);
                    }

                    let go_output = exec(go_bin, &[command, go_file]).unwrap();
                    let rust_output = exec(rust_bin, &[command, go_file]).unwrap();

                    if go_output.stdout != rust_output.stdout {
                        if is_dev {
                            // git diff
                        } else {
                            panic!("Rust/Go outputs diff on: {:?}", go_file)
                        }
                    }
                }
            });
        })
    })
    .unwrap();
}

fn exec(bin: &str, args: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
    let output = Command::new(bin).args(args).output()?;

    if !output.status.success() {
        return Err(format!("{} {:?} exited with status {:?}", bin, args, output.status).into());
    }

    Ok(output)
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
