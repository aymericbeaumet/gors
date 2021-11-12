use crossbeam::thread;
use glob::glob;
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
    let (is_dev, rust_build_flags, go_pattern, go_bin, rust_bin, thread_count) =
        if env::var("DEV").unwrap_or(String::from("dev")) == "true" {
            (
                true,
                vec!["build"],
                "tests/files/**/*.go",
                "tests/go-cli/go-cli",
                "target/debug/gors",
                1,
            )
        } else {
            (
                false,
                vec!["build", "--release"],
                ".repositories/**/*.go",
                "tests/go-cli/go-cli",
                "target/release/gors",
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
    let go_files: Vec<_> = glob(go_pattern)
        .unwrap()
        .map(|entry| entry.unwrap().to_str().unwrap().to_owned())
        .collect();

    let total = go_files.len();
    let chunk_size = (total / thread_count) + 1;
    println!(
        "| starting {} thread(s) to test on {} go files in chunks of {}",
        thread_count, total, chunk_size,
    );

    thread::scope(|scope| {
        go_files.chunks(chunk_size).for_each(|go_files| {
            scope.spawn(move |_| {
                for go_file in go_files {
                    if is_dev {
                        println!("> {}", go_file);
                    }

                    match exec(go_bin, &[command, go_file]) {
                        Ok(go_output) => {
                            let rust_output = exec(rust_bin, &[command, go_file]).unwrap();
                            if go_output.stdout != rust_output.stdout {
                                if is_dev {
                                    // git diff
                                } else {
                                    panic!("Rust/Go outputs diff on: {:?}", go_file)
                                }
                            }
                        }
                        Err(err) => println!("Skipping file: {}, because {:?}", go_file, err),
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
