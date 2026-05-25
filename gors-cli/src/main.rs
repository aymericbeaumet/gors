// Clippy lints are configured at workspace level in the root Cargo.toml

use clap::{CommandFactory, Parser};
use gors::error::{Diagnostic, DiagnosticKind};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::Help(cmd) => help(cmd),
        SubCommand::Run(cmd) => run(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
        SubCommand::Version => version(),
    }
}

/// Print a formatted error with source context
fn print_error(diagnostic: &Diagnostic) {
    // Check if stdout supports colors
    let use_colors = atty::is(atty::Stream::Stderr);
    eprint!("{}", diagnostic.format_terminal(use_colors));
}

struct ProfileTimer {
    label: &'static str,
    start: Option<Instant>,
}

impl ProfileTimer {
    fn start(label: &'static str) -> Self {
        let enabled = std::env::var("GORS_PROFILE")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        Self {
            label,
            start: enabled.then(Instant::now),
        }
    }
}

impl Drop for ProfileTimer {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        eprintln!(
            "[gors-profile] {}: {:.2}ms",
            self.label,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

const ROOT_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
{usage-heading} {usage}

\x1b[1mCommands:\x1b[0m
{subcommands}

\x1b[1mAdvanced Commands:\x1b[0m
  \x1b[1mast\x1b[0m     Parse the named Go file and print the AST
  \x1b[1mtokens\x1b[0m  Scan the named Go file and print the tokens

\x1b[1mOptions:\x1b[0m
{options}{after-help}";

#[derive(Parser)]
#[command(
    name = "gors",
    author = "Aymeric Beaumet <hi@aymericbeaumet.com>",
    about = "gors is a go toolbelt written in rust; providing a parser and rust transpiler",
    disable_help_subcommand = true,
    disable_version_flag = true,
    help_template = ROOT_HELP_TEMPLATE
)]
struct Opts {
    #[command(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    /// Parse the named Go file and print the AST
    #[command(hide = true)]
    Ast(Ast),
    /// Transpile Go source to Rust files, writing to cache or --output
    #[command(display_order = 0)]
    Build(Build),
    /// Print this message or the help of the given command(s)
    #[command(display_order = 0)]
    Help(Help),
    /// Transpile, compile, and run Go source path(s)
    #[command(display_order = 0)]
    Run(Run),
    /// Scan the named Go file and print the tokens
    #[command(hide = true)]
    Tokens(Tokens),
    /// Print gors version
    #[command(display_order = 0)]
    Version,
}

#[derive(Parser)]
struct Ast {
    /// The files to parse
    #[arg(required = true)]
    files: Vec<String>,
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
struct Help {
    /// Print help for the command(s)
    #[arg(value_name = "COMMAND", num_args = 0.., trailing_var_arg = true)]
    commands: Vec<String>,
}

#[derive(Parser)]
struct Tokens {
    /// The files to lex
    #[arg(required = true)]
    files: Vec<String>,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    match cmd.files.as_slice() {
        [file] => ast_single(file, &mut w)?,
        files => {
            for file in files {
                write_file_result(&mut w, file, ast_output(file))?;
            }
        }
    }
    w.flush()?;

    Ok(())
}

fn ast_single(file: &str, w: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(file)?;
    let ast = match gors::parser::parse_file(file, &buffer) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };
    gors::ast::fprint(w, ast)?;
    Ok(())
}

fn ast_output(file: &str) -> Result<Vec<u8>, String> {
    let buffer = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let ast = gors::parser::parse_file(file, &buffer).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    gors::ast::fprint(&mut output, ast).map_err(|e| e.to_string())?;
    Ok(output)
}

fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let parse_timer = ProfileTimer::start("cli.parse");
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
    drop(parse_timer);

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

    let output = gors::printer::generate_multi(compiled)?;
    let output_dir = cmd
        .output
        .as_deref()
        .map(PathBuf::from)
        .map_or_else(|| build_cache_dir(&cmd.path), Ok)?;
    let stats = write_generated_output(&output, &output_dir)?;
    let output_dir = output_dir.display();
    if stats.removed == 0 {
        println!(
            "Wrote {} files to {output_dir} ({} unchanged)",
            stats.written, stats.skipped
        );
    } else {
        println!(
            "Wrote {} files to {output_dir} ({} unchanged, {} removed)",
            stats.written, stats.skipped, stats.removed
        );
    }

    Ok(())
}

fn build_cache_dir(source_path: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    if let Ok(cwd) = std::env::current_dir() {
        hasher.update(cwd.to_string_lossy().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(source_path.as_bytes());
    if let Ok(canonical) = std::fs::canonicalize(source_path) {
        hasher.update(b"\0");
        hasher.update(canonical.to_string_lossy().as_bytes());
    }

    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(gors_cache_base()?.join("build").join(key))
}

struct FileWriteStats {
    written: usize,
    skipped: usize,
    removed: usize,
}

fn write_generated_output(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<FileWriteStats, Box<dyn std::error::Error>> {
    let timer = ProfileTimer::start("cli.file_writes");
    std::fs::create_dir_all(output_dir)?;

    let prev_manifest = gors::compiler::manifest::BuildManifest::load(output_dir);
    let mut new_manifest = gors::compiler::manifest::BuildManifest::new();
    let mut stats = FileWriteStats {
        written: 0,
        skipped: 0,
        removed: 0,
    };

    for (filename, source) in &output.files {
        let file_path = output_dir.join(filename);
        let current_hash = sha2_hash(source);
        let unchanged = prev_manifest
            .as_ref()
            .is_some_and(|manifest| !manifest.needs_recompile(filename, &current_hash));

        if unchanged && file_path.exists() {
            stats.skipped += 1;
        } else {
            std::fs::write(&file_path, source)?;
            stats.written += 1;
        }

        new_manifest.modules.insert(
            filename.clone(),
            gors::compiler::manifest::ModuleEntry {
                content_hash: current_hash,
                output_file: filename.clone(),
            },
        );
    }

    if let Some(prev_manifest) = &prev_manifest {
        for (filename, entry) in &prev_manifest.modules {
            if output.files.contains_key(filename) {
                continue;
            }
            let file_path = output_dir.join(&entry.output_file);
            if file_path.is_file() {
                std::fs::remove_file(&file_path)?;
                stats.removed += 1;
            }
        }
    }

    new_manifest.save(output_dir)?;
    drop(timer);
    Ok(stats)
}

fn sha2_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn run_cache_dir(
    source_paths: &[String],
    release: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(if release {
        b"release".as_slice()
    } else {
        b"debug".as_slice()
    });
    hasher.update(b"\0");
    if let Ok(cwd) = std::env::current_dir() {
        hasher.update(cwd.to_string_lossy().as_bytes());
    }
    for path in source_paths {
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
        if let Ok(canonical) = std::fs::canonicalize(path) {
            hasher.update(b"\0");
            hasher.update(canonical.to_string_lossy().as_bytes());
        }
    }
    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(gors_cache_base()?.join("run").join(key))
}

fn gors_cache_base() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("gors"));
    }
    if let Some(path) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(path).join(".cache").join("gors"));
    }
    Ok(std::env::temp_dir().join("gors-cache"))
}

/// Split CLI arguments into source paths and program arguments.
///
/// If the first argument ends with `.go`, all leading `.go` arguments are source
/// files. Otherwise, the first argument is a directory/package path. Everything
/// after the source paths is passed through to the compiled program.
fn split_run_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    if args.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if args.first().is_some_and(|a| a.ends_with(".go")) {
        let split = args
            .iter()
            .position(|a| !a.ends_with(".go"))
            .unwrap_or(args.len());
        (
            args.get(..split).unwrap_or_default().to_vec(),
            args.get(split..).unwrap_or_default().to_vec(),
        )
    } else {
        (
            args.first().cloned().into_iter().collect(),
            args.get(1..).unwrap_or_default().to_vec(),
        )
    }
}

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let (source_paths, program_args) = split_run_args(&cmd.args);

    let parse_timer = ProfileTimer::start("cli.parse");
    let program = match gors::parser::parse_program_files(&source_paths) {
        Ok(result) => result,
        Err(gors::parser::PathParseError::ParserError(err)) => {
            let source_path = source_paths.first().cloned().unwrap_or_default();
            let (file, buffer) = if let Some((f, b)) = get_file_for_error(&source_path) {
                (f, b)
            } else {
                (source_path, String::new())
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
    drop(parse_timer);

    let primary_file = program
        .main_package
        .files
        .first()
        .map(|(f, _)| f.clone())
        .unwrap_or_else(|| source_paths.first().cloned().unwrap_or_default());

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

    let output = gors::printer::generate_multi(compiled)?;

    let cache_dir = run_cache_dir(&source_paths, cmd.release)?;
    write_generated_output(&output, &cache_dir)?;

    let src_path = cache_dir.join("main.rs");
    let bin_path = cache_dir.join("main");
    let incremental_path = cache_dir.join("rustc-incremental");
    std::fs::create_dir_all(&incremental_path)?;

    let src_str = src_path.to_string_lossy();
    let bin_str = bin_path.to_string_lossy();
    let incremental_str = incremental_path.to_string_lossy();
    let rustc_args = RustcArgs {
        src: &src_str,
        out: Some(&bin_str),
        emit: None,
        release: cmd.release,
        incremental: Some(&incremental_str),
    };

    let rustc_timer = ProfileTimer::start("cli.rustc");
    let rustc_status = Command::new("rustc").args(Vec::from(rustc_args)).status()?;
    drop(rustc_timer);

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

    match cmd.files.as_slice() {
        [file] => tokens_single(file, &mut w)?,
        files => {
            for file in files {
                write_file_result(&mut w, file, tokens_output(file))?;
            }
        }
    }
    w.flush()?;

    Ok(())
}

fn tokens_single(file: &str, w: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(file)?;
    for step in gors::scanner::Scanner::new(file, &buffer) {
        match step {
            Ok(s) => {
                serde_json::to_writer(&mut *w, &s)?;
                w.write_all(b"\n")?;
            }
            Err(err) => {
                let diagnostic = Diagnostic::from_scanner_error(&err, file, &buffer);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn tokens_output(file: &str) -> Result<Vec<u8>, String> {
    let buffer = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    for step in gors::scanner::Scanner::new(file, &buffer) {
        let token = step.map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut output, &token).map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    Ok(output)
}

fn write_file_result(
    w: &mut impl Write,
    path: &str,
    result: Result<Vec<u8>, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    match result {
        Ok(stdout) => serde_json::to_writer(
            &mut *w,
            &serde_json::json!({
                "path": path,
                "ok": true,
                "stdout": String::from_utf8_lossy(&stdout),
            }),
        )?,
        Err(stderr) => serde_json::to_writer(
            &mut *w,
            &serde_json::json!({
                "path": path,
                "ok": false,
                "stderr": stderr,
            }),
        )?,
    }
    w.write_all(b"\n")?;
    Ok(())
}

fn help(cmd: Help) -> Result<(), Box<dyn std::error::Error>> {
    let mut root = Opts::command();
    let mut current = &mut root;
    for command in &cmd.commands {
        let Some(next) = current.find_subcommand_mut(command) else {
            eprintln!("error: unknown command '{command}'");
            std::process::exit(2);
        };
        current = next;
    }
    if !cmd.commands.is_empty() {
        current.set_bin_name(format!("gors {}", cmd.commands.join(" ")));
    }
    current.print_help()?;
    println!();
    Ok(())
}

fn version() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "gors version gors{} {} {}/{}",
        env!("CARGO_PKG_VERSION"),
        gors::STDLIB_VERSION,
        go_target_os(),
        go_target_arch()
    );
    Ok(())
}

fn go_target_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    }
}

fn go_target_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86" => "386",
        "x86_64" => "amd64",
        arch => arch,
    }
}

struct RustcArgs<'a> {
    src: &'a str,
    out: Option<&'a str>,
    emit: Option<&'a str>,
    release: bool,
    incremental: Option<&'a str>,
}

impl<'a> From<RustcArgs<'a>> for Vec<String> {
    fn from(args: RustcArgs<'a>) -> Self {
        let mut flags = vec![
            args.src.to_string(),
            "--edition=2024".to_string(),
            "-D".to_string(),
            "unused_imports".to_string(),
            "-D".to_string(),
            "unused_macros".to_string(),
            "-C".to_string(),
            "overflow-checks=off".to_string(),
        ];

        if let Some(emit) = args.emit {
            flags.extend(["--emit".to_string(), emit.to_string()]);
        }

        if let Some(out) = args.out {
            flags.extend(["-o".to_string(), out.to_string()]);
        }

        if let Some(incremental) = args.incremental {
            flags.extend(["-C".to_string(), format!("incremental={incremental}")]);
        }

        if args.release {
            flags.extend([
                "-Ccodegen-units=1".to_string(),
                "-Clto=fat".to_string(),
                "-Copt-level=3".to_string(),
                "-Ctarget-cpu=native".to_string(),
            ]);
        }

        flags
    }
}

impl<'a> IntoIterator for RustcArgs<'a> {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        Vec::from(self).into_iter()
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn split_run_args_keeps_single_file_as_source() {
        let (sources, program_args) = split_run_args(&args(&["main.go", "--", "arg"]));
        assert_eq!(sources, args(&["main.go"]));
        assert_eq!(program_args, args(&["--", "arg"]));
    }

    #[test]
    fn split_run_args_groups_leading_go_files() {
        let (sources, program_args) = split_run_args(&args(&["main.go", "helpers.go", "--flag"]));
        assert_eq!(sources, args(&["main.go", "helpers.go"]));
        assert_eq!(program_args, args(&["--flag"]));
    }

    #[test]
    fn split_run_args_treats_directory_as_single_source() {
        let (sources, program_args) = split_run_args(&args(&[".", "--flag", "value"]));
        assert_eq!(sources, args(&["."]));
        assert_eq!(program_args, args(&["--flag", "value"]));
    }

    #[test]
    fn split_run_args_treats_package_path_as_single_source() {
        let (sources, program_args) = split_run_args(&args(&["./cmd/myapp", "arg"]));
        assert_eq!(sources, args(&["./cmd/myapp"]));
        assert_eq!(program_args, args(&["arg"]));
    }

    #[test]
    fn write_generated_output_removes_files_missing_from_new_manifest() {
        let tmp = tempfile::tempdir().unwrap();

        let mut first_files = BTreeMap::new();
        first_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
        first_files.insert("stale.rs".to_string(), "fn stale() {}\n".to_string());
        let first = gors::printer::GeneratedOutput { files: first_files };
        let first_stats = write_generated_output(&first, tmp.path()).unwrap();
        assert_eq!(first_stats.written, 2);
        assert!(tmp.path().join("stale.rs").exists());

        let mut second_files = BTreeMap::new();
        second_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
        let second = gors::printer::GeneratedOutput {
            files: second_files,
        };
        let second_stats = write_generated_output(&second, tmp.path()).unwrap();

        assert_eq!(second_stats.skipped, 1);
        assert_eq!(second_stats.removed, 1);
        assert!(!tmp.path().join("stale.rs").exists());
    }
}
