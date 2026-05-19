// Clippy lints are configured at workspace level in the root Cargo.toml

use clap::Parser;
use gors::error::{Diagnostic, DiagnosticKind};
use std::io::Write;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::Run(cmd) => run(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
    }
}

/// Print a formatted error with source context
fn print_error(diagnostic: &Diagnostic) {
    // Check if stdout supports colors
    let use_colors = atty::is(atty::Stream::Stderr);
    eprint!("{}", diagnostic.format_terminal(use_colors));
}

#[derive(Parser)]
#[command(
    version = "1.0",
    name = "gors",
    author = "Aymeric Beaumet <hi@aymericbeaumet.com>",
    about = "gors is a go toolbelt written in rust; providing a parser and rust transpiler"
)]
struct Opts {
    #[command(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    /// Parse the named Go file and print the AST
    Ast(Ast),
    /// Compile the named Go source file or directory to Rust
    Build(Build),
    /// Compile and run the named Go source file or directory
    Run(Run),
    /// Scan the named Go file and print the tokens
    Tokens(Tokens),
}

#[derive(Parser)]
struct Ast {
    /// The file to parse
    file: String,
}

#[derive(Parser)]
struct Build {
    /// The Go source file or directory to build
    path: String,
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
    /// Output path for source map (.map file in standard v3 format)
    #[arg(long)]
    sourcemap: Option<String>,
    /// Output file path
    #[arg(short, long)]
    output: Option<String>,
}

#[derive(Parser)]
struct Run {
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
    /// Go source file(s), directory, or package path, followed by optional program arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    args: Vec<String>,
}

#[derive(Parser)]
struct Tokens {
    /// The file to lex
    file: String,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let buffer = std::fs::read_to_string(&cmd.file)?;
    let ast = match gors::parser::parse_file(&cmd.file, &buffer) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, &cmd.file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };
    gors::ast::fprint(&mut w, ast)?;
    w.flush()?;

    Ok(())
}

fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let _toolchain = gors::toolchain::ensure()?;

    let program = match gors::parser::parse_program(&cmd.path) {
        Ok(result) => result,
        Err(gors::parser::PathParseError::ParserError(err)) => {
            let (file, buffer) = if let Some((f, b)) = get_file_for_error(&cmd.path) {
                (f, b)
            } else {
                (cmd.path.clone(), String::new())
            };
            let diagnostic = Diagnostic::from_parser_error(&err, &file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("error: {}", err);
            std::process::exit(1);
        }
    };

    let primary_file = program
        .main_package
        .files
        .first()
        .map(|(f, _)| f.clone())
        .unwrap_or_else(|| cmd.path.clone());

    let compiled = match gors::compiler::compile_program_multi(program) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(
                &primary_file,
                0,
                0,
                err.to_string(),
                DiagnosticKind::Compiler,
            );
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    let output = gors::backend_rust::generate_multi(compiled)?;
    let output_dir = cmd.output.as_deref().unwrap_or("gors_output");
    std::fs::create_dir_all(output_dir)?;

    let prev_manifest =
        gors::compiler::manifest::BuildManifest::load(std::path::Path::new(output_dir));

    let mut new_manifest = gors::compiler::manifest::BuildManifest::new();
    let mut written = 0;
    let mut skipped = 0;

    for (filename, source) in &output.files {
        let file_path = std::path::Path::new(output_dir).join(filename);
        let current_hash = sha2_hash(source);

        let unchanged = prev_manifest
            .as_ref()
            .and_then(|m| m.modules.get(filename))
            .is_some_and(|entry| entry.content_hash == current_hash);

        if unchanged && file_path.exists() {
            skipped += 1;
        } else {
            std::fs::write(&file_path, source)?;
            written += 1;
        }

        new_manifest.modules.insert(
            filename.clone(),
            gors::compiler::manifest::ModuleEntry {
                content_hash: current_hash,
                output_file: filename.clone(),
            },
        );
    }

    new_manifest.save(std::path::Path::new(output_dir))?;
    println!("Wrote {written} files to {output_dir} ({skipped} unchanged)");

    Ok(())
}

fn sha2_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Split CLI arguments into source paths and program arguments.
///
/// If the first argument ends with `.go`, all leading `.go` arguments are source
/// files. Otherwise, the first argument is a directory/package path. Everything
/// after the source paths is passed through to the compiled program.
fn split_run_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    if args.first().is_some_and(|a| a.ends_with(".go")) {
        let split = args
            .iter()
            .position(|a| !a.ends_with(".go"))
            .unwrap_or(args.len());
        (args[..split].to_vec(), args[split..].to_vec())
    } else {
        (vec![args[0].clone()], args[1..].to_vec())
    }
}

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let _toolchain = gors::toolchain::ensure()?;

    let (source_paths, program_args) = split_run_args(&cmd.args);

    let program = match gors::parser::parse_program_files(&source_paths) {
        Ok(result) => result,
        Err(gors::parser::PathParseError::ParserError(err)) => {
            let (file, buffer) = if let Some((f, b)) = get_file_for_error(&source_paths[0]) {
                (f, b)
            } else {
                (source_paths[0].clone(), String::new())
            };
            let diagnostic = Diagnostic::from_parser_error(&err, &file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("error: {}", err);
            std::process::exit(1);
        }
    };

    let primary_file = program
        .main_package
        .files
        .first()
        .map(|(f, _)| f.clone())
        .unwrap_or_else(|| source_paths[0].clone());

    let compiled = match gors::compiler::compile_program_multi(program) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(
                &primary_file,
                0,
                0,
                err.to_string(),
                DiagnosticKind::Compiler,
            );
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    let output = gors::backend_rust::generate_multi(compiled)?;

    let tmp_dir = tempfile::tempdir()?;
    for (filename, source) in &output.files {
        std::fs::write(tmp_dir.path().join(filename), source)?;
    }

    let src_path = tmp_dir.path().join("main.rs");
    let bin_path = tmp_dir.path().join("main");

    let rustc_args = RustcArgs {
        src: src_path.to_str().unwrap(),
        out: Some(bin_path.to_str().unwrap()),
        emit: None,
        release: cmd.release,
    };

    let rustc_status = Command::new("rustc")
        .args(Vec::from(rustc_args))
        .status()?;

    if !rustc_status.success() {
        std::process::exit(rustc_status.code().unwrap_or(1));
    }

    let status = Command::new(&bin_path).args(&program_args).status()?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Helper to get file path and contents for error reporting.
/// If path is a directory, returns the first .go file in it.
fn get_file_for_error(path: &str) -> Option<(String, String)> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_file() {
        let buffer = std::fs::read_to_string(path).ok()?;
        Some((path.to_string(), buffer))
    } else if metadata.is_dir() {
        let entries = std::fs::read_dir(path).ok()?;
        for entry in entries.flatten() {
            let file_path = entry.path();
            if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".go") && !name.ends_with("_test.go") && !name.starts_with('.') {
                    let buffer = std::fs::read_to_string(&file_path).ok()?;
                    return Some((file_path.to_string_lossy().into_owned(), buffer));
                }
            }
        }
        None
    } else {
        None
    }
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let buffer = std::fs::read_to_string(&cmd.file)?;
    for step in gors::scanner::Scanner::new(&cmd.file, &buffer) {
        match step {
            Ok(s) => {
                serde_json::to_writer(&mut w, &s)?;
                w.write_all(b"\n")?;
            }
            Err(err) => {
                let diagnostic = Diagnostic::from_scanner_error(&err, &cmd.file, &buffer);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    }
    w.flush()?;

    Ok(())
}

struct RustcArgs<'a> {
    src: &'a str,
    out: Option<&'a str>,
    emit: Option<&'a str>,
    release: bool,
}

impl<'a> From<RustcArgs<'a>> for Vec<&'a str> {
    fn from(args: RustcArgs<'a>) -> Self {
        let mut flags = vec![args.src, "--edition=2021"];

        if let Some(emit) = args.emit {
            flags.extend(["--emit", emit]);
        }

        if let Some(out) = args.out {
            flags.extend(["-o", out]);
        }

        if args.release {
            flags.extend([
                "-Ccodegen-units=1",
                "-Clto=fat",
                "-Copt-level=3",
                "-Ctarget-cpu=native",
            ]);
        }

        flags
    }
}

impl<'a> IntoIterator for RustcArgs<'a> {
    type Item = &'a str;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        Vec::from(self).into_iter()
    }
}
