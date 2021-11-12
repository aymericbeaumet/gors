use crossbeam::thread;
use std::env;
use std::path::Path;
use std::process::{Command, Output};

use glob::glob;

#[test]
fn test_lexer() {
    let root = env::var("CARGO_MANIFEST_DIR").unwrap();
    env::set_current_dir(Path::new(&root)).unwrap();

    println!("Updating git submodules...");
    exec("git", &["submodule", "update", "--init"]).unwrap();

    println!("Building the Rust binary...");
    exec("cargo", &["build", "--release"]).unwrap();

    println!("Finding go files...");
    let go_files: Vec<_> = glob(".repositories/**/*.go")
        .unwrap()
        .map(|entry| entry.unwrap().to_str().unwrap().to_owned())
        .collect();

    let parallelism = 8 * num_cpus::get();
    let total = go_files.len();
    let chunk_size = (total / parallelism) + 1;
    println!(
        "Testing on {} go files in chunk of {} (parallelism={})",
        total, chunk_size, parallelism,
    );

    thread::scope(|scope| {
        go_files.chunks(chunk_size).for_each(|go_files| {
            scope.spawn(move |_| {
                for go_file in go_files {
                    match exec("./tests/go-cli/go-cli", &["tokens", go_file]) {
                        Ok(go_output) => {
                            let rust_output =
                                exec("./target/release/gors", &["tokens", go_file]).unwrap();
                            if go_output.stdout != rust_output.stdout {
                                panic!("Rust/Go outputs diff on: {:?}", go_file)
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
