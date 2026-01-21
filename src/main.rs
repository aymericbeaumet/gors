// Clippy lints are configured at workspace level in the root Cargo.toml

use clap::Parser;
use gors::error::{Diagnostic, DiagnosticKind};
use std::io::Write;

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
    /// Compile and run the named Go file (uses WASM runtime)
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
    /// Type of output: rust (source), wasm (WebAssembly binary)
    #[arg(long)]
    emit: Option<String>,
    /// Output path for source map (.map file in standard v3 format)
    #[arg(long)]
    sourcemap: Option<String>,
    /// Target: wasm (default) or rust
    #[arg(long, default_value = "wasm")]
    target: String,
    /// Output file path
    #[arg(short, long)]
    output: Option<String>,
}

#[derive(Parser)]
struct Run {
    /// The file to run
    file: String,
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

    // Use source map tracking if --sourcemap is specified
    let compiled = if cmd.sourcemap.is_some() {
        match gors::compiler::compile_with_source_map(ast, &cmd.file, &buffer) {
            Ok(compiled) => compiled,
            Err(err) => {
                let diagnostic =
                    Diagnostic::new(&cmd.file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    } else {
        match gors::compiler::compile(ast) {
            Ok(compiled) => compiled,
            Err(err) => {
                let diagnostic =
                    Diagnostic::new(&cmd.file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    };

    // Write source map if requested
    if let Some(ref map_path) = cmd.sourcemap {
        let rust_source = gors::backend_rust::generate(compiled.clone())?;
        let source_map = gors::compiler::build_source_map(&rust_source);
        let mut map_file = std::fs::File::create(map_path)?;
        source_map.to_writer(&mut map_file)?;
    }

    // Emit Rust source code only
    if matches!(cmd.emit.as_deref(), Some("rust")) || cmd.target == "rust" {
        let rust_source = gors::backend_rust::generate(compiled)?;
        let output_path = cmd.output.as_deref().unwrap_or("main.rs");
        let mut w = std::fs::File::create(output_path)?;
        w.write_all(rust_source.as_bytes())?;
        println!("Wrote {output_path}");
        return Ok(());
    }

    // Default: WASM target
    let wasm_bytes = gors::backend_wasm::compile_to_wasm(&compiled).map_err(|e| {
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            as Box<dyn std::error::Error>
    })?;
    let output_path = cmd.output.as_deref().unwrap_or("output.wasm");
    std::fs::write(output_path, wasm_bytes)?;
    println!("Wrote {output_path}");

    Ok(())
}

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
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

    // Compile to WASM
    let wasm_bytes = gors::backend_wasm::compile_to_wasm(&compiled).map_err(|e| {
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            as Box<dyn std::error::Error>
    })?;

    // Run with Wasmer
    run_wasm(&wasm_bytes)
}

/// Run WASM bytes using the Wasmer runtime
fn run_wasm(wasm_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use wasmer::{imports, Instance, Module, Store, Function, FunctionEnv, FunctionEnvMut};

    // Create a store
    let mut store = Store::default();

    // Compile the WASM module
    let module = Module::new(&store, wasm_bytes)?;

    // Create environment for imports
    struct Env;
    let env = FunctionEnv::new(&mut store, Env);

    // Create print_i32 import function
    fn print_i32(_env: FunctionEnvMut<'_, Env>, value: i32) {
        println!("{value}");
    }

    // Create imports
    let import_object = imports! {
        "env" => {
            "print_i32" => Function::new_typed_with_env(&mut store, &env, print_i32),
        }
    };

    // Instantiate the module
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Get and call the main function
    let main_func = instance.exports.get_function("main")?;
    main_func.call(&mut store, &[])?;

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

#[allow(dead_code)]
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
