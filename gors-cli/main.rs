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
    /// Compile the named Go file
    Build(Build),
    /// Compile and run the named Go file
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
    /// The file to build
    file: String,
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
    /// Type of output for the compiler to emit: rust|asm|llvm-bc|llvm-ir|obj|metadata|link|dep-info|mir
    #[arg(long)]
    emit: Option<String>,
}

#[derive(Parser)]
struct Run {
    /// The file to run
    file: String,
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
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
    let buffer = std::fs::read_to_string(&cmd.file)?;

    let ast = match gors::parser::parse_file(&cmd.file, &buffer) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, &cmd.file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    let compiled = match gors::compiler::compile(ast) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(&cmd.file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    // shortcut when rust code is to be emitted
    if matches!(cmd.emit.as_deref(), Some("rust")) {
        let mut w = std::fs::File::create("main.rs")?;
        gors::codegen::fprint(&mut w, compiled)?;
        return Ok(());
    }

    let tmp_dir = tempfile::tempdir()?;
    let source_file = tmp_dir.path().join("main.rs");
    let mut w = std::fs::File::create(&source_file)?;
    gors::codegen::fprint(&mut w, compiled)?;
    w.sync_all()?;

    let src_path = source_file
        .to_str()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
    let rustc = Command::new("rustc")
        .args(RustcArgs {
            src: src_path,
            out: None,
            emit: cmd.emit.as_deref(),
            release: cmd.release,
        })
        .output()?;
    if !rustc.status.success() {
        print!("{}", String::from_utf8_lossy(&rustc.stdout));
        eprint!("{}", String::from_utf8_lossy(&rustc.stderr));
        return Ok(());
    }

    Ok(())
}

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = tempfile::tempdir()?;
    let out_rust = tmp_dir.path().join("main.rs");
    let out_bin = tmp_dir.path().join("main");
    let mut w = std::fs::File::create(&out_rust)?;

    let buffer = std::fs::read_to_string(&cmd.file)?;

    let ast = match gors::parser::parse_file(&cmd.file, &buffer) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, &cmd.file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    let compiled = match gors::compiler::compile(ast) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(&cmd.file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    gors::codegen::fprint(&mut w, compiled)?;
    w.sync_all()?;

    let src_path = out_rust
        .to_str()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
    let out_path = out_bin
        .to_str()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
    let rustc = Command::new("rustc")
        .args(RustcArgs {
            src: src_path,
            out: Some(out_path),
            emit: None,
            release: cmd.release,
        })
        .output()?;
    if !rustc.status.success() {
        print!("{}", String::from_utf8_lossy(&rustc.stdout));
        eprint!("{}", String::from_utf8_lossy(&rustc.stderr));
        return Ok(());
    }

    let mut cmd = Command::new(&out_bin).spawn()?;
    cmd.wait()?;

    Ok(())
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
