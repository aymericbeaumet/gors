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
    /// Compile the named Go source file or directory
    Build(Build),
    /// Compile and run the named Go source file or directory (uses WASM runtime)
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
    /// The Go source file or directory to run
    path: String,
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
    let (ast, files) = match gors::parser::parse_path(&cmd.path) {
        Ok(result) => result,
        Err(gors::parser::PathParseError::ParserError(err)) => {
            // For parser errors, try to get the file and buffer for diagnostics
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

    // Get the first file's info for error reporting and source maps
    let (primary_file, primary_buffer) = files.first()
        .map(|(f, b)| (f.as_str(), b.as_str()))
        .unwrap_or((&cmd.path, ""));

    // Use source map tracking if --sourcemap is specified
    let compiled = if cmd.sourcemap.is_some() {
        match gors::compiler::compile_with_source_map(ast, primary_file, primary_buffer) {
            Ok(compiled) => compiled,
            Err(err) => {
                let diagnostic =
                    Diagnostic::new(primary_file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    } else {
        match gors::compiler::compile(ast) {
            Ok(compiled) => compiled,
            Err(err) => {
                let diagnostic =
                    Diagnostic::new(primary_file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
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
    let (ast, files) = match gors::parser::parse_path(&cmd.path) {
        Ok(result) => result,
        Err(gors::parser::PathParseError::ParserError(err)) => {
            // For parser errors, try to get the file and buffer for diagnostics
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

    // Get the first file's info for error reporting
    let primary_file = files.first()
        .map(|(f, _)| f.as_str())
        .unwrap_or(&cmd.path);

    let compiled = match gors::compiler::compile(ast) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(primary_file, 0, 0, err.to_string(), DiagnosticKind::Compiler);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };

    // Compile to WASM
    let wasm_bytes = gors::backend_wasm::compile_to_wasm(&compiled).map_err(|e| {
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            as Box<dyn std::error::Error>
    })?;

    // Run with wasmi
    run_wasm(&wasm_bytes)
}

/// Helper to get file path and contents for error reporting.
/// If path is a directory, returns the first .go file in it.
fn get_file_for_error(path: &str) -> Option<(String, String)> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_file() {
        let buffer = std::fs::read_to_string(path).ok()?;
        Some((path.to_string(), buffer))
    } else if metadata.is_dir() {
        // Find first .go file
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

/// State for WASM runtime
struct WasmRunState {
    memory: Option<wasmi::Memory>,
}

/// Run WASM bytes using the wasmi runtime.
/// This works both natively and when gors is compiled to WASM.
fn run_wasm(wasm_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use wasmi::{Caller, Engine, Func, Linker, Module, Store};

    // Create engine and store
    let engine = Engine::default();
    let state = WasmRunState { memory: None };
    let mut store = Store::new(&engine, state);

    // Compile the WASM module
    let module = Module::new(&engine, wasm_bytes)?;

    // Create linker and add imports
    let mut linker = Linker::new(&engine);

    // print_i32 function that prints an i32 value
    linker.func_wrap("env", "print_i32", |_caller: Caller<'_, WasmRunState>, value: i32| {
        println!("{value}");
    })?;

    // print_str function that reads string from memory and prints it
    linker.func_wrap("env", "print_str", |caller: Caller<'_, WasmRunState>, offset: i32, len: i32| {
        if let Some(memory) = &caller.data().memory {
            let mut buffer = vec![0u8; len as usize];
            if memory.read(&caller, offset as usize, &mut buffer).is_ok() {
                if let Ok(s) = String::from_utf8(buffer) {
                    println!("{s}");
                }
            }
        }
    })?;

    // Instantiate the module
    let instance = linker.instantiate(&mut store, &module)?.start(&mut store)?;

    // Get memory export and store it for print_str to use
    if let Some(memory) = instance.get_export(&store, "memory").and_then(|e| e.into_memory()) {
        store.data_mut().memory = Some(memory);
    }

    // Get and call the main function
    let main_func: Func = instance
        .get_export(&store, "main")
        .and_then(|e| e.into_func())
        .ok_or("main function not found")?;

    main_func.call(&mut store, &[], &mut [])?;

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
